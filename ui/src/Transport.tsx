import { createSignal, createEffect, onCleanup, Show } from "solid-js";
import { transportCmd, transportState, thumbUrl, envelope, type Transport as T } from "./api";
import Waveform from "./Waveform";
import Meters, { nextMeter, type MeterMode } from "./Meters";
import { keyColour } from "./columns";
import {
  BackIcon,
  ForwardIcon,
  NextIcon,
  PauseIcon,
  PlayIcon,
  PrevIcon,
  StopIcon,
  MutedIcon,
  VolumeIcon,
} from "./icons";
import { type Settings } from "./settings";

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
  /** Double-clicking the cover opens the tag editor on what is playing. */
  onArt: (fileId: number) => void;
  /** Double-clicking the name goes to the file, in whichever pane has focus. */
  onLocate: () => void;
  settings: Settings;
  /** A click on the meter box steps to the next kind of meter. */
  onMeter: (mode: MeterMode) => void;
}) {
  const [scrubbing, setScrubbing] = createSignal<number | null>(null);
  const [wave, setWave] = createSignal<Uint8Array>(new Uint8Array());
  let bar: HTMLDivElement | undefined;

  // Poll while something is actually playing, and while a track is loaded but
  // has not reported a duration yet — that is a file still being opened, and it
  // is the one moment a stopped transport does have news coming. Otherwise a
  // paused or empty transport has nothing to move, and polling it would be a
  // timer that never sleeps.
  createEffect(() => {
    const opening = Boolean(props.state?.path) && !props.state?.durationMs;
    if (!props.state?.playing && !opening) return;
    const timer = window.setInterval(() => {
      transportState().then(props.onState).catch(() => {});
    }, TICK_MS);
    onCleanup(() => window.clearInterval(timer));
  });

  // Fetch the envelope when the loaded track changes, keyed on the file rather
  // than on the transport object: the state is replaced ten times a second while
  // playing, and re-decoding on each of those would be unthinkable.
  createEffect<number>((previous) => {
    const id = props.state?.fileId ?? 0;
    if (id === previous) return id;
    setWave(new Uint8Array());
    if (id > 0) {
      // Extraction decodes the whole track, so a stale reply is possible if the
      // user loads another one first; the id check drops it.
      envelope(id)
        .then((data) => {
          if ((props.state?.fileId ?? 0) === id) setWave(data);
        })
        .catch(() => {});
    }
    return id;
  }, 0);

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

  /** The title if the tags have one, else the file name. */
  const title = () => {
    const s = props.state;
    if (!s) return "";
    return s.title.trim() || s.name;
  };

  /** A tag value worth showing: the backend spells a missing one as a dash. */
  const present = (v?: string | null) => {
    const t = (v ?? "").trim();
    return t && t !== "—" ? t : "";
  };

  /** Everything the name line knows, for the tooltip when it is cut short. */
  const full = () => {
    const s = props.state;
    if (!s?.path) return "";
    const artist = s.artist.trim();
    return `${artist ? `${artist} - ` : ""}${title()}\n${s.path}`;
  };

  /**
   * The level to go back to when unmuting. Muting is a volume of zero rather
   * than a separate switch, so the slider always shows what is actually heard;
   * this remembers where it was.
   */
  const [unmuteTo, setUnmuteTo] = createSignal(100);
  const volume = () => props.state?.volume ?? 100;
  const muted = () => volume() === 0;
  function toggleMute() {
    if (muted()) {
      transportCmd("volume", unmuteTo() || 100).then(props.onState).catch(() => {});
    } else {
      setUnmuteTo(volume());
      transportCmd("volume", 0).then(props.onState).catch(() => {});
    }
  }

  return (
    <div class={`transport ${loaded() ? "" : "idle"}`}>
      {/* Cover of what is playing, at the size the row list already caches.
          Double-click rather than single: this sits under the pointer on the
          way to the play button, and a single click would open the editor by
          accident several times an evening. */}
      <span
        class="tart"
        data-tip={loaded() ? "Metadata" : undefined}
        data-tip-at="above start"
        onDblClick={() => {
          const id = props.state?.fileId ?? 0;
          if (id > 0) props.onArt(id);
        }}
      >
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

      {/* Title on top, artist beneath with the BPM and key beside it - two
          short lines rather than one long one cut off halfway through the
          second artist. The whole of it is in the tooltip. */}
      <div class="tmeta" title={full()}>
        <span
          class={`tname ${loaded() ? "go" : ""}`}
          data-tip={loaded() ? "Go to file" : undefined}
          data-tip-at="above start"
          onDblClick={() => loaded() && props.onLocate()}
        >
          {title() || "Nothing loaded"}
        </span>
        <span class="ttags">
          <Show when={present(props.state?.artist)}>
            <span class="tartist">{present(props.state?.artist)}</span>
          </Show>
          <Show when={present(props.state?.bpm)}>
            <span class="mono">{present(props.state?.bpm)}</span>
          </Show>
          <Show when={present(props.state?.key)}>
            <span
              class="mono"
              style={{ color: keyColour(props.state!.keyNorm ?? null) }}
            >
              {present(props.state?.key)}
            </span>
          </Show>
        </span>
      </div>

      {/* Play is the one button hit constantly and the one that must be found
          without looking, so it is the only one given a filled shape. */}
      <div class="tbtns">
        <button
          class="tbtn"
          onClick={props.onPrev}
          disabled={!loaded()}
          data-tip="Previous"
          data-tip-at="above"
          aria-label="Previous track"
        >
          <PrevIcon />
        </button>
        <button
          class="tbtn"
          onClick={() => nudge(-NUDGE_MS)}
          disabled={!loaded()}
          data-tip="−10s"
          data-tip-at="above"
          aria-label="Back 10 seconds"
        >
          <BackIcon />
        </button>
        <button
          class="tbtn play"
          onClick={act("toggle")}
          disabled={!loaded()}
          data-tip={props.state?.playing ? "Pause" : "Play"}
          data-tip-at="above"
          aria-label={props.state?.playing ? "Pause" : "Play"}
        >
          {props.state?.playing ? <PauseIcon /> : <PlayIcon />}
        </button>
        <button
          class="tbtn"
          onClick={() => nudge(NUDGE_MS)}
          disabled={!loaded()}
          data-tip="+10s"
          data-tip-at="above"
          aria-label="Forward 10 seconds"
        >
          <ForwardIcon />
        </button>
        <button
          class="tbtn"
          onClick={props.onNext}
          disabled={!loaded()}
          data-tip="Next"
          data-tip-at="above"
          aria-label="Next track"
        >
          <NextIcon />
        </button>
        <button
          class="tbtn"
          onClick={act("stop")}
          disabled={!loaded()}
          data-tip="Stop"
          data-tip-at="above"
          aria-label="Stop"
        >
          <StopIcon />
        </button>
      </div>

      <span class="ttime mono">{props.state?.position ?? "0:00"}</span>

      {/* The seek surface, inline with the controls. The waveform draws inside
          it rather than replacing it, so the click and drag geometry stays on
          one element whether or not there is an envelope to show. */}
      <div
        class="tbar"
        ref={(el) => (bar = el)}
        onPointerDown={startScrub}
        title={duration() > 0 ? "Click or drag to seek" : ""}
      >
        <Waveform envelope={wave()} progress={progress()} style={props.settings.wave} />
      </div>

      <span class="ttime mono">{props.state?.duration ?? "—"}</span>

      <Show when={props.settings.meter !== "off"}>
        <Meters
          mode={props.settings.meter}
          playing={Boolean(props.state?.playing)}
          onCycle={() => props.onMeter(nextMeter(props.settings.meter))}
        />
      </Show>

      <div class="tvol" title={`Volume ${volume()}%`}>
        <button
          class={`tvolicon ${muted() ? "muted" : ""}`}
          onClick={toggleMute}
          data-tip={muted() ? "Unmute" : "Mute"}
          data-tip-at="above"
          aria-label={muted() ? "Unmute" : "Mute"}
          aria-pressed={muted()}
        >
          {muted() ? <MutedIcon /> : <VolumeIcon />}
        </button>
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
        <span class="tvolpct mono">{props.state?.volume ?? 100}</span>
      </div>

      <Show when={props.state?.error}>
        <span class="terror" title={props.state!.error}>
          {props.state!.error}
        </span>
      </Show>
    </div>
  );
}
