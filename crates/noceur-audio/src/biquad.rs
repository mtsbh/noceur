//! Second-order IIR filters, for splitting audio into frequency bands.
//!
//! Hand-rolled rather than pulled in as a dependency: these are the two standard
//! RBJ cookbook forms, they are twenty lines, and the alternative is an FFT crate
//! and a windowing scheme to get an answer that is thrown away at eight bits of
//! precision. A filterbank also runs in one pass over the samples, which matters
//! when the whole point is to touch each sample of a track exactly once.

/// One biquad section in direct form I.
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

/// Butterworth Q: the flattest passband, which is what a band meant for looking
/// at rather than listening to wants.
const Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

impl Biquad {
    /// A 12dB/octave low-pass at `cutoff` Hz.
    pub fn low_pass(cutoff: f32, sample_rate: f32) -> Biquad {
        let (cos_w0, alpha) = shape(cutoff, sample_rate);
        let a0 = 1.0 + alpha;
        Biquad::new(
            (1.0 - cos_w0) / 2.0,
            1.0 - cos_w0,
            (1.0 - cos_w0) / 2.0,
            -2.0 * cos_w0,
            1.0 - alpha,
            a0,
        )
    }

    /// A 12dB/octave high-pass at `cutoff` Hz.
    pub fn high_pass(cutoff: f32, sample_rate: f32) -> Biquad {
        let (cos_w0, alpha) = shape(cutoff, sample_rate);
        let a0 = 1.0 + alpha;
        Biquad::new(
            (1.0 + cos_w0) / 2.0,
            -(1.0 + cos_w0),
            (1.0 + cos_w0) / 2.0,
            -2.0 * cos_w0,
            1.0 - alpha,
            a0,
        )
    }

    fn new(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32, a0: f32) -> Biquad {
        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Feeds one sample through and returns the filtered result.
    pub fn step(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// The two shared coefficients of both forms.
///
/// The cutoff is clamped below Nyquist: a filter designed above it produces
/// coefficients that oscillate rather than filter, and a file sampled at 8kHz
/// would otherwise turn the high band into noise.
fn shape(cutoff: f32, sample_rate: f32) -> (f32, f32) {
    let nyquist = sample_rate / 2.0;
    let cutoff = cutoff.clamp(1.0, nyquist * 0.99);
    let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
    (w0.cos(), w0.sin() / (2.0 * Q))
}

/// Splits a signal into low, mid and high, the way a DJ waveform is coloured.
///
/// The crossovers are the ones the DJ software conventions settle on: bass and
/// kick below 200Hz, the body of the music — vocals, melody, snare — in the
/// middle, hats and air above 2kHz. They do not need to be precise, because the
/// output is a colour rather than a measurement; they need to put a kick in the
/// low band and a hi-hat in the high one.
#[derive(Debug, Clone)]
pub struct Split {
    low: Biquad,
    mid_high_pass: Biquad,
    mid_low_pass: Biquad,
    high: Biquad,
}

/// Where the low band ends, in Hz.
pub const LOW_CROSSOVER: f32 = 200.0;
/// Where the high band begins, in Hz.
pub const HIGH_CROSSOVER: f32 = 2000.0;

impl Split {
    pub fn new(sample_rate: f32) -> Split {
        Split {
            low: Biquad::low_pass(LOW_CROSSOVER, sample_rate),
            // The mid band is the two crossovers cascaded, which is cheaper than
            // designing a band-pass and gives the same skirts as its neighbours.
            mid_high_pass: Biquad::high_pass(LOW_CROSSOVER, sample_rate),
            mid_low_pass: Biquad::low_pass(HIGH_CROSSOVER, sample_rate),
            high: Biquad::high_pass(HIGH_CROSSOVER, sample_rate),
        }
    }

    /// Feeds one mono sample through and returns `[low, mid, high]`.
    pub fn step(&mut self, x: f32) -> [f32; 3] {
        let low = self.low.step(x);
        let mid = self.mid_low_pass.step(self.mid_high_pass.step(x));
        let high = self.high.step(x);
        [low, mid, high]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 44_100.0;

    /// Runs a sine of the given frequency through a split and returns the RMS of
    /// each band, skipping the filters' settling time.
    fn bands_of(freq: f32) -> [f32; 3] {
        let mut split = Split::new(RATE);
        let frames = 44_100;
        let settle = 4_096;
        let mut sums = [0f64; 3];
        let mut counted = 0f64;

        for i in 0..frames {
            let t = i as f32 / RATE;
            let x = (2.0 * std::f32::consts::PI * freq * t).sin();
            let out = split.step(x);
            if i >= settle {
                for b in 0..3 {
                    sums[b] += (out[b] as f64) * (out[b] as f64);
                }
                counted += 1.0;
            }
        }
        [
            (sums[0] / counted).sqrt() as f32,
            (sums[1] / counted).sqrt() as f32,
            (sums[2] / counted).sqrt() as f32,
        ]
    }

    /// The whole point of the filterbank: a kick, a vocal and a hi-hat must land
    /// in different bands, because that is what the colour encodes.
    #[test]
    fn each_band_claims_the_frequencies_it_is_named_for() {
        for (freq, expected, label) in [
            (60.0, 0usize, "a kick"),
            (800.0, 1, "a vocal"),
            (8000.0, 2, "a hi-hat"),
        ] {
            let bands = bands_of(freq);
            let loudest = (0..3)
                .max_by(|a, b| bands[*a].total_cmp(&bands[*b]))
                .unwrap();
            assert_eq!(
                loudest, expected,
                "{label} at {freq}Hz should be loudest in band {expected}, got {bands:?}"
            );
        }
    }

    /// A band must not merely win, it must dominate — otherwise a kick tints the
    /// mids and every colour washes out towards grey.
    #[test]
    fn a_tone_well_inside_a_band_is_rejected_by_the_others() {
        let bands = bands_of(60.0);
        assert!(
            bands[0] > bands[1] * 8.0 && bands[0] > bands[2] * 8.0,
            "60Hz should be far louder in low than elsewhere, got {bands:?}"
        );

        let bands = bands_of(10_000.0);
        assert!(
            bands[2] > bands[0] * 8.0 && bands[2] > bands[1] * 8.0,
            "10kHz should be far louder in high than elsewhere, got {bands:?}"
        );
    }

    /// A tone in the middle of a band should come through at roughly its own
    /// level, so a colour reflects how loud the music is and not how much the
    /// filter took away.
    #[test]
    fn a_tone_inside_a_band_passes_at_close_to_full_level() {
        // A unit sine has an rms of 1/sqrt(2), about 0.707.
        let bands = bands_of(800.0);
        assert!(
            (0.6..=0.8).contains(&bands[1]),
            "800Hz should pass the mid band near unity, got {bands:?}"
        );
    }

    /// Filters designed above Nyquist do not filter, they ring. Low sample rates
    /// turn up in old rips and in sample libraries.
    #[test]
    fn a_low_sample_rate_does_not_destabilise_the_high_band() {
        let mut split = Split::new(8_000.0);
        let mut worst = 0f32;
        for i in 0..8_000 {
            let t = i as f32 / 8_000.0;
            let x = (2.0 * std::f32::consts::PI * 440.0 * t).sin();
            for v in split.step(x) {
                worst = worst.max(v.abs());
            }
        }
        assert!(
            worst.is_finite() && worst < 4.0,
            "an 8kHz file must not blow up the filterbank, peaked at {worst}"
        );
    }
}
