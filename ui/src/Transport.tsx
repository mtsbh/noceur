import { createSignal, createEffect, onCleanup, Show } from "solid-js";
import { transportCmd, transportState, thumbUrl, type Transport as T } from "./api";
import { keyColour } from "./columns";

/**
 * How often the position is re-read while playing.
 *
 * The audio thread already knows the position exactly; this only decides how
 * smoothly the bar moves. Ten times a second is smooth enough to read and cheap
 * enough to ignore.
 */
const TICK_MS = 100;

/** Seconds a nudge moves the playhead — enough to skip an intro, not a section. */
const NUDGE_MS = 10_000;

/**
 * The transport bar.
 *
 * Full width at the bottom of the window, per the spec — it survives split mode,
 * so it belongs to the window rather than to a pane.
 */
export default function Transport(props: {
  state: T | null;
  onState: (t: T) => void;
  onPrev: () => void;
  onNext: () => void;
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

  const loaded = () => Boolean(props.state?.path);
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

  const act = (action: Parameters<typeof transportCmd>[0], value?: number) => () =>
    transportCmd(action, value).then(props.onState).catch(() => {});

  /** Moves the playhead by a fixed step, clamped to the track. */
  function nudge(by: number) {
    const at = props.state?.positionMs ?? 0;
    const target = Math.min(Math.max(0, at + by), Math.max(0, duration()));
    transportCmd("seek", target).then(props.onState).catch(() => {});
  }

  /** Artist and title if the tags have them, else the file name. */
  const label = () => {
    const s = props.state;
    if (!s) return "";
    const artist = s.artist.trim();
    const title = s.title.trim();
    if (artist && title) return `${artist} — ${title}`;
    return title || s.name;
  };

  return (
    <div class={`transport ${loaded() ? "" : "idle"}`}>
      {/* Cover of what is playing, at the size the row list already caches. */}
      <span class="tart">
        <Show when={loaded() && props.state!.fileId > 0}>
          <img
            src={thumbUrl(props.state!.fileId, "medium")}
            alt=""
            decoding="async"
            draggable={false}
            onError={(e) => e.currentTarget.classList.add("blank")}
            onLoad={(e) => e.currentTarget.classList.remove("blank")}
          />
        </Show>
      </span>

      <div class="tmeta">
        <span class="tname" title={props.state?.path || ""}>
          {label() || "Nothing loaded"}
        </span>
        <span class="ttags">
          <Show when={props.state?.bpm}>
            <span class="mono">{props.state!.bpm}</span>
          </Show>
          <Show when={props.state?.key}>
            <span
              class="mono"
              style={{ color: keyColour(props.state!.keyNorm ?? null) }}
            >
              {props.state!.key}
            </span>
          </Show>
        </span>
      </div>

      <div class="tbtns">
        <button class="tbtn" onClick={props.onPrev} disabled={!loaded()} title="Previous track">
          ⏮
        </button>
        <button
          class="tbtn"
          onClick={() => nudge(-NUDGE_MS)}
          disabled={!loaded()}
          title="Back 10 seconds"
        >
          ⏪
        </button>
        <button
          class="tbtn play"
          onClick={act("toggle")}
          disabled={!loaded()}
          title={props.state?.playing ? "Pause (space)" : "Play (space)"}
        >
          {props.state?.playing ? "❚❚" : "▶"}
        </button>
        <button
          class="tbtn"
          onClick={() => nudge(NUDGE_MS)}
          disabled={!loaded()}
          title="Forward 10 seconds"
        >
          ⏩
        </button>
        <button class="tbtn" onClick={props.onNext} disabled={!loaded()} title="Next track">
          ⏭
        </button>
        <button class="tbtn" onClick={act("stop")} disabled={!loaded()} title="Stop">
          ■
        </button>
      </div>

      <span class="ttime mono">{props.state?.position ?? "0:00"}</span>

      {/* The seek surface. A waveform draws here next; the geometry and the drag
          behaviour are the same either way, which is why the bar owns them. */}
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

      {/* Volume. A slider rather than buttons: setting a level is one gesture,
          and stepping to it is several. */}
      <div class="tvol" title={`Volume ${props.state?.volume ?? 100}%`}>
        <span class="tvolicon">🔈</span>
        <input
          type="range"
          min="0"
          max="100"
          value={props.state?.volume ?? 100}
          onInput={(e) => {
            const level = Number(e.currentTarget.value);
            transportCmd("volume", level).then(props.onState).catch(() => {});
          }}
        />
      </div>

      <Show when={props.state?.error}>
        <span class="terror" title={props.state!.error}>
          {props.state!.error}
        </span>
      </Show>
    </div>
  );
}
