import { createEffect, onCleanup } from "solid-js";
import { scopeFrames, type ScopeFrames } from "./api";
import { themeTick } from "./themes";

/**
 * What the little box in the transport shows.
 *
 * All five read the same frames - the newest few thousand that went to the
 * output - and differ only in how they draw them.
 */
export type MeterMode = "off" | "vu" | "needle" | "scope" | "vector" | "spectrum";

/** The order a click on the box steps through. `off` is only in settings. */
export const METER_MODES: {
  value: Exclude<MeterMode, "off">;
  label: string;
  /** What the settings button says. */
  short: string;
}[] = [
  { value: "vu", label: "Peak meter", short: "Peak" },
  { value: "needle", label: "VU needles", short: "VU" },
  { value: "scope", label: "Oscilloscope", short: "Scope" },
  { value: "vector", label: "Vectorscope", short: "Vector" },
  { value: "spectrum", label: "Spectrum", short: "Spectrum" },
];

/** The mode after this one, for the click that cycles them. */
export function nextMeter(mode: MeterMode): MeterMode {
  const at = METER_MODES.findIndex((m) => m.value === mode);
  return METER_MODES[(at + 1) % METER_MODES.length].value;
}

/** Bottom of the peak meter and spectrum, in dBFS. */
const FLOOR_DB = -48;
/** Fall rate of the peak bars, in dB a second - a PPM's return, roughly. */
const FALL_DB = 24;
/** How long a peak marker sits before it falls. */
const HOLD_S = 1.2;
/** Integration time of the needles. 300ms is what makes a VU meter a VU meter. */
const VU_TAU = 0.3;
/**
 * Where 0 VU sits, in dBFS.
 *
 * Hotter than the -18 of broadcast alignment, because a club master averages
 * somewhere around -9 dBFS and at -18 every track in the library would sit
 * pinned against the stop. Here a loud master swings around 0 and a quiet rip
 * visibly lives down the scale, which is the comparison worth making.
 */
const VU_REF_DB = -10;
/** Spectrum bands, log-spaced. */
const BANDS = 40;
const FFT_SIZE = 2048;

const db = (v: number) => (v > 1e-9 ? 20 * Math.log10(v) : -200);
const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/**
 * The meter box in the transport.
 *
 * Polled on every display frame while playing, and kept running after a pause
 * only until everything has fallen back to rest, so an idle transport costs
 * nothing.
 */
