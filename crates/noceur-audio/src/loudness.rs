//! Loudness as a listener hears it, and peaks as a converter sees them.
//!
//! Integrated loudness is ITU-R BS.1770-4 (what EBU R128 and every streaming
//! service quote): each channel through the K-weighting filter, mean square over
//! 400 ms blocks every 100 ms, the blocks under -70 LUFS dropped, then the
//! blocks more than 10 LU under what is left dropped too, so a long quiet intro
//! does not drag a club track's number down.
//!
//! True peak is BS.1770's Annex 2: the signal oversampled four times (twice at
//! 96 kHz, as is at 192 kHz) and the largest value taken. A sample peak misses
//! the overs that happen between samples, which is where a track mastered to
//! 0.0 dBFS clips once a DAC or an MP3 decoder reconstructs it.
//!
//! Fed interleaved samples as they are decoded, so it shares the quality
//! check's one pass over the file.

/// What was measured.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Loudness {
    /// Integrated loudness in LUFS. `None` for silence, or a file shorter than
    /// one 400 ms block.
    pub lufs: Option<f32>,
    /// Largest inter-sample peak in dBTP. `None` for digital silence.
    pub true_peak_db: Option<f32>,
    /// Largest sample as decoded, linear. Symphonia's MP3 decoder clips its
    /// output to ±1.0, so a lossy file that reaches 1.0 here has overs the
    /// meter never saw, and its true peak is a lower bound.
    pub sample_peak: f32,
}

/// The loudness of one gating block's mean square, in LUFS.
fn lufs_of(energy: f64) -> f64 {
    -0.691 + 10.0 * energy.log10()
}

/// One biquad in direct form I, in double precision: K-weighting's high-pass
/// sits at 38 Hz, and single precision loses the bass at that corner.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn step(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// The K-weighting filter for any sample rate.
///
/// The standard gives coefficients for 48 kHz only. These are the analogue
/// prototypes they come from (as libebur128 and pyloudnorm derive them), put
/// back through the bilinear transform at the file's own rate; at 48 kHz they
/// reproduce the published numbers to eight places.
fn k_weighting(rate: f64) -> [Biquad; 2] {
    // Stage one: the head's high shelf, +4 dB above about 1.5 kHz.
    let f0 = 1_681.974_450_955_533;
    let gain = 3.999_843_853_973_347;
    let q = 0.707_175_236_955_419_6;
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let vh = 10f64.powf(gain / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [
            (vh + vb * k / q + k * k) / a0,
            2.0 * (k * k - vh) / a0,
            (vh - vb * k / q + k * k) / a0,
        ],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        x: [0.0; 2],
        y: [0.0; 2],
    };

    // Stage two: the RLB high-pass at 38 Hz.
    let f0 = 38.135_470_876_024_44;
    let q = 0.500_327_037_323_877_3;
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let high_pass = Biquad {
        b: [1.0, -2.0, 1.0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        x: [0.0; 2],
        y: [0.0; 2],
    };
    [shelf, high_pass]
}

/// How much each channel counts. Surrounds count 1.41 (+1.5 dB), the LFE not at
/// all; anything that is not 5.1 is treated as fronts.
fn weights(channels: usize) -> Vec<f64> {
    if channels == 6 {
        // L R C LFE Ls Rs, the order WAV, FLAC and the decoders all use.
        vec![1.0, 1.0, 1.0, 0.0, 1.41, 1.41]
    } else {
        vec![1.0; channels]
    }
}

/// ITU-R BS.1770-4 Annex 2's interpolation filter: 48 taps, as four phases of
/// twelve. Used at 4×, which is every rate under 96 kHz.
const ITU_PHASES: [[f32; 12]; 4] = [
    [
        0.001_708_984_4,
        0.010_986_328,
        -0.019_653_32,
        0.033_203_125,
        -0.059_448_242,
        0.137_329_1,
        0.972_167_97,
        -0.102_294_92,
        0.047_607_42,
        -0.026_611_328,
        0.014_892_578,
        -0.008_300_781,
    ],
    [
        -0.029_174_805,
        0.029_296_875,
        -0.051_757_812,
        0.089_111_33,
        -0.166_503_9,
        0.465_087_9,
        0.779_785_16,
        -0.200_317_38,
        0.101_562_5,
        -0.058_227_54,
        0.033_081_055,
        -0.018_920_898,
    ],
    [
        -0.018_920_898,
        0.033_081_055,
        -0.058_227_54,
        0.101_562_5,
        -0.200_317_38,
        0.779_785_16,
        0.465_087_9,
        -0.166_503_9,
        0.089_111_33,
        -0.051_757_812,
        0.029_296_875,
        -0.029_174_805,
    ],
    [
        -0.008_300_781,
        0.014_892_578,
        -0.026_611_328,
        0.047_607_42,
        -0.102_294_92,
        0.972_167_97,
        0.137_329_1,
        -0.059_448_242,
        0.033_203_125,
        -0.019_653_32,
        0.010_986_328,
        0.001_708_984_4,
    ],
];

/// Taps per phase of the filter designed for 2× (96 kHz and up).
const TAPS_2X: usize = 16;

/// A polyphase interpolator: one phase per output sample, so nothing is ever
/// multiplied by the zeros an upsampler inserts.
struct Oversampler {
    /// One coefficient set per interpolated sample; one set means no
    /// oversampling.
    phases: Vec<Vec<f32>>,
    /// The last input samples per channel, newest first, as many as a phase
    /// has taps.
    history: Vec<Vec<f32>>,
}

impl Oversampler {
    fn new(rate: u32, channels: usize) -> Oversampler {
        let phases: Vec<Vec<f32>> = if rate < 96_000 {
            ITU_PHASES.iter().map(|p| p.to_vec()).collect()
        } else if rate < 192_000 {
            designed_2x()
        } else {
            vec![vec![1.0]]
        };
        let taps = phases[0].len();
        Oversampler {
            phases,
            history: vec![vec![0f32; taps]; channels],
        }
    }

    /// Takes one sample on a channel and returns the largest magnitude among
    /// the samples it interpolates there.
    fn push(&mut self, channel: usize, x: f32) -> f32 {
        let h = &mut self.history[channel];
        let n = h.len();
        h.copy_within(0..n - 1, 1);
        h[0] = x;
        let mut peak = 0f32;
        for phase in &self.phases {
            let y: f32 = phase.iter().zip(h.iter()).map(|(c, s)| c * s).sum();
            peak = peak.max(y.abs());
        }
        peak
    }
}

/// A 2× interpolator for 96 kHz material, which the standard does not give:
/// a Kaiser-windowed sinc cut at the original Nyquist, each phase scaled to
/// pass DC at unity.
fn designed_2x() -> Vec<Vec<f32>> {
    let factor = 2;
    let len = TAPS_2X * factor;
    let centre = (len - 1) as f64 / 2.0;
    let beta = 7.0;
    let i0_beta = bessel_i0(beta);
    let taps: Vec<f64> = (0..len)
        .map(|n| {
            let t = (n as f64 - centre) / factor as f64;
            let sinc = if t.abs() < 1e-12 {
                1.0
            } else {
                (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
            };
            let r = (n as f64 - centre) / centre;
            sinc * bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0_beta
        })
        .collect();
    (0..factor)
        .map(|p| {
            let sum: f64 = (0..TAPS_2X).map(|k| taps[k * factor + p]).sum();
            (0..TAPS_2X)
                .map(|k| (taps[k * factor + p] / sum) as f32)
                .collect()
        })
        .collect()
}

/// The zeroth-order modified Bessel function, for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    for k in 1..50 {
        term *= (half / k as f64) * (half / k as f64);
        sum += term;
        if term < sum * 1e-16 {
            break;
        }
    }
    sum
}

