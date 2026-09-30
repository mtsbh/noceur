import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { analyseTracks, writeTags, type AnalysisRow, type WriteFailure } from "./api";

/** Files measured per round trip; measured side by side on the other end. */
const BATCH = 4;

/**
 * The tempo ranges on offer. An estimator hears a track at 87 and at 174
 * equally well; the range says which one this library counts in.
 */
const RANGES: [number, number][] = [
  [70, 180],
  [60, 120],
  [85, 175],
  [100, 200],
];

const RANGE_KEY = "noceur.tempoRange";

function storedRange(): [number, number] {
  try {
    const raw = localStorage.getItem(RANGE_KEY);
    const found = RANGES.find((r) => r.join("-") === raw);
    if (found) return found;
  } catch {
    // Storage can be unavailable; the default is fine.
  }
  return RANGES[0];
}

type Field = "bpm" | "key";

/** A tag's BPM as it reads, without the float noise an `f32` brings along. */
const showBpm = (bpm: number | null) => (bpm === null ? "—" : String(Number(bpm.toFixed(2))));

/** Whether writing this field would change the file. */
function differs(r: AnalysisRow, field: Field): boolean {
  if (field === "bpm") {
    return r.bpm !== null && (r.taggedBpm === null || Math.abs(r.bpm - r.taggedBpm) >= 0.05);
  }
  return r.key !== null && r.key !== r.taggedKey;
}

/**
 * Tempo and key for a selection, measured from the audio.
 *
 * Offered, never written by itself: every estimate sits next to what the file
 * already says, and only the ticked ones are written. The ones that differ
 * start ticked, because that is what was asked for; untick any you trust
 * more than the measurement.
 */