export default function Meters(props: {
  mode: MeterMode;
  playing: boolean;
  onCycle: () => void;
}) {
  let canvas: HTMLCanvasElement | undefined;
  let frame = 0;
  let inFlight = false;
  let last = performance.now();
  let latest: ScopeFrames | null = null;

  // Ballistics, per side.
  const peak = [FLOOR_DB, FLOOR_DB];
  const hold = [FLOOR_DB, FLOOR_DB];
  const heldFor = [0, 0];
  const vu = [0, 0];
  let correlation = 1;
  const bands = new Float32Array(BANDS).fill(FLOOR_DB);
  const bandPeaks = new Float32Array(BANDS).fill(FLOOR_DB);
  const hannWindow = hann(FFT_SIZE);

  let tokens = readTokens();
  createEffect(() => {
    themeTick();
    tokens = readTokens();
    // Persistence is drawn in the old colours; start clean in the new ones.
    clearCanvas();
  });

  function readTokens() {
    const style = getComputedStyle(document.documentElement);
    const t = (name: string, fallback: string) =>
      style.getPropertyValue(name).trim() || fallback;
    return {
      fg: t("--fg", "#eeeae4"),
      fg2: t("--fg2", "rgba(238,234,228,.54)"),
      fg3: t("--fg3", "rgba(238,234,228,.28)"),
      fg4: t("--fg4", "rgba(238,234,228,.14)"),
      hair: t("--hair2", "rgba(238,234,228,.1)"),
      acc: t("--acc", "#e8a55f"),
      lo: t("--meter-lo", "#7fc46a"),
      mid: t("--meter-mid", "#e8c35f"),
      hi: t("--meter-hi", "#e2685e"),
    };
  }

  function clearCanvas() {
    const ctx = canvas?.getContext("2d");
    if (ctx && canvas) ctx.clearRect(0, 0, canvas.width, canvas.height);
  }

  /** Frames asked for, per mode: as many as the drawing actually uses. */
  const wanted = () =>
    props.mode === "spectrum" ? FFT_SIZE : props.mode === "vector" ? 1024 : 1536;

  function tick(now: number) {
    frame = 0;
    const dt = Math.min(0.1, (now - last) / 1000);
    last = now;

    if (props.playing && !inFlight) {
      inFlight = true;
      scopeFrames(wanted())
        .then((f) => (latest = f))
        .catch(() => {})
        .finally(() => (inFlight = false));
    }
    // Paused or stopped: the ring holds the last thing heard, frozen. Feed the
    // meters silence instead so they fall back rather than stick.
    const input = props.playing ? latest : null;
    const resting = update(input, dt);
    draw(input);

    if (props.playing || !resting) frame = requestAnimationFrame(tick);
  }

  /** Moves every meter towards the input. Returns whether all are at rest. */
  function update(f: ScopeFrames | null, dt: number): boolean {
    const sides = f ? [f.left, f.right] : [null, null];
    // The peak meter reads only the most recent stretch - about a display
    // frame's worth plus slack - so a transient shows once, not for 35ms.
    const recent = f ? Math.min(f.left.length, Math.ceil(f.rate / 30)) : 0;

    for (let c = 0; c < 2; c++) {
      const data = sides[c];
      let p = 0;
      let sq = 0;
      if (data) {
        for (let i = data.length - recent; i < data.length; i++) {
          const v = Math.abs(data[i]);
          if (v > p) p = v;
          sq += data[i] * data[i];
        }
      }
      const level = db(p);
      peak[c] = Math.max(level, peak[c] - FALL_DB * dt, FLOOR_DB);
      if (level >= hold[c]) {
        hold[c] = level;
        heldFor[c] = 0;
      } else if ((heldFor[c] += dt) > HOLD_S) {
        hold[c] = Math.max(FLOOR_DB, hold[c] - FALL_DB * dt);
      }
      const rms = recent ? Math.sqrt(sq / recent) : 0;
      vu[c] += (rms - vu[c]) * (1 - Math.exp(-dt / VU_TAU));
    }

    if (f) {
      let lr = 0;
      let ll = 0;
      let rr = 0;
      for (let i = 0; i < f.left.length; i++) {
        lr += f.left[i] * f.right[i];
        ll += f.left[i] * f.left[i];
        rr += f.right[i] * f.right[i];
      }
      const c = ll > 1e-9 && rr > 1e-9 ? lr / Math.sqrt(ll * rr) : 1;
      correlation += (c - correlation) * (1 - Math.exp(-dt / 0.15));
    }

    if (props.mode === "spectrum") spectrum(f, dt);

    return (
      peak[0] <= FLOOR_DB &&
      peak[1] <= FLOOR_DB &&
      hold[0] <= FLOOR_DB &&
      hold[1] <= FLOOR_DB &&
      vu[0] < 1e-4 &&
      vu[1] < 1e-4 &&
      bands.every((b) => b <= FLOOR_DB)
    );
  }

  function spectrum(f: ScopeFrames | null, dt: number) {
    const fresh = new Float32Array(BANDS).fill(FLOOR_DB);
    if (f && f.left.length >= FFT_SIZE) {
      const re = new Float32Array(FFT_SIZE);
      const im = new Float32Array(FFT_SIZE);
      const off = f.left.length - FFT_SIZE;
      for (let i = 0; i < FFT_SIZE; i++) {
        re[i] = ((f.left[off + i] + f.right[off + i]) / 2) * hannWindow[i];
      }
      fft(re, im);
      const nyquist = f.rate / 2;
      const lowHz = 30;
      const highHz = Math.min(18_000, nyquist);
      const binHz = f.rate / FFT_SIZE;
      for (let b = 0; b < BANDS; b++) {
        const from = lowHz * Math.pow(highHz / lowHz, b / BANDS);
        const to = lowHz * Math.pow(highHz / lowHz, (b + 1) / BANDS);
        const i0 = Math.max(1, Math.floor(from / binHz));
        const i1 = Math.max(i0, Math.min(FFT_SIZE / 2 - 1, Math.ceil(to / binHz)));
        let power = 0;
        for (let i = i0; i <= i1; i++) power = Math.max(power, re[i] * re[i] + im[i] * im[i]);
        // Hann halves the amplitude of a sine; the 4/N puts a full-scale one
        // at 0 dB. Plus a tilt of 3 dB an octave, because music falls at about
        // that rate and without it the top half of the display sits dark.
        const amp = (Math.sqrt(power) * 4) / FFT_SIZE;
        const tilt = 3 * Math.log2(Math.sqrt(from * to) / 1000);
        fresh[b] = Math.max(FLOOR_DB, db(amp) + tilt);
      }
    }
    for (let b = 0; b < BANDS; b++) {
      bands[b] = Math.max(fresh[b], bands[b] - FALL_DB * 1.5 * dt, FLOOR_DB);
      bandPeaks[b] =
        fresh[b] >= bandPeaks[b] ? fresh[b] : Math.max(FLOOR_DB, bandPeaks[b] - FALL_DB * 0.5 * dt);
    }
  }

  function draw(f: ScopeFrames | null) {
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = window_dpr();
    const box = canvas.getBoundingClientRect();
    const w = Math.max(1, Math.floor(box.width));
    const h = Math.max(1, Math.floor(box.height));
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    switch (props.mode) {
      case "vu":
        ctx.clearRect(0, 0, w, h);
        return drawPeak(ctx, w, h);
      case "needle":
        ctx.clearRect(0, 0, w, h);
        return drawNeedles(ctx, w, h);
      case "scope":
        ctx.clearRect(0, 0, w, h);
        return drawScope(ctx, w, h, f);
      case "vector":
        return drawVector(ctx, w, h, f);
      case "spectrum":
        ctx.clearRect(0, 0, w, h);
        return drawSpectrum(ctx, w, h);
    }
  }

  /** Colour of a lit segment at this level: green, then amber, then red. */
  const zone = (level: number) =>
    level >= -3 ? tokens.hi : level >= -12 ? tokens.mid : tokens.lo;
  const toX = (level: number, w: number) => clamp01((level - FLOOR_DB) / -FLOOR_DB) * w;

  /**
   * AIMP's meter: two rows of LED segments, green into amber into red, with a
   * marker that holds the peak for a moment before it falls.
   */
  function drawPeak(ctx: CanvasRenderingContext2D, w: number, h: number) {
    const label = 9;
    const x0 = label + 3;
    const span = w - x0;
    const seg = 3;
    const gap = 1;
    const count = Math.floor(span / (seg + gap));
    const rowH = Math.floor((h - 6) / 2) - 1;

    ctx.font = "8px ui-monospace, Consolas, monospace";
    ctx.textBaseline = "middle";
    for (let c = 0; c < 2; c++) {
      const y = 1 + c * (rowH + 3);
      ctx.fillStyle = tokens.fg3;
      ctx.fillText(c ? "R" : "L", 1, y + rowH / 2 + 0.5);
      const lit = toX(peak[c], count);
      const held = Math.min(count - 1, Math.floor(toX(hold[c], count)));
      for (let s = 0; s < count; s++) {
        const level = FLOOR_DB + (s / count) * -FLOOR_DB;
        const on = s < lit;
        ctx.globalAlpha = on ? 1 : 0.13;
        ctx.fillStyle = on ? zone(level) : tokens.fg4;
        if (!on && s === held && hold[c] > FLOOR_DB) {
          ctx.globalAlpha = 1;
          ctx.fillStyle = zone(level);
        }
        ctx.fillRect(x0 + s * (seg + gap), y, seg, rowH);
      }
    }
    ctx.globalAlpha = 1;

    // A scale beneath, at the points worth knowing.
    ctx.fillStyle = tokens.fg4;
    const yScale = h - 2;
    for (const mark of [-36, -24, -12, -6, -3, 0]) {
      const x = x0 + toX(mark, count * (seg + gap));
      ctx.fillRect(Math.min(w - 1, Math.round(x)), yScale - 1, 1, 2);
    }
  }

  /** Two analogue VU meters side by side, needles on a 300ms integration. */
  function drawNeedles(ctx: CanvasRenderingContext2D, w: number, h: number) {
    const each = (w - 4) / 2;
    for (let c = 0; c < 2; c++) {
      const x = c * (each + 4);
      const cx = x + each / 2;
      const r = Math.min(each * 0.62, h * 1.25);
      const cy = h + r * 0.45;
      const arc = (v: number) => -Math.PI / 2 + (v - 0.5) * 1.35; // v in 0..1
      const vuScale = (vuDb: number) => clamp01((vuDb + 20) / 23); // -20..+3 VU

      ctx.strokeStyle = tokens.hair;
      ctx.lineWidth = 1;
      ctx.strokeRect(x + 0.5, 0.5, each - 1, h - 1);

      // The scale: pale up to 0 VU, red above it.
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.strokeStyle = tokens.fg3;
      ctx.arc(cx, cy, r - 3, arc(0), arc(vuScale(0)));
      ctx.stroke();
      ctx.beginPath();
      ctx.strokeStyle = tokens.hi;
      ctx.arc(cx, cy, r - 3, arc(vuScale(0)), arc(1));
      ctx.stroke();
      ctx.lineWidth = 1;
      for (const mark of [-20, -10, -7, -5, -3, -1, 0, 1, 2, 3]) {
        const a = arc(vuScale(mark));
        ctx.strokeStyle = mark >= 0 ? tokens.hi : tokens.fg3;
        ctx.beginPath();
        ctx.moveTo(cx + Math.cos(a) * (r - 6), cy + Math.sin(a) * (r - 6));
        ctx.lineTo(cx + Math.cos(a) * (r - 1), cy + Math.sin(a) * (r - 1));
        ctx.stroke();
      }

      ctx.fillStyle = tokens.fg4;
      ctx.font = "7px ui-monospace, Consolas, monospace";
      ctx.textBaseline = "bottom";
      ctx.fillText(c ? "R" : "L", x + 3, h - 2);

      const level = db(vu[c]) - VU_REF_DB;
      const a = arc(vuScale(level));
      ctx.strokeStyle = tokens.acc;
      ctx.lineWidth = 1.25;
      ctx.beginPath();
      ctx.moveTo(cx + Math.cos(a) * (r * 0.25), cy + Math.sin(a) * (r * 0.25));
      ctx.lineTo(cx + Math.cos(a) * (r - 1), cy + Math.sin(a) * (r - 1));
      ctx.stroke();
    }
    ctx.lineWidth = 1;
  }

  /**
   * Left and right as two traces, triggered on a rising zero crossing of their
   * sum so a steady tone stands still rather than scrolling.
   */
  function drawScope(ctx: CanvasRenderingContext2D, w: number, h: number, f: ScopeFrames | null) {
    const mid = h / 2;
    ctx.fillStyle = tokens.fg4;
    ctx.fillRect(0, Math.round(mid), w, 1);
    if (!f || f.left.length < 2) return;

    const shown = Math.min(768, Math.floor(f.left.length / 2));
    let start = f.left.length - shown;
    for (let i = f.left.length - shown - 1; i > f.left.length - 2 * shown && i > 0; i--) {
      const a = f.left[i - 1] + f.right[i - 1];
      const b = f.left[i] + f.right[i];
      if (a < 0 && b >= 0) {
        start = i;
        break;
      }
    }

    const trace = (data: Float32Array, colour: string, alpha: number) => {
      ctx.globalAlpha = alpha;
      ctx.strokeStyle = colour;
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let x = 0; x < w; x++) {
        const i = start + Math.floor((x / w) * shown);
        const y = mid - data[i] * (mid - 1);
        if (x === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.stroke();
    };
    trace(f.right, tokens.fg2, 0.7);
    trace(f.left, tokens.acc, 1);
    ctx.globalAlpha = 1;
  }

  /**
   * A goniometer, the way a mastering engineer reads one: mid up the vertical,
   * side across, so mono is a vertical line, wide material spreads sideways and
   * anything leaning out of phase lies down. Beside it, the correlation - the
   * same answer as one number, +1 mono, 0 wide, below 0 trouble.
   */
  function drawVector(ctx: CanvasRenderingContext2D, w: number, h: number, f: ScopeFrames | null) {
    const size = h;
    // Persistence: fade the last picture rather than wiping it, so the trace
    // reads as a cloud rather than flicker.
    ctx.save();
    ctx.globalCompositeOperation = "destination-out";
    ctx.fillStyle = "rgba(0,0,0,0.28)";
    ctx.fillRect(0, 0, size, h);
    ctx.restore();
    ctx.clearRect(size, 0, w - size, h);

    const cx = size / 2;
    const cy = h / 2;
    const r = size / 2 - 1;

    ctx.fillStyle = tokens.fg4;
    ctx.fillRect(Math.round(cx), 0, 1, h);
    ctx.fillRect(0, Math.round(cy), size, 1);

    if (f) {
      ctx.fillStyle = tokens.acc;
      ctx.globalAlpha = 0.55;
      const step = Math.max(1, Math.floor(f.left.length / 700));
      for (let i = 0; i < f.left.length; i += step) {
        const l = f.left[i];
        const rr = f.right[i];
        const side = (rr - l) * Math.SQRT1_2;
        const midv = (l + rr) * Math.SQRT1_2;
        // Square-root scaling, sign kept: quiet material is otherwise a dot.
        const sx = Math.sign(side) * Math.sqrt(Math.abs(side));
        const sy = Math.sign(midv) * Math.sqrt(Math.abs(midv));
        ctx.fillRect(cx + sx * r, cy - sy * r, 1, 1);
      }
      ctx.globalAlpha = 1;
    }

    // Correlation bar.
    const x0 = size + 8;
    const bw = w - x0 - 2;
    if (bw < 20) return;
    const y = Math.round(h / 2) - 3;
    ctx.fillStyle = tokens.fg4;
    ctx.fillRect(x0, y, bw, 6);
    ctx.fillRect(x0 + Math.round(bw / 2), y - 3, 1, 12);
    const pos = x0 + ((clamp01((correlation + 1) / 2)) * bw);
    ctx.fillStyle = correlation < 0 ? tokens.hi : correlation < 0.3 ? tokens.mid : tokens.lo;
    const from = x0 + bw / 2;
    ctx.fillRect(Math.min(from, pos), y, Math.abs(pos - from), 6);
    ctx.fillStyle = tokens.fg3;
    ctx.font = "8px ui-monospace, Consolas, monospace";
    ctx.textBaseline = "top";
    ctx.fillText("-1", x0, 0);
    ctx.textAlign = "right";
    ctx.fillText("+1", x0 + bw, 0);
    ctx.textAlign = "center";
    ctx.textBaseline = "bottom";
    ctx.fillText(correlation.toFixed(2), x0 + bw / 2, h);
    ctx.textAlign = "left";
  }

  /** Log-spaced bars with falling peak caps, coloured like the peak meter. */
  function drawSpectrum(ctx: CanvasRenderingContext2D, w: number, h: number) {
    const gap = 1;
    const bw = Math.max(1, (w - gap * (BANDS - 1)) / BANDS);
    const toY = (level: number) => clamp01((level - FLOOR_DB) / -FLOOR_DB) * h;
    for (let b = 0; b < BANDS; b++) {
      const x = b * (bw + gap);
      const bh = toY(bands[b]);
      const grad = ctx.createLinearGradient(0, h, 0, 0);
      grad.addColorStop(0, tokens.lo);
      grad.addColorStop(0.7, tokens.mid);
      grad.addColorStop(1, tokens.hi);
      ctx.fillStyle = grad;
      ctx.globalAlpha = 0.85;
      ctx.fillRect(x, h - bh, bw, bh);
      ctx.globalAlpha = 1;
      if (bandPeaks[b] > FLOOR_DB) {
        ctx.fillStyle = tokens.fg2;
        ctx.fillRect(x, Math.max(0, h - toY(bandPeaks[b]) - 1), bw, 1);
      }
    }
  }

  // Start the loop whenever there is something to show; it stops itself once
  // playback has stopped and every meter has fallen back to rest.
  createEffect(() => {
    const mode = props.mode;
    void props.playing;
    if (mode === "off") return;
    clearCanvas();
    if (!frame) {
      last = performance.now();
      frame = requestAnimationFrame(tick);
    }
  });

  onCleanup(() => {
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
  });

  const title = () => METER_MODES.find((m) => m.value === props.mode)?.label ?? "";

  return (
    <canvas
      class={`tmeter ${props.mode}`}
      ref={(el) => (canvas = el)}
      data-tip={`${title()} - click for the next`}
      data-tip-at="above"
      onClick={props.onCycle}
    />
  );
}

function window_dpr() {
  return globalThis.devicePixelRatio || 1;
}

function hann(n: number): Float32Array {
  const w = new Float32Array(n);
  for (let i = 0; i < n; i++) w[i] = 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (n - 1));
  return w;
}

/** In-place radix-2 FFT. `re.length` must be a power of two. */
function fft(re: Float32Array, im: Float32Array) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j], re[i]];
      [im[i], im[j]] = [im[j], im[i]];
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    const wr = Math.cos(ang);
    const wi = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let cr = 1;
      let ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const a = i + k;
        const b = a + len / 2;
        const tr = re[b] * cr - im[b] * ci;
        const ti = re[b] * ci + im[b] * cr;
        re[b] = re[a] - tr;
        im[b] = im[a] - ti;
        re[a] += tr;
        im[a] += ti;
        const next = cr * wr - ci * wi;
        ci = cr * wi + ci * wr;
        cr = next;
      }
    }
  }
}
