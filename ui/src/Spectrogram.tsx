import { createEffect, createSignal, onCleanup } from "solid-js";

/** Bands up the height. Must match `BANDS` in `noceur-audio`. */
const BANDS = 128;

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
}) {
  let canvas: HTMLCanvasElement | undefined;
  const [size, setSize] = createSignal({ w: 0, h: 0 });

  const columns = () =>
    props.grid.length > 0 ? Math.floor(props.grid.length / BANDS) : 0;

  // The picture only changes when the track does, so it is drawn to its own
  // canvas once and blitted. The playhead moves ten times a second and must not
  // drag a full repaint of the grid along with it.
  let image: HTMLCanvasElement | undefined;

  createEffect(() => {
    const grid = props.grid;
    const cols = columns();
    if (cols === 0) {
      image = undefined;
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

    ctx.clearRect(0, 0, box.w, box.h);
    if (!image) return;

    // Smoothing off: this is data, and interpolating it invents levels between
    // the ones that were measured.
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(image, 0, 0, box.w, box.h);

    const at = Math.round(box.w * Math.min(1, Math.max(0, props.progress)));
    ctx.fillStyle = "rgba(255, 255, 255, 0.85)";
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

  /** Frequency at the top of the picture, for the one label worth having. */
  const nyquist = () => (props.sampleRate > 0 ? props.sampleRate / 2000 : 0);

  return (
    <div class="spek">
      <canvas ref={(el) => (canvas = el)} class="spekcanvas" />
      <div class="spekaxis">
        <span>{nyquist() > 0 ? `${nyquist().toFixed(1)}k` : ""}</span>
        <span>{nyquist() > 0 ? `${(nyquist() / 2).toFixed(0)}k` : ""}</span>
        <span>0</span>
      </div>
    </div>
  );
}