export default function Analysis(props: {
  fileIds: number[];
  onClose: () => void;
  onDone: () => void;
}) {
  const [range, setRange] = createSignal<[number, number]>(storedRange());
  const [rows, setRows] = createSignal<AnalysisRow[]>([]);
  const [done, setDone] = createSignal(0);
  const [running, setRunning] = createSignal(true);
  const [error, setError] = createSignal("");
  /** `id:field` pairs the user has unticked. Everything that differs is ticked otherwise. */
  const [skipped, setSkipped] = createSignal<Set<string>>(new Set());
  const [busy, setBusy] = createSignal(false);
  const [failures, setFailures] = createSignal<WriteFailure[]>([]);

  /** Bumped per run, so a run left behind by a range change stops by itself. */
  let run = 0;
  onCleanup(() => {
    run++;
  });

  createEffect(() => {
    const ids = props.fileIds;
    const [min, max] = range();
    const mine = ++run;
    setRows([]);
    setDone(0);
    setError("");
    setRunning(true);

    (async () => {
      for (let at = 0; at < ids.length; at += BATCH) {
        if (mine !== run) return;
        try {
          const batch = await analyseTracks(ids.slice(at, at + BATCH), min, max);
          if (mine !== run) return;
          setRows((current) => [...current, ...batch]);
          setDone(Math.min(at + BATCH, ids.length));
        } catch (e) {
          setError(String(e));
          break;
        }
      }
      if (mine === run) setRunning(false);
    })();
  });

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

  function pickRange(r: [number, number]) {
    try {
      localStorage.setItem(RANGE_KEY, r.join("-"));
    } catch {
      // Remembering it is a convenience.
    }
    setRange(r);
  }

  const ticked = (r: AnalysisRow, f: Field) => differs(r, f) && !skipped().has(`${r.fileId}:${f}`);

  function toggle(r: AnalysisRow, f: Field) {
    const id = `${r.fileId}:${f}`;
    setSkipped((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  const count = (f: Field) => rows().filter((r) => differs(r, f)).length;
  const chosen = createMemo(() =>
    rows().reduce((n, r) => n + Number(ticked(r, "bpm")) + Number(ticked(r, "key")), 0),
  );

  async function write() {
    setBusy(true);
    const failed: WriteFailure[] = [];
    // One write per file: each has its own values, and the editor's command
    // sets one value across a selection.
    for (const r of rows()) {
      const edit: { bpm?: string; key?: string } = {};
      if (ticked(r, "bpm")) edit.bpm = String(r.bpm);
      if (ticked(r, "key")) edit.key = r.key!;
      if (!edit.bpm && !edit.key) continue;
      try {
        const report = await writeTags([r.fileId], edit);
        failed.push(...report.failures);
      } catch (e) {
        failed.push({ name: r.name, reason: String(e) });
      }
    }
    setBusy(false);
    props.onDone();
    if (failed.length === 0) props.onClose();
    else setFailures(failed);
  }

  /** One field of a row: the tag, what was measured, and whether to write it. */
  const cell = (r: AnalysisRow, f: Field) => {
    const tagged = f === "bpm" ? showBpm(r.taggedBpm) : (r.taggedKey ?? "—");
    const found = f === "bpm" ? r.bpm : r.key;
    const same = found !== null && !differs(r, f);
    const hint =
      f === "key" && r.keyConfidence !== null
        ? `${Math.round(r.keyConfidence * 100)}% sure`
        : (r.error ?? undefined);
    return (
      <>
        <span class="tempotag mono">{tagged}</span>
        <span class="renamearrow">→</span>
        <span class={`tempofound mono ${same ? "same" : ""} ${found === null ? "none" : ""}`} title={hint}>
          <Show when={found !== null} fallback={r.error ? "error" : "—"}>
            {same ? "same" : f === "bpm" ? showBpm(r.bpm) : r.key}
          </Show>
        </span>
        <input
          type="checkbox"
          disabled={!differs(r, f)}
          checked={ticked(r, f)}
          onChange={() => toggle(r, f)}
          aria-label={`Write ${f === "bpm" ? "BPM" : "key"} for ${r.name}`}
        />
      </>
    );
  };

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer wide" role="dialog" aria-label="Detect BPM and key">
        <header class="sethead">
          <span class="settitle">Detect BPM &amp; key</span>
          <span class="tagsubject">
            <Show
              when={running()}
              fallback={`${rows().length} ${rows().length === 1 ? "track" : "tracks"}`}
            >
              {done()} of {props.fileIds.length}…
            </Show>
          </span>
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

        <div class="renamebody">
          <div class="setseg" role="group" aria-label="BPM range">
            <For each={RANGES}>
              {(r) => (
                <button
                  class={`segbtn ${range() === r ? "on" : ""}`}
                  onClick={() => pickRange(r)}
                >
                  {r[0]}–{r[1]}
                </button>
              )}
            </For>
          </div>

          <Show when={rows().length > 0} fallback={<p class="sethint">Listening…</p>}>
            <div class="quallist">
              <div class="temporow head">
                <span />
                <span class="tempofield">BPM</span>
                <span class="tempofield">Key</span>
              </div>
              <For each={rows()}>
                {(r) => (
                  <div class="temporow">
                    <span class="qualname" title={r.name}>
                      {r.name}
                    </span>
                    {cell(r, "bpm")}
                    {cell(r, "key")}
                  </div>
                )}
              </For>
            </div>
          </Show>

          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>

          <Show when={failures().length > 0}>
            <div class="tagfailures">
              <p>Failed {failures().length}:</p>
              <For each={failures()}>
                {(f) => (
                  <p class="tagfail">
                    <b>{f.name}</b> — {f.reason}
                  </p>
                )}
              </For>
            </div>
          </Show>
        </div>

        <footer class="tagfoot">
          <span class="sethint credit">
            <Show when={!running()} fallback="Analyzing…">
              <Show when={count("bpm") + count("key") > 0} fallback="All match">
                {count("bpm")} BPM · {count("key")} keys differ
              </Show>
            </Show>
          </span>
          <button class="btn" onClick={props.onClose}>
            Cancel
          </button>
          <button class="btn primary" disabled={chosen() === 0 || busy()} onClick={write}>
            {busy() ? "Writing…" : `Write ${chosen()}`}
          </button>
        </footer>
      </div>
    </>
  );
}
