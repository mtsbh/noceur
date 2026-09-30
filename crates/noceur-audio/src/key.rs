//! Musical key, by a small convolutional network.
//!
//! The model is KeyNet (Korzeniowski & Widmer, "Genre-Agnostic Key
//! Classification With Convolutional Neural Networks", ISMIR 2018), with the
//! weights OpenKeyScan retrained from MusicalKeyCNN's. They ship with the app
//! (`models/`, MIT); inference is written out here rather
//! than pulled in through a machine-learning runtime, because it is nine
//! convolutions and nothing else.
//!
//! Its input is a constant-Q spectrogram computed the way librosa computes one
//! — the network was trained on librosa's, and learned its exact scale — so
//! [`cqt`] follows librosa's filters bin for bin. librosa gets there by
//! repeatedly halving the sample rate for the lower octaves; that is a speed-up,
//! not a different answer, and this computes every bin at the file's own rate.
//!
//! Only an estimate. Nothing here writes a tag; the user decides.

use std::path::Path;
use std::sync::OnceLock;

/// Spectrogram shape the model was trained on: 105 bins at 24 per octave from
/// 65 Hz (C2 to about 1.3 kHz), a frame every fifth of a second.
const BINS: usize = 105;
const BINS_PER_OCTAVE: f64 = 24.0;
const FMIN: f64 = 65.0;
const FRAME_SECS: f64 = 0.2;

/// The weights, with batch norm already folded into each convolution; see
/// `models/README.md` for where they come from.
static WEIGHTS: &[u8] = include_bytes!("../models/openkeyscan2.bin");

/// A key estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    /// Camelot notation, `1A`..`12B`, the form every key tag in the app is
    /// written in.
    pub camelot: String,
    /// The model's probability for that key, 0..1. Not calibrated; useful to
    /// tell a confident answer from a coin toss between neighbours.
    pub confidence: f32,
}

/// Estimates a file's key.
pub fn detect(path: &Path) -> Result<Option<Estimate>, crate::AudioError> {
    let d = crate::decode(path, crate::ANALYSIS_SECS)?;
    Ok(estimate(&d.samples, d.rate, d.channels))
}

/// Estimates the key of interleaved samples with the bundled model. `None`
/// when there is too little audio to say.
pub fn estimate(samples: &[f32], rate: u32, channels: usize) -> Option<Estimate> {
    estimate_with(Model::bundled(), samples, rate, channels)
}

/// A set of KeyNet weights.
///
/// The app only ever uses [`Model::bundled`]; others are loaded to measure
/// alternatives against it.
pub struct Model(Network);

impl Model {
    /// The weights that ship with the app.
    pub fn bundled() -> &'static Model {
        static NET: OnceLock<Model> = OnceLock::new();
        NET.get_or_init(|| Model::parse(WEIGHTS).expect("the bundled key model is well formed"))
    }

    /// Weights in the format `models/export.py` writes; `None` if malformed.
    pub fn parse(bytes: &[u8]) -> Option<Model> {
        parse(bytes).map(Model)
    }
}

