import { createSignal, createEffect, onCleanup, Show } from "solid-js";
import { transportCmd, transportState, type Transport as T } from "./api";

/**
 * How often the position is re-read while playing.
 *
 * The audio thread already knows the position exactly; this only decides how
 * smoothly the bar moves. Ten times a second is smooth enough to read and cheap
 * enough to ignore.
 */
const TICK_MS = 100;

/**
 * The transport bar.
 *
 * Full width at the bottom of the window, per the spec — it survives split mode,
 * so it belongs to the window rather than to a pane. This is the seek surface
 * until the waveform replaces it.
 */
export default function Transport(props: {
  state: T | null;
  onState: (t: T) => void;
}) {
  const [scrubbing, setScrubbing] = createSignal<number | null>(null);
  let bar: HTMLDivElement | undefined;

  // Poll only while something is actually playing. A paused or empty transport
  // has nothing to move, and polling it would be a timer that never sleeps.
  createEffect(() => {
    if (!props.state?.playing) return;
    const timer = window.setInterval(() => {
      transportState().then(props.onState).catch(() => {});
    }, TICK_MS);
    onCleanup(() => window.clearInterval(timer));
  });

  const duration = () => props.state?.durationMs ?? 0;

  /** Fraction played, 0..1. Uses the scrub position while a drag is in progress. */
  const progress = () => {
    const d = duration();
    if (d <= 0) return 0;
    const at = scrubbing() ?? props.state?.positionMs ?? 0;
    return Math.min(1, Math.max(0, at / d));
  };

  function positionFrom(e: PointerEvent): number {
    if (!bar || duration() <= 0) return 0;
    const box = bar.getBoundingClientRect();
    const fraction = (e.clientX - box.left) / box.width;
    return Math.round(Math.min(1, Math.max(0, fraction)) * duration());
  }

  /**
   * Click or drag to seek.
   *
   * The bar follows the pointer during the drag and the seek is sent once on
   * release: seeking on every pointer move would ask the decoder to re-seek
   * dozens of times across a drag it is only going to finish once.
   */
  function startScrub(e: PointerEvent) {
    if (duration() <= 0) return;
    e.preventDefault();
    setScrubbing(positionFrom(e));

    const move = (ev: PointerEvent) => setScrubbing(positionFrom(ev));
    const stop = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      const target = positionFrom(ev);
      setScrubbing(null);
      transportCmd("seek", target).then(props.onState).catch(() => {});
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  const act = (action: Parameters<typeof transportCmd>[0]) => () =>
    transportCmd(action).then(props.onState).catch(() => {});

  return (
    <div class={`transport ${props.state?.path ? "" : "idle"}`}>
      <button
        class="tbtn"
        onClick={act("toggle")}
        disabled={!props.state?.path}
        title={props.state?.playing ? "Pause (space)" : "Play (space)"}
      >
        {props.state?.playing ? "❚❚" : "▶"}
      </button>
      <button
        class="tbtn"
        onClick={act("stop")}
        disabled={!props.state?.path}
        title="Stop"
      >
        ■
      </button>

      <span class="tname" title={props.state?.path || ""}>
        {props.state?.name || "Nothing loaded"}
      </span>

      <span class="ttime mono">{props.state?.position ?? "0:00"}</span>

      {/* The seek surface. A waveform draws here in the next milestone; the
          geometry and the drag behaviour are the same either way. */}
      <div
        class="tbar"
        ref={(el) => (bar = el)}
        onPointerDown={startScrub}
        title={duration() > 0 ? "Click or drag to seek" : ""}
      >
        <div class="tfill" style={{ width: `${progress() * 100}%` }} />
        <Show when={duration() > 0}>
          <div class="thead" style={{ left: `${progress() * 100}%` }} />
        </Show>
      </div>

      <span class="ttime mono">{props.state?.duration ?? "—"}</span>

      <Show when={props.state?.error}>
        <span class="terror" title={props.state!.error}>
          {props.state!.error}
        </span>
      </Show>
    </div>
  );
}
