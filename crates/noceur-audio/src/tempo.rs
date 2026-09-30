//! Tempo: how many beats a minute a track runs at.
//!
//! librosa's method, ported rather than shipped with a Python interpreter:
//! an onset-strength envelope (spectral flux on a mel spectrogram), its
//! autocorrelation, and the strongest period weighted by a tempo prior. On top
//! of that, one step librosa does not take: its answer is only as fine as one
//! envelope frame, which at 128 BPM is several BPM wide, and a DJ needs the
//! decimal. So the coarse answer is refined by lining up many beats at once —
//! over two dozen beats, a tenth of a BPM moves the last one by a whole frame.
//!
//! Only an estimate. Nothing here writes a tag; the user decides.

use std::path::Path;

/// What the envelope is computed at, roughly: most files are 44.1 or 48 kHz and
/// are averaged down by two. Onsets live well below this.
const TARGET_RATE: u32 = 22_050;
/// Window and hop of the spectrogram, at the reduced rate: 46 ms and 11.6 ms.
const N_FFT: usize = 1024;
const HOP: usize = 256;
const MEL_BANDS: usize = 40;
const MEL_MIN_HZ: f32 = 30.0;
const MEL_MAX_HZ: f32 = 11_000.0;
/// Quieter than this below the loudest bin is treated as silence, as librosa's
/// `top_db` does, so the noise floor does not produce onsets of its own.
const TOP_DB: f32 = 80.0;
/// Beats lined up by the fine search.
const FINE_BEATS: usize = 24;
/// Where the tempo prior is centred. librosa uses 120; 125 is the middle of a
/// club library, and it moves the half-or-double tie-break to where DJs put
/// it: a track that is as much 87 as 174 reads as 174 (drum and bass), while
/// one that is as much 90 as 180 stays at 90. The tie flips at `PRIOR_BPM / √2`
/// and its double, about 88 and 177.
const PRIOR_BPM: f64 = 125.0;

/// The tempi a result may fall in.
///
/// An estimator hears a track at 87 and at 174 BPM equally well; which is right
/// is a convention of the genre, not something in the audio. The range settles
/// it: only tempi inside it are considered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

impl Default for Range {
    fn default() -> Self {
        Range {
            min: 70.0,
            max: 180.0,
        }
    }
}

/// A tempo estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub bpm: f64,
    /// How strongly the beats line up at that tempo, 0..1. A four-to-the-floor
    /// track sits well above a beatless ambient one.
    pub confidence: f64,
}

/// Estimates a file's tempo.
pub fn detect(path: &Path, range: Range) -> Result<Option<Estimate>, crate::AudioError> {
    let d = crate::decode(path, crate::ANALYSIS_SECS)?;
    Ok(estimate(&d.samples, d.rate, d.channels, range))
}

/// Estimates the tempo of interleaved samples. `None` when there is too little
/// audio, or nothing rhythmic in it at all.
pub fn estimate(samples: &[f32], rate: u32, channels: usize, range: Range) -> Option<Estimate> {
    let (mono, rate) = downmix(samples, rate, channels.max(1));
    let env = onset_envelope(&mono, rate);
    let fps = rate as f64 / HOP as f64;
    tempo(&env, fps, range)
}

/// Mono, averaged down to about [`TARGET_RATE`].
///
/// A plain average of neighbouring samples is a poor low-pass, and the alias it
/// lets through does not matter: what follows only asks when energy arrives,
/// not at what pitch.
fn downmix(samples: &[f32], rate: u32, channels: usize) -> (Vec<f32>, u32) {
    let factor = ((rate as f32 / TARGET_RATE as f32).round() as usize).max(1);
    let step = channels * factor;
    let mono = samples
        .chunks_exact(step)
        .map(|c| c.iter().sum::<f32>() / step as f32)
        .collect();
    (mono, rate / factor as u32)
}

