import { For, Show, createEffect, createResource, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { overview, type Overview as Stats, type Tally } from "./api";
import { keyColour } from "./columns";

/** BPM per bar of the histogram. */
const BPM_STEP = 2;
/** The tempo chart's range. */
const BPM_MIN = 60;
const BPM_MAX = 200;

/**
 * The library at a glance.
 *
 * Read from the index, so it opens at once and covers the folders Noceur has
 * opened; nothing is decoded. BPM and key fall back to what was measured where
 * the tags say nothing, the same as the list.
 */
export default function Overview(props: {
  /** The focused folder, offered as a narrower scope. */
  folder: string;
  onOpen: (path: string) => void;
  onClose: () => void;
}) {
  const [onlyHere, setOnlyHere] = createSignal(false);
  // Wrapped in an object: a falsy source would stop the resource from fetching.
  const [stats] = createResource(
    () => ({ scope: onlyHere() ? props.folder : null }),
    ({ scope }) => overview(scope),
  );

  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer wide overview" role="dialog" aria-label="Library overview">
        <header class="sethead">
          <span class="settitle">Library</span>
          <label class="ovscope">
            <input
              type="checkbox"
              checked={onlyHere()}
              onChange={(e) => setOnlyHere(e.currentTarget.checked)}
            />
            <span>Only {folderName(props.folder)}</span>
          </label>
          <button
            class="setclose"
            onClick={props.onClose}
            data-tip="Close"
            data-tip-at="end"
            aria-label="Close"
          >
            <CloseIcon />
          </button>
        </header>

        <div class="renamebody ovbody">
          <Show when={stats.error}>
            <p class="tagerror">{String(stats.error)}</p>
          </Show>
          <Show when={stats.latest} fallback={<p class="sethint">Reading the index…</p>}>
            {(s) => (
              <Show
                when={s().tracks > 0}
                fallback={<p class="sethint">No tracks indexed here yet</p>}
              >
                <Body stats={s()} onOpen={props.onOpen} />
              </Show>
            )}
          </Show>
        </div>
      </div>
    </>
  );
}

