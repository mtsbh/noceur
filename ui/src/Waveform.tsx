import { createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { themeTick } from "./themes";

/** Bytes per bucket in the envelope. Matches `ENVELOPE_BANDS`. */
const BANDS = 4;
const PEAK = 0;
const LOW = 1;
const MID = 2;
const HIGH = 3;

/** How the waveform is drawn. Persisted; chosen in Settings. */
export type WaveShape = "filled" | "bars";
export type WaveColour = "rgb" | "threeband" | "blue" | "accent";
export type WaveStyle = { shape: WaveShape; colour: WaveColour };

/**
 * The palettes, each one a look a DJ will already know from somewhere.
 *
 * These are not decoration. A DJ reads a waveform the way a driver reads a
 * dashboard — by colour, at a glance, without stopping to think — and the
 * association is learned from whatever program they came from. Someone off
 * Rekordbox reads blue-and-orange instantly and finds RGB unfamiliar; someone
 * off Traktor is the other way round. Offering the conventions is cheaper than
 * asking anyone to relearn one.
 */
const RGB_HUES = {
  low: [216, 84, 58],
  mid: [122, 182, 72],
  high: [74, 144, 217],
} as const;

/** Rekordbox's three, drawn as separate shapes rather than blended. */
const THREE_BAND = {
  low: [42, 111, 212],
  mid: [224, 138, 46],
  high: [242, 239, 230],
} as const;

/** Serato's single blue, from deep to pale as the highs come up. */
const BLUE_DEEP = [26, 96, 148] as const;
const BLUE_PALE = [150, 226, 245] as const;

/** How much of full brightness the unplayed part keeps. */
const AHEAD_DIM = 0.42;
/** How much of the body's brightness the peak halo behind it keeps. */
const HALO_DIM = 0.32;

/** Bar width and gap for the bars shape, in CSS pixels. */
const BAR = 2;
const GAP = 1;

/**
 * The amplitude envelope of the loaded track, drawn as the seek surface.
 *
 * A canvas rather than an image from the backend. The envelope is eight kilobytes
 * and there is one on screen, so the reason cover art is served over `thumb://`
 * — many large images at once — does not apply here. Drawing in the client also
 * means the played portion recolours as the playhead moves and the bar re-renders
 * at a new width on resize, neither of which costs a round trip.
 *
 * Height is how loud the music is; colour is what kind of loud. A breakdown drops
 * its kick and turns blue, a drop takes it back and turns red, and a full chorus
 * with everything in it goes pale — which is the whole reason the envelope is
 * split by frequency rather than stored as a single amplitude.
 */
export default function Waveform(props: {
  /** Interleaved `[peak, low, mid, high]` per bucket, 0..255. Empty while loading. */
  envelope: Uint8Array;
  /** Fraction played, 0..1. */
  progress: number;
  style: WaveStyle;
}) {
  let canvas: HTMLCanvasElement | undefined;

  /** Canvas size in CSS pixels, so the aggregation can depend on it. */
  const [size, setSize] = createSignal({ w: 0, h: 0 });

  /** How many slots across: one per pixel when filled, one per bar otherwise. */
  const slots = () => {
    const w = size().w;
    return props.style.shape === "bars"
      ? Math.max(1, Math.floor(w / (BAR + GAP)))
      : Math.max(1, w);
  };

  /**
   * One entry per slot: the height of each band, the peak, and a blended colour.
   *
   * Recomputed only when the envelope, the size or the shape changes — never on
   * the position poll, which arrives ten times a second while playing and only
   * ever moves the playhead. Colour mode is applied at draw time from these, so
   * switching palette does not re-aggregate anything.
   */
  const columns = createMemo(() => {
    const e = props.envelope;
    const n = slots();
    if (e.length === 0 || n < 1) return null;

    const buckets = e.length / BANDS;
    const per = buckets / n;

    const height = new Float32Array(n);
    const lows = new Float32Array(n);
    const mids = new Float32Array(n);
    const highs = new Float32Array(n);
    const halo = new Float32Array(n);
    const rgb = new Uint8Array(n * 3);
    const blue = new Uint8Array(n * 3);

    const rawLoud = new Float32Array(n);
    const rawLow = new Float32Array(n);
    const rawMid = new Float32Array(n);
    const rawHigh = new Float32Array(n);

    // Track loudness and track peak, so the drawing is normalised to the music
    // rather than to full scale. Masters do not agree on how loud full scale is;
    // a well-mastered club track whose peaks sit at a third of it would
    // otherwise draw as a low flat strip beside a limited one that fills the
    // bar — a difference in mastering shown as a difference in the music.
    let loudest = 0;
    let loudestPeak = 0;

    for (let i = 0; i < n; i++) {
      const from = Math.floor(i * per);
      const to = Math.max(from + 1, Math.floor((i + 1) * per));

      // The three bands aggregate as means and the peak as a maximum, and the
      // difference matters. A transient that survives one bucket in eight is
      // exactly what a peak band is for. Band energies are mean-square
      // quantities: taking the loudest of eight would pull every slot up to its
      // busiest moment and flatten the differences between loud and quiet
      // passages that the shape exists to show.
      let low = 0;
      let mid = 0;
      let high = 0;
      let top = 0;
      let count = 0;
      for (let j = from; j < to && j < buckets; j++) {
        const at = j * BANDS;
        low += e[at + LOW];
        mid += e[at + MID];
        high += e[at + HIGH];
        if (e[at + PEAK] > top) top = e[at + PEAK];
        count++;
      }
      if (count > 0) {
        low /= count;
        mid /= count;
        high /= count;
      }

      // The bands partition the spectrum, so their energies add: this is the
      // number a plain full-range RMS would have given.
      const loud = Math.sqrt(low * low + mid * mid + high * high);
      rawLoud[i] = loud;
      rawLow[i] = low;
      rawMid[i] = mid;
      rawHigh[i] = high;
      halo[i] = top;
      if (loud > loudest) loudest = loud;
      if (top > loudestPeak) loudestPeak = top;

      // Colour is the balance of the three, independent of how loud the moment
      // is: a quiet passage of pure bass is the same red as a loud one, only
      // shorter. Scaling by the strongest band rather than by their sum keeps a
      // single-band moment fully saturated instead of washing it towards grey.
      const strongest = Math.max(low, mid, high) || 1;
      const r = low / strongest;
      const g = mid / strongest;
      const b = high / strongest;
      const total = r + g + b || 1;
      for (let c = 0; c < 3; c++) {
        rgb[i * 3 + c] = Math.min(
          255,
          Math.round((r * RGB_HUES.low[c] + g * RGB_HUES.mid[c] + b * RGB_HUES.high[c]) / total),
        );
      }

      // Serato's one blue is not flat: it pales where the highs come up, which
      // is what makes hats and rides visible in a single-colour waveform.
      const air = Math.min(1, high / (low + mid + high || 1) / 0.45);
      for (let c = 0; c < 3; c++) {
        blue[i * 3 + c] = Math.round(BLUE_DEEP[c] + (BLUE_PALE[c] - BLUE_DEEP[c]) * air);
      }
    }

    if (loudest === 0 && loudestPeak === 0) return null;
    // Every band is scaled by the same loudness, so their relative heights stay
    // true: the three-band drawing is only readable if a bass-heavy moment
    // really does show more blue than orange.
    for (let i = 0; i < n; i++) {
      height[i] = curve(rawLoud[i] / (loudest || 1));
      lows[i] = curve(rawLow[i] / (loudest || 1));
      mids[i] = curve(rawMid[i] / (loudest || 1));
      highs[i] = curve(rawHigh[i] / (loudest || 1));
      halo[i] = curve(halo[i] / (loudestPeak || 1));
    }
    return { height, lows, mids, highs, halo, rgb, blue };
  });

  /**
   * The perceptual curve applied to every height.
   *
   * A square root, not decoration. Loudness is roughly logarithmic, so drawing
   * amplitude linearly spends most of a short bar on the top few decibels and
   * squashes every intro, breakdown and outro into the bottom sliver — which is
   * where the detail a DJ is looking for lives.
   */
  function curve(level: number): number {
    return Math.sqrt(Math.min(1, Math.max(0, level)));
  }

  function draw() {
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Lay out in CSS pixels and scale the backing store, so the shape stays
    // crisp on a scaled display rather than being drawn at a fraction of device
    // resolution.
    const dpr = window.devicePixelRatio || 1;
    const box = canvas.getBoundingClientRect();
    const w = Math.max(1, Math.floor(box.width));
    const h = Math.max(1, Math.floor(box.height));
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    // Read so a change of theme redraws in the new colours.
    themeTick();
    const style = getComputedStyle(canvas);
    const token = (name: string, fallback: string) =>
      style.getPropertyValue(name).trim() || fallback;
    const faint = token("--fg4", "rgba(238,234,228,.14)");
    const accent = token("--acc", "#e8a55f");

    const mid = h / 2;
    const edge = props.progress * w;
    const cols = columns();

    // Nothing to draw yet, or a track of pure silence: a flat rule keeps the
    // seek surface visible and the layout still, so nothing jumps when the
    // envelope arrives.
    if (!cols) {
      const y = Math.round(mid) - 1;
      ctx.fillStyle = faint;
      ctx.fillRect(0, y, w, 2);
      ctx.fillStyle = accent;
      ctx.fillRect(0, y, edge, 2);
      return;
    }

    // A pixel of headroom, so the loudest moment does not sit flush against the
    // edge of the canvas.
    const reach = h / 2 - 1;
    const bars = props.style.shape === "bars";
    const step = bars ? BAR + GAP : 1;
    const thick = bars ? BAR : 1;
    const mode = props.style.colour;
    const accentAhead = token("--fg2", "rgba(238,234,228,.54)");

    /** Draws one slot's rectangle, centred, at a fraction of the half-height. */
    const slot = (x: number, level: number) => {
      const tall = Math.max(0.5, level * reach);
      ctx.fillRect(x, mid - tall, thick, tall * 2);
    };

    for (let i = 0; i < cols.height.length; i++) {
      const x = i * step;
      if (x >= w) break;
      // Ahead of the playhead the same colours are dimmed rather than replaced
      // with grey: colour is the information here, and a waveform that only
      // showed it behind the playhead would be blank exactly when it is useful
      // — before you have played the part you are looking at.
      const lit = x + thick <= edge ? 1 : AHEAD_DIM;

      if (mode === "threeband") {
        // Three shapes stacked back to front rather than one blended colour.
        // Bass is usually the tallest, so blue forms the outline, the mids sit
        // inside it in orange, and the highs show as a pale core — which is why
        // a Rekordbox waveform looks banded rather than mixed.
        const order = [
          [cols.lows[i], THREE_BAND.low],
          [cols.mids[i], THREE_BAND.mid],
          [cols.highs[i], THREE_BAND.high],
        ] as const;
        for (const [level, hue] of order) {
          ctx.fillStyle = `rgb(${hue[0]} ${hue[1]} ${hue[2]} / ${lit})`;
          slot(x, level);
        }
        continue;
      }

      let r: number;
      let g: number;
      let b: number;
      if (mode === "rgb") {
        r = cols.rgb[i * 3];
        g = cols.rgb[i * 3 + 1];
        b = cols.rgb[i * 3 + 2];
      } else if (mode === "blue") {
        r = cols.blue[i * 3];
        g = cols.blue[i * 3 + 1];
        b = cols.blue[i * 3 + 2];
      } else {
        // The app's own accent, and the one mode that keeps the rest of the
        // interface's convention: played is the accent, unplayed is grey.
        ctx.fillStyle = x + thick <= edge ? accent : accentAhead;
        ctx.globalAlpha = lit;
        if (h >= 56) {
          ctx.globalAlpha = lit * HALO_DIM;
          slot(x, cols.halo[i]);
          ctx.globalAlpha = lit;
        }
        slot(x, cols.height[i]);
        ctx.globalAlpha = 1;
        continue;
      }

      if (h >= 56) {
        ctx.fillStyle = `rgb(${r} ${g} ${b} / ${lit * HALO_DIM})`;
        slot(x, cols.halo[i]);
      }
      ctx.fillStyle = `rgb(${r} ${g} ${b} / ${lit})`;
      slot(x, cols.height[i]);
    }

    // The playhead itself, so the exact position is not left to whichever column
    // won a rounding comparison.
    if (props.progress > 0) {
      ctx.fillStyle = token("--fg", "#eeeae4");
      ctx.fillRect(Math.min(w - 1, Math.round(edge)), 0, 1, h);
    }
  }

  createEffect(draw);

  // Track the canvas size: the bar is the flexible element in the transport, so
  // it changes whenever the window is resized or a longer title appears beside
  // it. Recording it in a signal re-runs the aggregation as well as the draw.
  createEffect(() => {
    const el = canvas;
    if (!el) return;
    const measure = () => {
      const box = el.getBoundingClientRect();
      const next = { w: Math.max(1, Math.floor(box.width)), h: Math.max(1, Math.floor(box.height)) };
      setSize((prev) => (prev.w === next.w && prev.h === next.h ? prev : next));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    onCleanup(() => observer.disconnect());
  });

  return <canvas class="twave" ref={(el) => (canvas = el)} />;
}
