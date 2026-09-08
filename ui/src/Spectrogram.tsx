import { For, createEffect, createSignal, onCleanup } from "solid-js";

/** Bands up the height. Must match `BANDS` in `noceur-audio`. */
const BANDS = 256;

/**
 * Quietest level drawn, in dB below the track's own peak.
 *
 * Must match `FLOOR_DB` in `noceur-audio`: the bytes arriving are scaled across
 * exactly this range, and a legend built against a different one would label
 * every colour wrongly.
 */
const FLOOR_DB = -96;

/**
 * The colour ramp, from silence to full.
 *
 * Black through violet and red into a pale yellow — the map Spek and every
 * spectrum analyser since has used, and the reason is not decoration: the eye
 * separates hues far better than it separates shades of one colour, so a ramp
 * that moves through hue shows twenty distinguishable levels where a greyscale
 * one shows five. A cutoff that is thirty decibels down reads as a hard edge
 * here and as a slightly darker grey there.
 */
const RAMP: [number, number, number][] = [
  [0, 0, 0],
  [22, 8, 58],
  [86, 15, 110],
  [151, 34, 106],
  [204, 63, 76],
  [239, 116, 48],
  [250, 180, 55],
  [252, 240, 168],
];

/** The ramp expanded to a lookup of 256 colours, built once. */
const LUT = (() => {
  const out = new Uint8ClampedArray(256 * 3);
  for (let i = 0; i < 256; i++) {
    const at = (i / 255) * (RAMP.length - 1);
    const low = Math.floor(at);
    const high = Math.min(low + 1, RAMP.length - 1);
    const t = at - low;
    for (let c = 0; c < 3; c++) {
      out[i * 3 + c] = RAMP[low][c] + (RAMP[high][c] - RAMP[low][c]) * t;
    }
  }
  return out;
})();

/** The same ramp as a CSS gradient, for the legend beside the picture. */
const LEGEND = `linear-gradient(to top, ${RAMP.map(
  (c, i) => `rgb(${c[0]},${c[1]},${c[2]}) ${(i / (RAMP.length - 1)) * 100}%`,
).join(", ")})`;