/// Onset strength per frame: how much louder each mel band got since the frame
/// before, averaged across bands, with its slow drift taken out.
fn onset_envelope(mono: &[f32], rate: u32) -> Vec<f32> {
    if mono.len() < N_FFT {
        return Vec::new();
    }
    let window = crate::fft::hann(N_FFT);
    let bank = mel_bank(rate);
    let frames = (mono.len() - N_FFT) / HOP + 1;

    let mut spec = Vec::with_capacity(frames * MEL_BANDS);
    let (mut re, mut im) = (vec![0.0f32; N_FFT], vec![0.0f32; N_FFT]);
    for f in 0..frames {
        let at = f * HOP;
        for i in 0..N_FFT {
            re[i] = mono[at + i] * window[i];
            im[i] = 0.0;
        }
        crate::fft::fft(&mut re, &mut im);
        let power: Vec<f32> = (0..=N_FFT / 2)
            .map(|k| re[k] * re[k] + im[k] * im[k])
            .collect();
        for band in &bank {
            let e: f32 = band.iter().map(|&(k, w)| power[k] * w).sum();
            spec.push(10.0 * e.max(1e-10).log10());
        }
    }

    let loudest = spec.iter().copied().fold(f32::MIN, f32::max);
    for db in &mut spec {
        *db = db.max(loudest - TOP_DB);
    }

    let mut flux = vec![0.0f32; frames];
    for f in 1..frames {
        let (now, before) = (
            &spec[f * MEL_BANDS..][..MEL_BANDS],
            &spec[(f - 1) * MEL_BANDS..][..MEL_BANDS],
        );
        flux[f] = now
            .iter()
            .zip(before)
            .map(|(a, b)| (a - b).max(0.0))
            .sum::<f32>()
            / MEL_BANDS as f32;
    }

    // A breakdown is quieter than a drop, and that difference is not a beat.
    // Taking out a one-second moving average leaves the onsets and removes the
    // swell, which would otherwise dominate the autocorrelation.
    let half = (rate as usize / HOP) / 2;
    let mut prefix = vec![0.0f64; frames + 1];
    for (i, v) in flux.iter().enumerate() {
        prefix[i + 1] = prefix[i] + *v as f64;
    }
    (0..frames)
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(half), (i + half + 1).min(frames));
            let mean = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
            flux[i] - mean as f32
        })
        .collect()
}

/// Triangular mel filters over the FFT bins, as `(bin, weight)` lists.
fn mel_bank(rate: u32) -> Vec<Vec<(usize, f32)>> {
    let mel = |hz: f32| 2595.0 * (1.0 + hz / 700.0).log10();
    let hz = |m: f32| 700.0 * (10f32.powf(m / 2595.0) - 1.0);
    let top = MEL_MAX_HZ.min(rate as f32 / 2.0);
    let (lo, hi) = (mel(MEL_MIN_HZ), mel(top));
    let edges: Vec<f32> = (0..MEL_BANDS + 2)
        .map(|i| hz(lo + (hi - lo) * i as f32 / (MEL_BANDS + 1) as f32))
        .collect();
    let bin_hz = rate as f32 / N_FFT as f32;

    (0..MEL_BANDS)
        .map(|b| {
            let (l, c, r) = (edges[b], edges[b + 1], edges[b + 2]);
            (0..=N_FFT / 2)
                .filter_map(|k| {
                    let f = k as f32 * bin_hz;
                    let w = if f > l && f <= c {
                        (f - l) / (c - l)
                    } else if f > c && f < r {
                        (r - f) / (r - c)
                    } else {
                        0.0
                    };
                    (w > 0.0).then_some((k, w))
                })
                .collect()
        })
        .collect()
}