/// Measures as interleaved samples arrive.
pub struct Meter {
    channels: usize,
    filters: Vec<[Biquad; 2]>,
    weights: Vec<f64>,
    /// Frames in a 100 ms step.
    step: usize,
    /// Weighted sum of squares in the step being filled.
    partial: f64,
    in_step: usize,
    /// The last four steps' sums, which together are one 400 ms block.
    recent: [f64; 4],
    filled: usize,
    /// Each complete block's mean square, weighted across channels.
    blocks: Vec<f64>,
    channel: usize,
    peak: f32,
    sample_peak: f32,
    over: Oversampler,
}

impl Meter {
    pub fn new(channels: usize, rate: u32) -> Meter {
        let channels = channels.max(1);
        Meter {
            channels,
            filters: vec![k_weighting(rate as f64); channels],
            weights: weights(channels),
            step: (rate as usize / 10).max(1),
            partial: 0.0,
            in_step: 0,
            recent: [0.0; 4],
            filled: 0,
            blocks: Vec::new(),
            channel: 0,
            peak: 0.0,
            sample_peak: 0.0,
            over: Oversampler::new(rate, channels),
        }
    }

    /// One sample, in interleaved order.
    pub fn push(&mut self, sample: f32) {
        let c = self.channel;
        self.peak = self.peak.max(self.over.push(c, sample));
        self.sample_peak = self.sample_peak.max(sample.abs());

        let [shelf, high_pass] = &mut self.filters[c];
        let y = high_pass.step(shelf.step(sample as f64));
        self.partial += self.weights[c] * y * y;

        self.channel += 1;
        if self.channel < self.channels {
            return;
        }
        self.channel = 0;
        self.in_step += 1;
        if self.in_step < self.step {
            return;
        }

        self.recent.rotate_left(1);
        self.recent[3] = self.partial;
        self.partial = 0.0;
        self.in_step = 0;
        self.filled += 1;
        if self.filled >= 4 {
            let sum: f64 = self.recent.iter().sum();
            self.blocks.push(sum / (4 * self.step) as f64);
        }
    }