/// As [`estimate`], with the given weights.
pub fn estimate_with(
    model: &Model,
    samples: &[f32],
    rate: u32,
    channels: usize,
) -> Option<Estimate> {
    let channels = channels.max(1);
    let mono: Vec<f32> = samples
        .chunks_exact(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect();
    let (spec, frames) = cqt(&mono, rate);
    // librosa's last two frames are dropped before the model sees them, as
    // they were in training; a track needs a few seconds left after that.
    if frames < 2 + 10 {
        return None;
    }
    let frames = frames - 2;
    let input: Vec<f32> = (0..BINS)
        .flat_map(|b| spec[b * (frames + 2)..][..frames].iter().copied())
        .collect();
    let logits = model.0.forward(&input, BINS, frames);
    let best = (0..logits.len()).max_by(|&a, &b| logits[a].total_cmp(&logits[b]))?;

    let top = logits[best];
    let total: f32 = logits.iter().map(|l| (l - top).exp()).sum();
    Some(Estimate {
        camelot: camelot(best),
        confidence: 1.0 / total,
    })
}

/// The model's class index as Camelot: `idx % 12 + 1`, minor (`A`) below 12.
fn camelot(class: usize) -> String {
    format!("{}{}", class % 12 + 1, if class < 12 { 'A' } else { 'B' })
}

/// One constant-Q filter, stored as the part of its spectrum that matters.
struct Filter {
    /// `(fft bin, re, im)`, the bins holding all but 1% of the filter's
    /// magnitude, as librosa's `sparsify_rows` keeps.
    taps: Vec<(usize, f32, f32)>,
    /// Filter length in samples, for librosa's `scale=True` normalisation.
    length: f64,
}

/// `log1p(|CQT|)`, bins × frames row-major, and the frame count.
///
/// librosa's `cqt(y, sr, hop_length=0.2 s, n_bins=105, bins_per_octave=24,
/// fmin=65)` with its defaults: Hann-windowed complex wavelets of Q ≈ 34.6,
/// each normalised to unit L1 norm and applied in the frequency domain, frames
/// centred on multiples of the hop with zero padding, and each bin divided by
/// the square root of its filter length.
pub fn cqt(mono: &[f32], rate: u32) -> (Vec<f32>, usize) {
    // The top bin is 1.3 kHz, so everything above ~11 kHz is only cost: at
    // 44.1 kHz the lowest bin's filter needs a 32k-point transform per frame,
    // at a quarter of the rate an 8k one. librosa computes its lower octaves
    // at the reduced rate for the same reason.
    let (reduced, sr) = decimate(mono, rate);
    let mono = &reduced[..];
    // librosa divides by the square root of each filter's length at the
    // original rate. The filters here are shorter by the reduction factor, so
    // their responses are smaller by it; this puts back the difference.
    let rescale = (rate as f64 / sr).sqrt();
    let hop = (sr * FRAME_SECS).round() as usize;
    // librosa's relative bandwidth for evenly spaced bins, and the Q from it.
    let ratio = 2f64.powf(2.0 / BINS_PER_OCTAVE);
    let q = 1.0 / ((ratio - 1.0) / (ratio + 1.0));
    let freqs: Vec<f64> = (0..BINS)
        .map(|k| FMIN * 2f64.powf(k as f64 / BINS_PER_OCTAVE))
        .collect();
    let lengths: Vec<f64> = freqs.iter().map(|f| q * sr / f).collect();
    let n = (lengths[0].ceil() as usize).next_power_of_two();
    let plan = crate::fft::Plan::new(n);

    let filters: Vec<Filter> = freqs
        .iter()
        .zip(&lengths)
        .map(|(&f, &length)| filter(f, length, sr, n, &plan))
        .collect();

    let frames = 1 + mono.len() / hop;
    let mut out = vec![0.0f32; BINS * frames];
    let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
    for t in 0..frames {
        // The frame spans [t·hop − n/2, t·hop + n/2), silence past either end.
        let start = (t * hop) as isize - (n / 2) as isize;
        for (i, v) in re.iter_mut().enumerate() {
            let at = start + i as isize;
            *v = if at >= 0 && (at as usize) < mono.len() {
                mono[at as usize]
            } else {
                0.0
            };
        }
        im.fill(0.0);
        plan.run(&mut re, &mut im);
        for (b, flt) in filters.iter().enumerate() {
            let (mut sr_, mut si) = (0.0f64, 0.0f64);
            for &(k, fr, fi) in &flt.taps {
                // No conjugate: librosa takes `basis_fft · stft`.
                sr_ += (fr * re[k] - fi * im[k]) as f64;
                si += (fr * im[k] + fi * re[k]) as f64;
            }
            let mag = (sr_ * sr_ + si * si).sqrt() / flt.length.sqrt() * rescale;
            out[b * frames + t] = (mag as f32).ln_1p();
        }
    }
    (out, frames)
}

/// The highest frequency any filter passes, with room for its skirt: the top
/// bin is 65·2^(104/24) ≈ 1.31 kHz and its bandwidth about 1/35 of that.
const PASS_HZ: f64 = 1_400.0;

/// Low-passed and kept every `d`-th sample, `d` the largest whole factor that
/// leaves at least 11,025 Hz. Returns the samples and their rate.
///
/// A windowed-sinc FIR, evaluated only at the samples kept. What folds back
/// onto the analysed band comes from within `PASS_HZ` of a multiple of the new
/// rate, so the filter only has to be flat to 1.4 kHz and deep by
/// `rate/d − 1.4 kHz`; a Blackman window reaches about 74 dB over that span.
fn decimate(mono: &[f32], rate: u32) -> (Vec<f32>, f64) {
    let sr = rate as f64;
    let d = ((sr / 11_025.0).floor() as usize).max(1);
    if d == 1 {
        return (mono.to_vec(), sr);
    }
    let out_rate = sr / d as f64;
    let transition = out_rate - 2.0 * PASS_HZ;
    // Blackman: transition width ≈ 5.5·rate / taps.
    let taps = ((5.5 * sr / transition).ceil() as usize) | 1;
    let cutoff = out_rate / 2.0 / sr; // cycles per input sample
    let half = (taps / 2) as isize;
    let mut kernel: Vec<f64> = (0..taps as isize)
        .map(|i| {
            let n = (i - half) as f64;
            let sinc = if n == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * n).sin() / (std::f64::consts::PI * n)
            };
            let x = 2.0 * std::f64::consts::PI * i as f64 / (taps - 1) as f64;
            sinc * (0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos())
        })
        .collect();
    // Unity gain at 0 Hz, so levels match what the model was trained on.
    let dc: f64 = kernel.iter().sum();
    for k in &mut kernel {
        *k /= dc;
    }
    let kernel: Vec<f32> = kernel.into_iter().map(|k| k as f32).collect();

    let len = mono.len().div_ceil(d);
    let out = (0..len)
        .map(|m| {
            let centre = (m * d) as isize;
            let mut acc = 0.0f32;
            for (i, k) in kernel.iter().enumerate() {
                let at = centre + i as isize - half;
                if at >= 0 && (at as usize) < mono.len() {
                    acc += k * mono[at as usize];
                }
            }
            acc
        })
        .collect();
    (out, out_rate)
}

