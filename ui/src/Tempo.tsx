import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { detectTempo, writeTags, type TempoRow, type WriteFailure } from "./api";

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

/** A tag's BPM as it reads, without the float noise an `f32` brings along. */
const show = (bpm: number | null) => (bpm === null ? "—" : String(Number(bpm.toFixed(2))));

/** The estimate agrees with the tag closely enough that writing it changes nothing. */
const same = (r: TempoRow) =>
  r.detected !== null && r.tagged !== null && Math.abs(r.detected - r.tagged) < 0.05;

/**
 * Tempo for a selection, measured from the audio.
 *
 * Offered, never written by itself: every estimate sits next to what the file
 * already says, and only the ticked ones are written. The ones that differ
 * start ticked, because that is what was asked for; untick any you trust
 * more than the measurement.
 */
export default function Tempo(props: {
  fileIds: number[];
  onClose: () => void;
  onDone: () => void;
}) {
  const [range, setRange] = createSignal<[number, number]>(storedRange());
  const [rows, setRows] = createSignal<TempoRow[]>([]);
  const [done, setDone] = createSignal(0);
  const [running, setRunning] = createSignal(true);
  const [error, setError] = createSignal("");
  /** Files the user has unticked. Everything that differs is ticked otherwise. */
  const [skipped, setSkipped] = createSignal<Set<number>>(new Set());
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
          const batch = await detectTempo(ids.slice(at, at + BATCH), min, max);
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

  /** Rows that would change the file if written. */
  const differing = createMemo(() => rows().filter((r) => r.detected !== null && !same(r)));
  const chosen = createMemo(() => differing().filter((r) => !skipped().has(r.fileId)));

  function toggle(id: number) {
    setSkipped((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function write() {
    setBusy(true);
    const failed: WriteFailure[] = [];
    // One write per file: each has its own value, and the editor's command
    // sets one value across a selection.
    for (const r of chosen()) {
      try {
        const report = await writeTags([r.fileId], { bpm: String(r.detected) });
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

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer" role="dialog" aria-label="Detect BPM">
        <header class="sethead">
          <span class="settitle">Detect BPM</span>
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
          <div class="setseg" role="group" aria-label="Range">
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
              <For each={rows()}>
                {(r) => (
                  <label
                    class={`temporow ${same(r) ? "same" : ""} ${r.detected === null ? "none" : ""}`}
                  >
                    <input
                      type="checkbox"
                      disabled={r.detected === null || same(r)}
                      checked={r.detected !== null && !same(r) && !skipped().has(r.fileId)}
                      onChange={() => toggle(r.fileId)}
                    />
                    <span class="qualname" title={r.name}>
                      {r.name}
                    </span>
                    <span class="tempotag mono">{show(r.tagged)}</span>
                    <span class="renamearrow">→</span>
                    <span class="tempofound mono" title={r.error ?? undefined}>
                      <Show when={r.detected !== null} fallback={r.error ? "error" : "no beat"}>
                        {same(r) ? "same" : show(r.detected)}
                      </Show>
                    </span>
                  </label>
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
              <Show when={differing().length > 0} fallback="All match">
                {differing().length} differ
              </Show>
            </Show>
          </span>
          <button class="btn" onClick={props.onClose}>
            Cancel
          </button>
          <button
            class="btn primary"
            disabled={chosen().length === 0 || busy()}
            onClick={write}
          >
            {busy() ? "Writing…" : `Write ${chosen().length}`}
          </button>
        </footer>
      </div>
    </>
  );
}
