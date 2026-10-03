import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { analyseQuality, writeReplayGain, type QualityRow } from "./api";

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
export type QualityRun = {
  fileIds: number[];
  rows: () => QualityRow[];
  done: () => number;
  running: () => boolean;
  error: () => string;
  stop: () => void;
};

/**
 * Starts checking files. Lives outside the dialog, so hiding the dialog to get
 * back to the list leaves a two-hundred-file check running.
 */
export function startQuality(ids: number[]): QualityRun {
  const [rows, setRows] = createSignal<QualityRow[]>([]);
  const [done, setDone] = createSignal(0);
  const [running, setRunning] = createSignal(true);
  const [error, setError] = createSignal("");
  let stopped = false;

  (async () => {
    for (let at = 0; at < ids.length; at += BATCH) {
      if (stopped) break;
      try {
        const batch = await analyseQuality(ids.slice(at, at + BATCH));
        if (stopped) break;
        setRows((current) => [...current, ...batch]);
        setDone(Math.min(at + BATCH, ids.length));
      } catch (e) {
        setError(String(e));
        break;
      }
    }
    setRunning(false);
  })();

  return {
    fileIds: ids,
    rows,
    done,
    running,
    error,
    stop: () => {
      stopped = true;
      setRunning(false);
    },
  };
}