/// librosa's wavelet for one bin, transformed and trimmed.
fn filter(freq: f64, length: f64, sr: f64, n: usize, plan: &crate::fft::Plan) -> Filter {
    // `np.arange(-ilen // 2, ilen // 2)`, with `ilen` a float.
    let lo = (-length / 2.0).floor() as i64;
    let hi = (length / 2.0).floor() as i64;
    let len = (hi - lo) as usize;
    // A periodic Hann of that many points, unit L1 norm overall, then scaled
    // by `length / n` as librosa re-normalises for the transform length.
    let window: Vec<f64> = (0..len)
        .map(|j| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * j as f64 / len as f64).cos())
        .collect();
    let norm: f64 = window.iter().sum();
    let gain = length / n as f64 / norm;

    let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
    let pad = (n - len) / 2;
    for (j, w) in window.iter().enumerate() {
        let phase = 2.0 * std::f64::consts::PI * freq * (lo + j as i64) as f64 / sr;
        re[pad + j] = (w * gain * phase.cos()) as f32;
        im[pad + j] = (w * gain * phase.sin()) as f32;
    }
    plan.run(&mut re, &mut im);

    // Keep the largest bins until all but 1% of the magnitude is accounted
    // for, over the non-negative frequencies — librosa's sparsity=0.01.
    let half = n / 2 + 1;
    let mags: Vec<f32> = (0..half)
        .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt())
        .collect();
    let total: f64 = mags.iter().map(|&m| m as f64).sum();
    let mut sorted = mags.clone();
    sorted.sort_by(f32::total_cmp);
    let mut acc = 0.0f64;
    let mut threshold = 0.0f32;
    for &m in &sorted {
        acc += m as f64;
        if acc / total >= 0.01 {
            threshold = m;
            break;
        }
    }
    let taps = (0..half)
        .filter(|&k| mags[k] >= threshold)
        .map(|k| (k, re[k], im[k]))
        .collect();
    Filter { taps, length }
}