function Body(props: { stats: Stats; onOpen: (path: string) => void }) {
  const s = () => props.stats;
  const pct = (n: number) => (s().tracks ? Math.round((n / s().tracks) * 100) : 0);

  return (
    <>
      <div class="ovhead">
        <Figure value={s().tracks.toLocaleString()} label="tracks" />
        <Figure value={hours(s().seconds)} label="playing time" />
        <Figure value={bytes(s().bytes)} label="on disk" />
        <Figure value={s().folders.toLocaleString()} label="folders" />
      </div>

      <div class="ovgrid">
        <section class="ovcard ovwide">
          <h3>
            BPM <span class="ovnote">{tagged(s().tracks - s().noBpm, s().tracks)}</span>
          </h3>
          <BpmChart bpm={s().bpm} />
        </section>

        <section class="ovcard">
          <h3>
            Keys <span class="ovnote">{tagged(s().tracks - s().noKey, s().tracks)}</span>
          </h3>
          <Wheel keys={s().keys} />
        </section>

        <section class="ovcard">
          <h3>Formats</h3>
          <Bars items={s().formats} total={s().tracks} />
          <h3 class="ovsub">Sample rates</h3>
          <Bars
            items={s().sampleRates.map((t) => ({ ...t, value: rate(t.value) }))}
            total={s().tracks}
          />
        </section>

        <section class="ovcard">
          <h3>Quality</h3>
          <div class="ovfacts">
            <Fact label="Checked" value={s().checked} of={s().tracks} />
            <Fact label="Damaged" value={s().damaged} bad />
            <Fact label="Unreadable" value={s().unreadable} bad />
          </div>
          <Show when={s().lufs.length > 0}>
            <h3 class="ovsub">Loudness (LUFS)</h3>
            <Histogram
              points={s().lufs}
              tip={(v, n) => `${v} LUFS · ${n}`}
              axis={(v) => v % 4 === 0}
            />
          </Show>
          <Show when={s().checked === 0}>
            <p class="sethint">Run Check quality to fill this in</p>
          </Show>
        </section>

        <section class="ovcard">
          <h3>Missing</h3>
          <div class="ovfacts">
            <Fact label="Artist" value={s().noArtist} pct={pct(s().noArtist)} />
            <Fact label="Title" value={s().noTitle} pct={pct(s().noTitle)} />
            <Fact label="Genre" value={s().noGenre} pct={pct(s().noGenre)} />
            <Fact label="BPM" value={s().noBpm} pct={pct(s().noBpm)} />
            <Fact label="Key" value={s().noKey} pct={pct(s().noKey)} />
            <Fact label="Cover art" value={s().noArt} pct={pct(s().noArt)} />
          </div>
        </section>

        <div class="ovtops">
          <Top title="Top labels" items={s().labels} none="No labels tagged" />
          <Top title="Top genres" items={s().genres} none="No genres tagged" />
          <Top title="Top artists" items={s().artists} none="No artists tagged" />
        </div>

        <Show when={s().problemFolders.length > 0}>
          <section class="ovcard ovwide">
            <h3>Folders with the most problems</h3>
            <For each={s().problemFolders}>
              {(f) => (
                <div class="ovfolder" onClick={() => props.onOpen(f.path)} data-tip="Open">
                  <span class="ovpath">{f.path}</span>
                  <span class="ovbad mono">
                    {[
                      f.damaged ? `${f.damaged} damaged` : "",
                      f.unreadable ? `${f.unreadable} unreadable` : "",
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </span>
                  <span class="ovcount mono">of {f.tracks}</span>
                </div>
              )}
            </For>
          </section>
        </Show>
      </div>
    </>
  );
}

function Figure(props: { value: string; label: string }) {
  return (
    <div class="ovfigure">
      <span class="ovvalue">{props.value}</span>
      <span class="ovlabel">{props.label}</span>
    </div>
  );
}

function Fact(props: { label: string; value: number; of?: number; pct?: number; bad?: boolean }) {
  return (
    <div class={`ovfact ${props.bad && props.value > 0 ? "bad" : ""}`}>
      <span>{props.label}</span>
      <span class="mono">
        {props.value.toLocaleString()}
        <Show when={props.of !== undefined}>
          <span class="ovnote"> of {props.of!.toLocaleString()}</span>
        </Show>
        <Show when={props.pct !== undefined}>
          <span class="ovnote"> · {props.pct}%</span>
        </Show>
      </span>
    </div>
  );
}

function Top(props: { title: string; items: Tally[]; none: string }) {
  return (
    <section class="ovcard">
      <h3>{props.title}</h3>
      <Show when={props.items.length} fallback={<p class="sethint">{props.none}</p>}>
        <Bars items={props.items} total={props.items[0]?.count ?? 1} />
      </Show>
    </section>
  );
}

/** A ranked list, each row with a bar against `total`. */
function Bars(props: { items: Tally[]; total: number }) {
  return (
    <div class="ovbars">
      <For each={props.items}>
        {(t) => (
          <div class="ovbar">
            <span class="ovbarname" data-tip={t.value}>
              {t.value}
            </span>
            <span class="ovbartrack">
              <span
                class="ovbarfill"
                style={{ width: `${Math.max(2, (t.count / Math.max(1, props.total)) * 100)}%` }}
              />
            </span>
            <span class="ovcount mono">{t.count.toLocaleString()}</span>
          </div>
        )}
      </For>
    </div>
  );
}

/** Columns over a run of whole numbers, with gaps kept as gaps. */
function Histogram(props: {
  points: [number, number][];
  tip: (value: number, count: number) => string;
  axis: (value: number) => boolean;
  step?: number;
}) {
  const columns = () => {
    const step = props.step ?? 1;
    const counts = new Map<number, number>();
    for (const [v, n] of props.points) {
      const at = Math.floor(v / step) * step;
      counts.set(at, (counts.get(at) ?? 0) + n);
    }
    const keys = [...counts.keys()];
    if (keys.length === 0) return [];
    const lo = Math.min(...keys);
    const hi = Math.max(...keys);
    const out: [number, number][] = [];
    for (let v = lo; v <= hi; v += step) out.push([v, counts.get(v) ?? 0]);
    return out;
  };
  const peak = () => Math.max(1, ...columns().map(([, n]) => n));

  return (
    <div class="ovhist">
      <div class="ovcols">
        <For each={columns()}>
          {([v, n]) => (
            <span class="ovcol" data-tip={props.tip(v, n)}>
              <span class="ovcolfill" style={{ height: `${(n / peak()) * 100}%` }} />
            </span>
          )}
        </For>
      </div>
      <div class="ovaxis">
        <For each={columns()}>
          {([v]) => <span class="ovtick">{props.axis(v) ? v : ""}</span>}
        </For>
      </div>
    </div>
  );
}

/**
 * Tracks by tempo. Half-time and double-time readings are left where they
 * fall: a library of 87s and 174s is a drum & bass library, not a mistake.
 */
function BpmChart(props: { bpm: [number, number][] }) {
  // A mistyped tag (1280 for 128) would stretch the axis to nothing, so the
  // chart keeps to where dance music lives and counts the rest.
  const inside = () => props.bpm.filter(([v]) => v >= BPM_MIN && v <= BPM_MAX);
  const outside = () =>
    props.bpm.reduce((n, [v, c]) => (v < BPM_MIN || v > BPM_MAX ? n + c : n), 0);

  return (
    <Show when={inside().length > 0} fallback={<p class="sethint">No BPM known</p>}>
      <Histogram
        points={inside()}
        step={BPM_STEP}
        tip={(v, n) => `${v}–${v + BPM_STEP - 1} BPM · ${n}`}
        axis={(v) => v % 10 === 0}
      />
      <Show when={outside() > 0}>
        <p class="sethint">
          {outside()} outside {BPM_MIN}–{BPM_MAX}
        </p>
      </Show>
    </Show>
  );
}

/**
 * Keys on the Camelot wheel: majors (B) on the outer ring, minors (A) inside,
 * each segment as bright as its share of the busiest key. Neighbours on the
 * wheel mix, so a lit arc is a set of tracks that can follow each other.
 */
function Wheel(props: { keys: number[] }) {
  const peak = () => Math.max(1, ...props.keys);
  const size = 220;
  const c = size / 2;

  /** An annular sector for wheel position `i` (0 = 1A/1B at the top-ish). */
  const sector = (i: number, r0: number, r1: number) => {
    // Number 12 sits at the top on the printed wheel; 1 is one step clockwise.
    const a0 = ((i + 1) * 30 - 15 - 90) * (Math.PI / 180);
    const a1 = a0 + (30 * Math.PI) / 180;
    const p = (r: number, a: number) => `${c + r * Math.cos(a)} ${c + r * Math.sin(a)}`;
    return `M ${p(r1, a0)} A ${r1} ${r1} 0 0 1 ${p(r1, a1)} L ${p(r0, a1)} A ${r0} ${r0} 0 0 0 ${p(r0, a0)} Z`;
  };
  const labelAt = (i: number, r: number) => {
    const a = ((i + 1) * 30 - 90) * (Math.PI / 180);
    return { x: c + r * Math.cos(a), y: c + r * Math.sin(a) };
  };

  const rings = [
    { minor: false, r0: 74, r1: 108, label: 91 },
    { minor: true, r0: 40, r1: 72, label: 56 },
  ];

  return (
    <svg class="ovwheel" viewBox={`0 0 ${size} ${size}`} role="img" aria-label="Keys">
      <For each={rings}>
        {(ring) => (
          <For each={Array.from({ length: 12 }, (_, i) => i)}>
            {(i) => {
              const canonical = i * 2 + (ring.minor ? 0 : 1);
              const n = () => props.keys[canonical] ?? 0;
              const name = `${i + 1}${ring.minor ? "A" : "B"}`;
              const at = labelAt(i, ring.label);
              return (
                <g data-tip={`${name} · ${n()}`}>
                  <path
                    d={sector(i, ring.r0, ring.r1)}
                    fill={keyColour(canonical)}
                    fill-opacity={n() ? 0.15 + 0.85 * (n() / peak()) : 0.06}
                    class="ovseg"
                  />
                  <text x={at.x} y={at.y} class={`ovkey ${n() ? "" : "none"}`}>
                    {name}
                  </text>
                </g>
              );
            }}
          </For>
        )}
      </For>
    </svg>
  );
}

function tagged(n: number, of: number) {
  return of ? `${Math.round((n / of) * 100)}% known` : "";
}

function hours(seconds: number) {
  const h = seconds / 3600;
  if (h >= 48) return `${Math.round(h / 24).toLocaleString()} days`;
  if (h >= 1) return `${Math.round(h)} h`;
  return `${Math.round(seconds / 60)} min`;
}

function bytes(n: number) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n >= 100 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
}

/** `"44100/16"` → `"44.1 kHz · 16-bit"`. */
function rate(value: string) {
  const [hz, depth] = value.split("/");
  if (!hz || hz === "?") return "Unknown";
  const khz = `${(Number(hz) / 1000).toString()} kHz`;
  return depth ? `${khz} · ${depth}-bit` : khz;
}

function folderName(path: string) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