export default function Quality(props: {
  run: QualityRun;
  /** Hides the report; the check carries on. */
  onHide: () => void;
  /** Drops the report, stopping the check if it is still going. */
  onClose: () => void;
  /** Selects these files in the list, so a verdict can be acted on. */
  onSelect: (fileIds: number[]) => void;
}) {
  const rows = () => props.run.rows();
  const done = () => props.run.done();
  const running = () => props.run.running();
  const error = () => props.run.error();

  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        props.onHide();
      }
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  /**
   * Worst first — a report you have to scroll to find the problems in is a
   * list. A damaged file goes ahead of everything: a transcode still plays.
   */
  const sorted = () =>
    [...rows()].sort(
      (a, b) =>
        Number(b.damage !== "") - Number(a.damage !== "") ||
        ORDER.indexOf(a.verdict) - ORDER.indexOf(b.verdict),
    );

  /** Each file is tallied once: a damaged file under damaged, not also its verdict. */
  const sound = () => rows().filter((r) => r.damage === "");
  const count = (verdict: string) =>
    sound().filter((r) => r.verdict === verdict).length;

  /** Broken rather than lossy: cut short, dropouts, decode errors. */
  const damaged = () => rows().filter((r) => r.damage !== "");

  const suspect = () =>
    rows().filter(
      (r) =>
        r.damage !== "" ||
        r.verdict === "upscaled" ||
        r.verdict === "transcoded" ||
        r.verdict === "upsampled",
    ).length;

  /**
   * Whether a file clips enough to be worth mentioning.
   *
   * Both conditions, because either alone misleads. A track can touch full
   * scale on a hundredth of a percent of its samples and be badly distorted if
   * those are one long run; and a run of three is a rounding artefact unless
   * there are many of them.
   */
  const clipped = (r: QualityRow) => r.clipRun >= 8 && r.clipPercent >= 0.01;

  /** True peak with its sign, `≥` when only a lower bound. */
  const peak = (r: QualityRow) => {
    if (r.truePeak === null) return "—";
    const v = Math.round(r.truePeak * 10) / 10;
    const text = v === 0 ? "0.0" : `${v > 0 ? "+" : ""}${v.toFixed(1)}`;
    return r.peakFloor ? `≥ ${text}` : text;
  };
  /** Over full scale once reconstructed: it clips in a DAC or a decoder. */
  const over = (r: QualityRow) => r.truePeak !== null && (r.truePeak > 0 || r.peakFloor);

  const measured = () => rows().filter((r) => r.lufs !== null);
  const [gainNote, setGainNote] = createSignal("");
  const [writing, setWriting] = createSignal(false);
  async function replayGain() {
    setWriting(true);
    try {
      const r = await writeReplayGain(measured().map((m) => m.fileId));
      setGainNote(
        r.failures.length
          ? `ReplayGain on ${r.written} · ${r.failures.length} failed`
          : `ReplayGain on ${r.written}`,
      );
    } catch (e) {
      setGainNote(String(e));
    } finally {
      setWriting(false);
    }
  }

  /** Share of samples in a clipped run, `0.42%`, `<0.01%` for a trace. */
  const clipText = (r: QualityRow) =>
    r.clipPercent < 0.01 ? "<0.01%" : `${r.clipPercent.toFixed(2)}%`;
  /** The run, and where the tops sit when the track was turned down after clipping. */
  const clipTip = (r: QualityRow) =>
    r.clipDb < 0
      ? `Clipped, then turned down to ${r.clipDb.toFixed(2).replace("-", "−")} dB · longest run ${r.clipRun}`
      : `Longest run ${r.clipRun} samples`;

  /**
   * What the spectrum says the file really is: the bitrate its cutoff is
   * typical of, or `lossless` / `full` when the content runs to the top.
   */
  const real = (r: QualityRow) => {
    if (r.verdict === "unreadable") return "—";
    if (r.impliedKbps) return `${r.impliedKbps}k`;
    if (r.verdict === "upsampled") return "—";
    return r.lossless ? "lossless" : "full";
  };

  /** What the file says it is, for the left-hand column. */
  const claim = (r: QualityRow) => {
    if (r.lossless) return "lossless";
    return r.claimedKbps ? `${r.claimedKbps}k` : "—";
  };

  return (
    <>
      <div class="scrim" onPointerDown={props.onHide} />
      <div class="renamer wide" role="dialog" aria-label="Check quality">
        <header class="sethead">
          <span class="settitle">Check quality</span>
          <span class="tagsubject">
            <Show
              when={running()}
              fallback={`${rows().length} ${rows().length === 1 ? "track" : "tracks"}`}
            >
              {done()} of {props.run.fileIds.length}…
            </Show>
          </span>
          <button
            class="setclose"
            onClick={props.onHide}
            data-tip={running() ? "Hide, keep checking" : "Hide"}
            data-tip-at="end"
            aria-label="Close"
          >
            <CloseIcon />
          </button>
        </header>

        <div class="renamebody">
          <Show when={rows().length > 0} fallback={<p class="sethint">Reading…</p>}>
            <div class="qualsummary">
              <Show when={damaged().length > 0}>
                <button
                  class="qualtally damaged"
                  data-tip="Select in list"
                  onClick={() => props.onSelect(damaged().map((r) => r.fileId))}
                >
                  {damaged().length} damaged
                </button>
              </Show>
              <For each={ORDER}>
                {(verdict) => (
                  <Show when={count(verdict) > 0}>
                    <button
                      class={`qualtally ${verdict}`}
                      data-tip="Select in list"
                      onClick={() =>
                        props.onSelect(
                          sound()
                            .filter((r) => r.verdict === verdict)
                            .map((r) => r.fileId),
                        )
                      }
                    >
                      {count(verdict)} {verdict}
                    </button>
                  </Show>
                )}
              </For>
            </div>

            <div class="quallist">
              <div class="qualrow qualhead">
                <span />
                <span>Track</span>
                <span class="qualclaim" data-tip="What the file's header says">Claimed bitrate</span>
                <span class="qualreal" data-tip="What the audio itself shows it was encoded at">Actual bitrate</span>
                <span class="qualcut" data-tip="Highest frequency with real signal">Cutoff</span>
                <span class="qualclip" data-tip="Samples in flat tops, at full scale or turned down">Clip</span>
                <span class="quallufs" data-tip="Integrated loudness, LUFS">LUFS</span>
                <span class="qualpeak" data-tip="True peak, dBTP">Peak</span>
                <span class="qualwhy">Result</span>
              </div>
              <For each={sorted()}>
                {(r) => (
                  <div class={`qualrow ${r.verdict}`}>
                    <span class={`qualmark ${r.damage ? "damaged" : r.verdict}`} />
                    <span class="qualname" title={r.name}>
                      {r.name}
                    </span>
                    <span class="qualclaim mono">{claim(r)}</span>
                    <span class={`qualreal mono ${r.verdict === "upscaled" || r.verdict === "transcoded" ? "bad" : ""}`}>
                      {real(r)}
                    </span>
                    <span class="qualcut mono">
                      <Show when={r.cutoffKhz > 0} fallback="—">
                        {r.cutoffKhz.toFixed(1)} kHz
                      </Show>
                    </span>
                    {/* Any clipping at all, red once it is enough to hear. Blank
                        when there is none: a column of "0%" is noise. */}
                    <span
                      class={`qualclip mono ${clipped(r) ? "bad" : ""}`}
                      data-tip={r.clipPercent > 0 ? clipTip(r) : undefined}
                    >
                      <Show when={r.clipPercent > 0}>
                        {clipText(r)}
                        <Show when={r.clipDb < 0}> ↓</Show>
                      </Show>
                    </span>
                    <span class="quallufs mono" data-tip="Integrated loudness">
                      {r.lufs === null ? "—" : r.lufs.toFixed(1)}
                    </span>
                    <span
                      class={`qualpeak mono ${over(r) ? "bad" : ""}`}
                      data-tip={
                        r.peakFloor
                          ? "True peak, at least: the decoder stops at full scale"
                          : "True peak, dBTP"
                      }
                    >
                      {peak(r)}
                    </span>
                    <span class="qualwhy" title={r.damage ? `${r.damage} · ${r.reason}` : r.reason}>
                      <Show when={r.damage}>
                        <span class="qualdamage">{r.damage}</span> ·{" "}
                      </Show>
                      {r.reason}
                    </span>
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
            <Show when={!running()} fallback="Analyzing…">
              <Show when={gainNote()} fallback={
                <Show when={suspect() > 0} fallback="No issues">
                  {suspect()} of {rows().length} suspect
                </Show>
              }>
                {gainNote()}
              </Show>
            </Show>
          </span>
          <Show when={!running() && measured().length > 0}>
            <button
              class="btn"
              disabled={writing()}
              data-tip="Track gain to -18 LUFS, and true peak"
              onClick={replayGain}
            >
              {writing() ? "Writing…" : "Write ReplayGain"}
            </button>
          </Show>
          <Show
            when={running()}
            fallback={
              <button class="btn" onClick={props.onClose}>
                Close
              </button>
            }
          >
            <button class="btn" onClick={props.onHide}>
              Hide
            </button>
            <button class="btn" onClick={() => props.run.stop()}>
              Stop
            </button>
          </Show>
        </footer>
      </div>
    </>
  );
}