/// One convolution with batch norm folded in: `out = conv(in) + bias`.
struct Conv {
    cout: usize,
    cin: usize,
    k: usize,
    /// `[cout][cin][k][k]`.
    weights: Vec<f32>,
    bias: Vec<f32>,
}

struct Network {
    layers: Vec<Conv>,
}

/// Reads the flat weight file `models/export.py` writes.
fn parse(bytes: &[u8]) -> Option<Network> {
    let mut rest = bytes.strip_prefix(b"NKEY")?;
    let mut take = |n: usize| -> Option<&[u8]> {
        let (head, tail) = (rest.get(..n)?, rest.get(n..)?);
        rest = tail;
        Some(head)
    };
    let u32_at = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap()) as usize;
    if u32_at(take(4)?) != 1 {
        return None;
    }
    let count = u32_at(take(4)?);
    let mut layers = Vec::with_capacity(count);
    for _ in 0..count {
        let (cout, cin, k) = (u32_at(take(4)?), u32_at(take(4)?), u32_at(take(4)?));
        let floats = |raw: &[u8]| -> Vec<f32> {
            raw.chunks_exact(4)
                .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
                .collect()
        };
        let weights = floats(take(cout * cin * k * k * 4)?);
        let bias = floats(take(cout * 4)?);
        layers.push(Conv {
            cout,
            cin,
            k,
            weights,
            bias,
        });
    }
    (layers.len() == 9).then_some(Network { layers })
}

impl Network {
    /// Class scores for a `h × w` spectrogram.
    ///
    /// KeyNet: two convolutions, pool, two, pool, two, pool, two more, a 1×1
    /// convolution to 24 classes, and the average over everything left. ELU
    /// after every convolution, including the last.
    fn forward(&self, input: &[f32], h: usize, w: usize) -> Vec<f32> {
        let (mut x, mut h, mut w) = (input.to_vec(), h, w);
        for (i, layer) in self.layers.iter().enumerate() {
            x = layer.apply(&x, h, w);
            for v in &mut x {
                if *v < 0.0 {
                    *v = v.exp_m1();
                }
            }
            if matches!(i, 1 | 3 | 5) {
                (x, h, w) = pool(&x, layer.cout, h, w);
            }
        }
        let area = (h * w) as f32;
        x.chunks_exact(h * w)
            .map(|c| c.iter().sum::<f32>() / area)
            .collect()
    }
}

/// 2×2 max pooling, stride 2, a trailing odd row or column dropped.
fn pool(x: &[f32], c: usize, h: usize, w: usize) -> (Vec<f32>, usize, usize) {
    let (ph, pw) = (h / 2, w / 2);
    let mut out = Vec::with_capacity(c * ph * pw);
    for ch in x.chunks_exact(h * w) {
        for y in 0..ph {
            for xx in 0..pw {
                let at = |dy: usize, dx: usize| ch[(2 * y + dy) * w + 2 * xx + dx];
                out.push(at(0, 0).max(at(0, 1)).max(at(1, 0)).max(at(1, 1)));
            }
        }
    }
    (out, ph, pw)
}

