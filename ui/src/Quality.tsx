import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { analyseQuality, type QualityRow } from "./api";

/**
 * Files analysed per round trip.
 *
 * Small, because the progress count only moves when a batch comes back and a
 * larger one would make the dialog look stuck. Each file is roughly half a
 * second, so this is about two seconds of work per update.
 */
const BATCH = 4;

/** The order verdicts are worth reading in: the problems first. */
const ORDER = [
  "upscaled",
  "transcoded",
  "upsampled",
  "unsure",
  "unreadable",
  "genuine",
];

/**
 * What a selection of files actually is.
 *
 * Every lossy encoder throws the top of the spectrum away, and the higher the
 * bitrate the less it throws. That leaves a signature a header cannot fake: a
 * file claiming 320 kbps that stops dead at 17 kHz was 160 kbps before somebody
 * re-encoded it, and a FLAC that stops at 16 kHz was never lossless.
 *
 * Nothing is written and nothing is changed. This only ever tells you what you
 * have.
 */
export default function Quality(props: {
  fileIds: number[];
  onClose: () => void;
}) {
  const [rows, setRows] = createSignal<QualityRow[]>([]);
  const [done, setDone] = createSignal(0);
  const [running, setRunning] = createSignal(true);
  const [error, setError] = createSignal("");
  /** Set when the dialog closes, so a batch in flight stops queueing more. */
  let abandoned = false;

  onCleanup(() => {
    abandoned = true;
  });

  createEffect(() => {
    const ids = props.fileIds;

    (async () => {
      for (let at = 0; at < ids.length; at += BATCH) {
        if (abandoned) return;
        try {
          const batch = await analyseQuality(ids.slice(at, at + BATCH));
          if (abandoned) return;
          setRows((current) => [...current, ...batch]);
          setDone(Math.min(at + BATCH, ids.length));
        } catch (e) {
          setError(String(e));
          break;
        }
      }
      setRunning(false);
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

  /** Worst first — a report you have to scroll to find the problems in is a list. */
  const sorted = () =>
    [...rows()].sort(
      (a, b) => ORDER.indexOf(a.verdict) - ORDER.indexOf(b.verdict),
    );

  const count = (verdict: string) =>
    rows().filter((r) => r.verdict === verdict).length;

  const suspect = () =>
    count("upscaled") + count("transcoded") + count("upsampled");

  /** What the file says it is, for the left-hand column. */
  const claim = (r: QualityRow) => {
    if (r.lossless) return "lossless";
    return r.claimedKbps ? `${r.claimedKbps}k` : "—";
  };

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer wide" role="dialog" aria-label="Check quality">
        <header class="sethead">
          <span class="settitle">Check quality</span>
          <span class="tagsubject">
            <Show
              when={running()}
              fallback={`${rows().length} ${rows().length === 1 ? "track" : "tracks"}`}
            >
              {done()} of {props.fileIds.length}…
            </Show>
          </span>
          <button class="setclose" onClick={props.onClose} title="Close (Esc)">
            <CloseIcon />
          </button>
        </header>

        <div class="renamebody">
          <Show when={rows().length > 0} fallback={<p class="sethint">Reading…</p>}>
            <div class="qualsummary">
              <For each={ORDER}>
                {(verdict) => (
                  <Show when={count(verdict) > 0}>
                    <span class={`qualtally ${verdict}`}>
                      {count(verdict)} {verdict}
                    </span>
                  </Show>
                )}
              </For>
            </div>

            <div class="quallist">
              <For each={sorted()}>
                {(r) => (
                  <div class={`qualrow ${r.verdict}`}>
                    <span class={`qualmark ${r.verdict}`} />
                    <span class="qualname" title={r.name}>
                      {r.name}
                    </span>
                    <span class="qualclaim mono">{claim(r)}</span>
                    <span class="qualcut mono">
                      <Show when={r.cutoffKhz > 0} fallback="—">
                        {r.cutoffKhz.toFixed(1)} kHz
                      </Show>
                    </span>
                    <span class="qualwhy">{r.reason}</span>
                  </div>
                )}
              </For>
            </div>
          </Show>

          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>
        </div>

        <footer class="tagfoot">
          <span class="sethint credit">
            <Show
              when={!running()}
              fallback="Reading each track end to end — about half a second each."
            >
              <Show
                when={suspect() > 0}
                fallback="Nothing here looks re-encoded."
              >
                {suspect()} worth looking at. A cutoff well below what the
                bitrate claims means the file was encoded lower and re-wrapped.
              </Show>
            </Show>
          </span>
          <button class="btn" onClick={props.onClose}>
            {running() ? "Stop" : "Close"}
          </button>
        </footer>
      </div>
    </>
  );
}