    pub fn finish(self) -> Loudness {
        // Absolute gate, then relative: 10 LU under the mean of what passed.
        let loud: Vec<f64> = self
            .blocks
            .iter()
            .copied()
            .filter(|e| *e > 0.0 && lufs_of(*e) > -70.0)
            .collect();
        let lufs = if loud.is_empty() {
            None
        } else {
            let mean = loud.iter().sum::<f64>() / loud.len() as f64;
            let gate = lufs_of(mean) - 10.0;
            let kept: Vec<f64> = loud.into_iter().filter(|e| lufs_of(*e) > gate).collect();
            let mean = kept.iter().sum::<f64>() / kept.len() as f64;
            Some(lufs_of(mean) as f32)
        };
        Loudness {
            lufs,
            true_peak_db: (self.peak > 0.0).then(|| 20.0 * self.peak.log10()),
            sample_peak: self.sample_peak,
        }
    }
}

/// ReplayGain 2.0's track gain for a loudness: what brings it to -18 LUFS.
pub fn replay_gain_db(lufs: f32) -> f32 {
    -18.0 - lufs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(
        channels: usize,
        rate: u32,
        frames: usize,
        f: impl Fn(usize, usize) -> f32,
    ) -> Loudness {
        let mut m = Meter::new(channels, rate);
        for i in 0..frames {
            for c in 0..channels {
                m.push(f(i, c));
            }
        }
        m.finish()
    }

    fn sine(hz: f64, rate: u32, amp: f64) -> impl Fn(usize, usize) -> f32 {
        move |i, _| (amp * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin()) as f32
    }

    /// EBU Tech 3341, case 1: a stereo 1 kHz sine at -23 dBFS reads -23.0 LUFS.
    #[test]
    fn a_minus_23_sine_reads_minus_23() {
        for rate in [44_100, 48_000, 96_000] {
            let amp = 10f64.powf(-23.0 / 20.0);
            let l = measure(2, rate, rate as usize * 20, sine(1000.0, rate, amp));
            let lufs = l.lufs.unwrap();
            assert!((lufs + 23.0).abs() < 0.1, "{rate} Hz: {lufs}");
        }
    }

    /// EBU Tech 3341, case 2: -33 dBFS reads -33.0.
    #[test]
    fn a_minus_33_sine_reads_minus_33() {
        let amp = 10f64.powf(-33.0 / 20.0);
        let lufs = measure(2, 48_000, 48_000 * 20, sine(1000.0, 48_000, amp))
            .lufs
            .unwrap();
        assert!((lufs + 33.0).abs() < 0.1, "{lufs}");
    }

    /// EBU Tech 3341, case 3: 10 s at -36, 60 s at -23, 10 s at -36 dBFS. The
    /// relative gate drops the quiet ends, so it reads -23.0.
    #[test]
    fn the_relative_gate_ignores_quiet_ends() {
        let rate = 48_000;
        let quiet = 10f64.powf(-36.0 / 20.0);
        let loud = 10f64.powf(-23.0 / 20.0);
        let lufs = measure(2, rate, rate as usize * 80, |i, _| {
            let s = i / rate as usize;
            let amp = if (10..70).contains(&s) { loud } else { quiet };
            (amp * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / rate as f64).sin()) as f32
        })
        .lufs
        .unwrap();
        assert!((lufs + 23.0).abs() < 0.1, "{lufs}");
    }

    #[test]
    fn silence_has_no_loudness_and_no_peak() {
        let l = measure(2, 44_100, 44_100 * 3, |_, _| 0.0);
        assert_eq!(l, Loudness::default());
    }

    /// A sine at a quarter of the rate, sampled 45° off its crest, never
    /// shows its peak in a sample: every sample sits at 0.707. True peak finds
    /// the crest between them.
    #[test]
    fn true_peak_finds_the_crest_between_samples() {
        let rate = 44_100;
        let l = measure(1, rate, rate as usize, |i, _| {
            (std::f64::consts::FRAC_PI_2 * i as f64 + std::f64::consts::FRAC_PI_4).sin() as f32
        });
        let tp = l.true_peak_db.unwrap();
        // The samples alone say -3.0 dBFS.
        assert!(tp > -0.5 && tp < 0.3, "{tp}");
    }

    #[test]
    fn a_full_scale_low_tone_peaks_at_zero() {
        let tp = measure(2, 48_000, 48_000, sine(100.0, 48_000, 1.0))
            .true_peak_db
            .unwrap();
        assert!(tp.abs() < 0.1, "{tp}");
    }

    #[test]
    fn replay_gain_aims_at_minus_18() {
        assert_eq!(replay_gain_db(-9.0), -9.0);
        assert_eq!(replay_gain_db(-23.0), 5.0);
    }
}