/// The tempo an onset envelope beats at.
fn tempo(env: &[f32], fps: f64, range: Range) -> Option<Estimate> {
    let (min, max) = (range.min.max(1.0), range.max.max(range.min + 1.0));
    let lag_of = |bpm: f64| 60.0 * fps / bpm;

    // Enough lags for the fine search to line up its beats at the slowest
    // tempo, plus a little for interpolating past the last one.
    let longest =
        ((lag_of(min * 0.985) * FINE_BEATS as f64).ceil() as usize + 4).min(env.len() / 2);
    if (longest as f64) <= lag_of(min) * 4.0 {
        return None;
    }
    let ac = autocorrelation(env, longest);
    if ac[0] <= 0.0 {
        return None;
    }

    // An onset is a frame or two wide, and a beat that lands between frames
    // splits across them. Smoothed, a period between whole frames still finds
    // its peak — widely for the coarse pass, which has to find it from afar,
    // and barely for the fine one, which has to say exactly where it is.
    let wide = smooth(&ac, &[1.0, 2.0, 3.0, 2.0, 1.0]);
    let narrow = smooth(&ac, &[1.0, 2.0, 1.0]);
    let at = |ac: &[f64], pos: f64| -> f64 {
        let i = pos.floor() as usize;
        let t = pos - i as f64;
        match (ac.get(i), ac.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * t,
            _ => 0.0,
        }
    };

    // Coarse: tempi across the range a fifth of a percent apart, each scored
    // on its first four beats and weighted by a log-normal prior, as librosa
    // does. The prior only breaks near-ties; a clearly stronger period wins
    // regardless.
    let score = |bpm: f64| -> f64 {
        let period = lag_of(bpm);
        (1..=4)
            .map(|k| at(&wide, k as f64 * period) / k as f64)
            .sum()
    };
    let weighted = |bpm: f64, s: f64| {
        let prior = -0.5 * (bpm / PRIOR_BPM).log2().powi(2);
        (1e6 * s.max(0.0)).ln_1p() + prior
    };
    let mut coarse = None::<(f64, f64, f64)>;
    let mut bpm = min;
    while bpm <= max {
        let s = score(bpm);
        let w = weighted(bpm, s);
        if coarse.is_none_or(|(_, _, best)| w > best) {
            coarse = Some((bpm, s, w));
        }
        bpm *= 1.002;
    }
    let (coarse, s, _) = coarse?;
    if s <= 0.0 {
        return None;
    }

    // Fine: many beats at once. A period off by a hundredth of a frame is off
    // by a quarter frame by the twenty-fourth beat, where a single lag could
    // not tell the difference.
    let comb = |bpm: f64| -> f64 {
        let period = lag_of(bpm);
        let beats = (1..=FINE_BEATS).take_while(|k| *k as f64 * period + 1.0 < ac.len() as f64);
        let (sum, n) = beats.fold((0.0, 0), |(s, n), k| {
            (s + at(&narrow, k as f64 * period), n + 1)
        });
        if n == 0 {
            0.0
        } else {
            sum / n as f64
        }
    };
    let (mut bpm, mut strength) = (coarse, comb(coarse));
    let steps = 300;
    for i in 0..=steps {
        let candidate = coarse * (0.985 + 0.03 * i as f64 / steps as f64);
        let s = comb(candidate);
        if s > strength {
            (bpm, strength) = (candidate, s);
        }
    }

    Some(Estimate {
        bpm: bpm.clamp(min, max),
        confidence: strength.clamp(0.0, 1.0),
    })
}

/// A tempo as it is worth writing: a whole number when it is within a tenth of
/// one, otherwise to one decimal.
///
/// Nearly every club track is produced at a whole BPM, and the estimate lands
/// within a few hundredths of it; `124.03` in a tag would be noise. A track
/// that genuinely sits between — a 128.5 edit, a hand-played record — keeps
/// its half.
pub fn rounded(bpm: f64) -> f64 {
    let whole = bpm.round();
    if (bpm - whole).abs() <= 0.1 {
        whole
    } else {
        (bpm * 10.0).round() / 10.0
    }
}

/// `values` convolved with a symmetric kernel, which is normalised here.
fn smooth(values: &[f64], kernel: &[f64]) -> Vec<f64> {
    let total: f64 = kernel.iter().sum();
    let half = kernel.len() / 2;
    (0..values.len())
        .map(|i| {
            kernel
                .iter()
                .enumerate()
                .map(|(j, w)| {
                    let at = (i + j).checked_sub(half).and_then(|k| values.get(k));
                    // Past either end, the edge value stands in.
                    let v = at.copied().unwrap_or(if i < half {
                        values[0]
                    } else {
                        values[values.len() - 1]
                    });
                    v * w
                })
                .sum::<f64>()
                / total
        })
        .collect()
}