impl Conv {
    /// "Same" convolution over `[cin][h][w]`, zero padded.
    ///
    /// As a matrix product: output positions are taken a tile at a time, the
    /// input patches under them unrolled into columns (im2col), and each
    /// output channel is its weight row times those columns. The channels are
    /// shared out across threads; the columns are read by all of them.
    fn apply(&self, x: &[f32], h: usize, w: usize) -> Vec<f32> {
        let (k, pad) = (self.k, self.k / 2);
        let patch = self.cin * k * k;
        let positions = h * w;
        // Columns kept to a few megabytes whatever the layer.
        let tile = (2_000_000 / patch).clamp(64, 4096).min(positions);
        let mut out = vec![0.0f32; self.cout * positions];
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let per = self.cout.div_ceil(threads);
        let mut cols = vec![0.0f32; patch * tile];

        for start in (0..positions).step_by(tile) {
            let len = tile.min(positions - start);
            for ci in 0..self.cin {
                let plane = &x[ci * positions..][..positions];
                for dy in 0..k {
                    for dx in 0..k {
                        let row = &mut cols[((ci * k + dy) * k + dx) * tile..][..len];
                        for (j, v) in row.iter_mut().enumerate() {
                            let p = start + j;
                            let (y, xx) = (
                                (p / w) as isize + dy as isize - pad as isize,
                                (p % w) as isize + dx as isize - pad as isize,
                            );
                            *v = if y >= 0 && (y as usize) < h && xx >= 0 && (xx as usize) < w {
                                plane[y as usize * w + xx as usize]
                            } else {
                                0.0
                            };
                        }
                    }
                }
            }
            let cols = &cols;
            std::thread::scope(|s| {
                for (chunk, outs) in out.chunks_mut(per * positions).enumerate() {
                    s.spawn(move || {
                        for (i, plane) in outs.chunks_mut(positions).enumerate() {
                            let co = chunk * per + i;
                            let acc = &mut plane[start..start + len];
                            acc.fill(self.bias[co]);
                            let row = &self.weights[co * patch..][..patch];
                            for (p, &wv) in row.iter().enumerate() {
                                let src = &cols[p * tile..][..len];
                                for (a, s) in acc.iter_mut().zip(src) {
                                    *a += wv * s;
                                }
                            }
                        }
                    });
                }
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_are_named_as_on_the_camelot_wheel() {
        assert_eq!(camelot(0), "1A"); // G# minor
        assert_eq!(camelot(7), "8A"); // A minor
        assert_eq!(camelot(19), "8B"); // C major
        assert_eq!(camelot(23), "12B"); // E major
    }

    #[test]
    fn the_bundled_model_loads_with_keynets_shape() {
        let net = &Model::bundled().0;
        let shapes: Vec<(usize, usize, usize)> =
            net.layers.iter().map(|l| (l.cout, l.cin, l.k)).collect();
        assert_eq!(shapes[0], (20, 1, 5));
        assert_eq!(shapes[8], (24, 160, 1));
    }

    /// A 440 Hz tone lands in the bin 24·log2(440/65) ≈ 66 above 65 Hz.
    #[test]
    fn a_tone_peaks_in_its_own_bin() {
        let rate = 44_100;
        let tone: Vec<f32> = (0..rate * 4)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect();
        let (spec, frames) = cqt(&tone, rate as u32);
        let t = frames / 2;
        let peak = (0..BINS)
            .max_by(|&a, &b| spec[a * frames + t].total_cmp(&spec[b * frames + t]))
            .unwrap();
        assert_eq!(peak, 66);
    }

    #[test]
    fn a_convolution_matches_one_done_by_hand() {
        // One input channel, 3×3 kernel of ones, bias 1: each output is 1 plus
        // the sum of its neighbourhood, zeros past the edge.
        let conv = Conv {
            cout: 1,
            cin: 1,
            k: 3,
            weights: vec![1.0; 9],
            bias: vec![1.0],
        };
        let x: Vec<f32> = (1..=6).map(|v| v as f32).collect(); // 2 × 3
        let out = conv.apply(&x, 2, 3);
        assert_eq!(out, vec![13.0, 22.0, 17.0, 13.0, 22.0, 17.0]);
    }

    #[test]
    fn silence_too_short_to_judge_has_no_key() {
        assert_eq!(estimate(&[0.0; 44_100], 44_100, 1), None);
    }
}