/** `m:ss`, for the time axis. */
function clock(seconds: number): string {
  const whole = Math.max(0, Math.round(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

/**
 * Seconds between time labels, so a track gets six or so however long it is.
 *
 * Chosen from a list of round numbers rather than computed: 30 seconds and a
 * minute are intervals anyone reads instantly, and 47 seconds is not.
 */
function timeStep(total: number): number {
  for (const step of [10, 15, 30, 60, 120, 300, 600, 900]) {
    if (total / step <= 8) return step;
  }
  return 1800;
}

/**
 * The spectrogram of the loaded track.
 *
 * Time across, frequency up, loudness as colour. It says at a glance what the
 * quality report says in words: a lossy file has a ruler-straight line across
 * the top with nothing above it, and a resampled one has that line below its
 * own Nyquist.
 *
 * Drawn from the grid rather than from an image, so the colour map is a
 * rendering choice here rather than something baked in Rust and re-decoded to
 * change — the same arrangement the waveform uses.
 */
export default function Spectrogram(props: {
  /** `columns * BANDS` bytes, column by column. Empty while it is being measured. */
  grid: Uint8Array;
  /** Fraction of the track played, 0..1, for the playhead. */
  progress: number;
  sampleRate: number;
  durationMs: number;
  /** Shown above the picture, as Spek does. */
  name: string;
}) {
  let canvas: HTMLCanvasElement | undefined;
  const [size, setSize] = createSignal({ w: 0, h: 0 });

  const columns = () =>
    props.grid.length > 0 ? Math.floor(props.grid.length / BANDS) : 0;

  /** Top of the picture, in kHz. */
  const nyquist = () => (props.sampleRate > 0 ? props.sampleRate / 2000 : 0);

  /** Frequency labels, from the top down, at a round interval. */
  const frequencies = () => {
    const top = nyquist();
    if (top <= 0) return [];
    const step = top > 30 ? 10 : top > 14 ? 5 : 2;

    const marks: number[] = [];
    for (let khz = 0; khz <= top; khz += step) marks.push(khz);
    // The very top is worth labelling even when it is not on the interval: it
    // is where the answer usually is.
    if (top - marks[marks.length - 1] > step * 0.35) marks.push(top);
    return marks;
  };

  const seconds = () => props.durationMs / 1000;

  const times = () => {
    const total = seconds();
    if (total <= 0) return [];
    const step = timeStep(total);

    const marks: number[] = [];
    for (let at = 0; at < total; at += step) marks.push(at);
    marks.push(total);
    return marks;
  };

  /** The dB scale beside the ramp, in twenties. */
  const decibels = () => {
    const marks: number[] = [];
    for (let db = 0; db >= FLOOR_DB; db -= 20) marks.push(db);
    return marks;
  };

  // The picture only changes when the track does, so it is drawn to its own
  // canvas once and blitted. The playhead moves ten times a second and must not
  // drag a full repaint of the grid along with it.
  let image: HTMLCanvasElement | undefined;

  createEffect(() => {
    const grid = props.grid;
    const cols = columns();
    if (cols === 0) {
      image = undefined;
      draw();
      return;
    }

    const off = document.createElement("canvas");
    off.width = cols;
    off.height = BANDS;
    const ctx = off.getContext("2d");
    if (!ctx) return;

    const pixels = ctx.createImageData(cols, BANDS);
    for (let x = 0; x < cols; x++) {
      for (let y = 0; y < BANDS; y++) {
        // Row 0 of the image is the top of the picture and the *highest*
        // frequency; band 0 of the grid is the lowest. Drawing them in the same
        // order puts the bass in the sky.
        const value = grid[x * BANDS + (BANDS - 1 - y)];
        const at = (y * cols + x) * 4;
        pixels.data[at] = LUT[value * 3];
        pixels.data[at + 1] = LUT[value * 3 + 1];
        pixels.data[at + 2] = LUT[value * 3 + 2];
        pixels.data[at + 3] = 255;
      }
    }
    ctx.putImageData(pixels, 0, 0);
    image = off;
    draw();
  });

  function draw() {
    const box = size();
    if (!canvas || box.w === 0) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(box.w * dpr);
    canvas.height = Math.round(box.h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, box.w, box.h);
    if (!image) return;

    // Smoothing off horizontally would show every column edge, and on
    // vertically would blur the very line being looked for. Columns are far
    // narrower than the box, rows far taller, so this is the useful way round.
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(image, 0, 0, box.w, box.h);

    const at = Math.round(box.w * Math.min(1, Math.max(0, props.progress)));
    ctx.fillStyle = "rgba(255, 255, 255, 0.9)";
    ctx.fillRect(at, 0, 1, box.h);
  }

  createEffect(draw);

  createEffect(() => {
    if (!canvas) return;
    const observer = new ResizeObserver(([entry]) => {
      const box = entry.contentRect;
      setSize({ w: box.width, h: box.height });
    });
    observer.observe(canvas);
    onCleanup(() => observer.disconnect());
  });

  return (
    <div class="spek">
      <div class="spekhead" title={props.name}>
        {props.name}
      </div>

      <div class="spekgrid">
        <div class="spekfreq">
          <For each={frequencies()}>
            {(khz) => (
              <span style={{ bottom: `${(khz / nyquist()) * 100}%` }}>
                {khz % 1 === 0 ? khz : khz.toFixed(1)} kHz
              </span>
            )}
          </For>
        </div>

        <div class="spekplot">
          <canvas ref={(el) => (canvas = el)} />
        </div>

        <div class="speklegend">
          <div class="spekramp" style={{ background: LEGEND }} />
          <div class="spekdb">
            <For each={decibels()}>
              {(db) => (
                <span style={{ top: `${(db / FLOOR_DB) * 100}%` }}>{db} dB</span>
              )}
            </For>
          </div>
        </div>

        <div class="spektime">
          <For each={times()}>
            {(at) => (
              <span style={{ left: `${(at / seconds()) * 100}%` }}>
                {clock(at)}
              </span>
            )}
          </For>
        </div>
      </div>
    </div>
  );
}