/// Normalised autocorrelation for lags `0..lags`, each divided by the overlap
/// it was summed over so long lags are not penalised for being long.
fn autocorrelation(env: &[f32], lags: usize) -> Vec<f64> {
    let n = env.len();
    let mut ac: Vec<f64> = (0..lags.min(n))
        .map(|lag| {
            let sum: f64 = env[..n - lag]
                .iter()
                .zip(&env[lag..])
                .map(|(a, b)| (*a as f64) * (*b as f64))
                .sum();
            sum / (n - lag) as f64
        })
        .collect();
    let zero = ac.first().copied().unwrap_or(0.0);
    if zero > 0.0 {
        for v in &mut ac {
            *v /= zero;
        }
    }
    ac
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// A drum pattern: a decaying low thump on each beat, and optionally a
    /// short noise hit on the offbeat, the way a house track sits.
    fn pattern(bpm: f64, secs: f64, offbeat: bool) -> Vec<f32> {
        let n = (RATE as f64 * secs) as usize;
        let period = 60.0 / bpm * RATE as f64;
        let mut noise = 0x1234_5678u32;
        let mut out = vec![0.0f32; n];
        let mut beat = 0.0;
        while (beat as usize) < n {
            let start = beat as usize;
            for i in 0..(RATE as usize / 8).min(n - start) {
                let t = i as f32 / RATE as f32;
                out[start + i] +=
                    (2.0 * std::f32::consts::PI * 55.0 * t).sin() * (-t * 30.0).exp() * 0.8;
            }
            if offbeat {
                let hat = (beat + period / 2.0) as usize;
                for i in 0..(RATE as usize / 40).min(n.saturating_sub(hat)) {
                    noise ^= noise << 13;
                    noise ^= noise >> 17;
                    noise ^= noise << 5;
                    let white = (noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
                    out[hat + i] += white * 0.3 * (-(i as f32) / 200.0).exp();
                }
            }
            beat += period;
        }
        out
    }

    fn bpm_of(samples: &[f32], range: Range) -> f64 {
        estimate(samples, RATE, 1, range).expect("an estimate").bpm
    }

    #[test]
    fn a_four_to_the_floor_pattern_is_found_to_the_decimal() {
        for bpm in [122.0, 124.5, 128.0, 132.0] {
            let got = bpm_of(&pattern(bpm, 60.0, true), Range::default());
            assert!((got - bpm).abs() < 0.1, "{bpm}: got {got}");
        }
    }

    /// A kick on every beat is exactly as much 87 as 174; the prior puts the
    /// tie where DJs do. The same pattern at 90 is hip-hop, not 180.
    #[test]
    fn half_or_double_time_ties_go_the_way_a_dj_counts() {
        let got = bpm_of(&pattern(174.0, 60.0, false), Range::default());
        assert!((got - 174.0).abs() < 0.1, "drum and bass: got {got}");

        let got = bpm_of(&pattern(90.0, 60.0, false), Range::default());
        assert!((got - 90.0).abs() < 0.1, "hip-hop: got {got}");
    }

    #[test]
    fn a_whole_bpm_is_written_whole_and_a_half_keeps_its_half() {
        assert_eq!(rounded(124.03), 124.0);
        assert_eq!(rounded(125.95), 126.0);
        assert_eq!(rounded(128.51), 128.5);
        assert_eq!(rounded(122.87), 122.9);
    }

    #[test]
    fn the_range_settles_half_and_double_time() {
        let fast = pattern(128.0, 60.0, false);
        let got = bpm_of(
            &fast,
            Range {
                min: 50.0,
                max: 90.0,
            },
        );
        assert!((got - 64.0).abs() < 0.1, "half time: got {got}");

        let slow = pattern(85.0, 60.0, false);
        let got = bpm_of(
            &slow,
            Range {
                min: 150.0,
                max: 190.0,
            },
        );
        assert!((got - 170.0).abs() < 0.2, "double time: got {got}");
    }

    #[test]
    fn stereo_and_48k_give_the_same_answer() {
        let mono = pattern(126.0, 40.0, true);
        let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
        let got = estimate(&stereo, RATE, 2, Range::default()).unwrap().bpm;
        assert!((got - 126.0).abs() < 0.1, "stereo: got {got}");

        // The same pattern rendered at 48 kHz, by resampling the time axis.
        let n = mono.len() * 48_000 / RATE as usize;
        let at48: Vec<f32> = (0..n).map(|i| mono[i * RATE as usize / 48_000]).collect();
        let got = estimate(&at48, 48_000, 1, Range::default()).unwrap().bpm;
        assert!((got - 126.0).abs() < 0.1, "48k: got {got}");
    }

    #[test]
    fn silence_and_scraps_have_no_tempo() {
        assert_eq!(
            estimate(&vec![0.0; RATE as usize * 20], RATE, 1, Range::default()),
            None
        );
        assert_eq!(estimate(&[0.1; 500], RATE, 1, Range::default()), None);
    }
}
