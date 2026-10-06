import { createSignal, Show } from "solid-js";

/** `m:ss`, or `h:mm:ss` past the hour — the same shape the transport prints. */
function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/**
 * The plain seek bar, for anyone who would rather not read a waveform.
 *
 * Drawn only: the click and drag belong to `.tbar` around it, as they do for the
 * waveform, so seeking behaves the same whichever is chosen. What this adds is
 * the hover — a ghost fill and the time under the pointer — which a waveform
 * gets for free from its shape and a bare line does not.
 */
export default function SeekSlider(props: {
  /** Fraction played, 0..1. */
  progress: number;
  durationMs: number;
  /** Whether a drag is in progress, so the thumb stays up while the pointer leaves. */
  scrubbing: boolean;
}) {
  const [hover, setHover] = createSignal<number | null>(null);
  let root: HTMLDivElement | undefined;

  const track = (e: PointerEvent) => {
    if (!root || props.durationMs <= 0) return;
    const box = root.getBoundingClientRect();
    setHover(Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)));
  };

  const pct = (f: number) => `${(f * 100).toFixed(3)}%`;
  const live = () => props.durationMs > 0;
  // While dragging, the tip follows the playhead rather than a stale hover.
  const tipAt = () => (props.scrubbing ? props.progress : hover());

  return (
    <div
      class={`tslider ${live() ? "" : "off"} ${props.scrubbing ? "drag" : ""}`}
      ref={(el) => (root = el)}
      onPointerMove={track}
      onPointerLeave={() => setHover(null)}
    >
      <div class="tsrail">
        <Show when={hover() !== null && !props.scrubbing}>
          <div class="tsghost" style={{ width: pct(hover()!) }} />
        </Show>
        <div class="tsfill" style={{ width: pct(props.progress) }} />
      </div>
      <div class="tsthumb" style={{ left: pct(props.progress) }} />
      <Show when={live() && tipAt() !== null}>
        <span class="tstip mono" style={{ left: pct(tipAt()!) }}>
          {clock(tipAt()! * props.durationMs)}
        </span>
      </Show>
    </div>
  );
}
