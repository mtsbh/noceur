//! A real-input FFT, and the window that goes in front of it.
//!
//! Written out rather than pulled in. This needs one transform of one size on
//! one thread, and every crate that offers it also offers planners, SIMD
//! backends and a complex-number type that would then appear in signatures all
//! over this crate. The radix-2 Cooley-Tukey below is thirty lines and is not
//! the slow part of anything here — decoding the file is.

use std::f32::consts::PI;

/// In-place complex FFT, decimation in time.
///
/// `re` and `im` are the same length, which must be a power of two. The result
/// replaces the input, in the usual bin order: bin `k` is `k * rate / n` Hz, and
/// only the first half is meaningful for real input.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert_eq!(n, im.len());
    debug_assert!(n.is_power_of_two(), "the transform length must be 2^k");
    if n < 2 {
        return;
    }

    // Bit-reversal permutation, so the butterflies below can run in place.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= n {
        let angle = -2.0 * PI / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());

        for start in (0..n).step_by(len) {
            // The twiddle factor is advanced by repeated multiplication rather
            // than recomputed with a sin/cos per butterfly. At this size the
            // drift is far below the noise floor of anything being measured.
            let (mut ur, mut ui) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * ur - im[b] * ui;
                let ti = re[b] * ui + im[b] * ur;

                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;

                let next = ur * wr - ui * wi;
                ui = ur * wi + ui * wr;
                ur = next;
            }
        }
        len <<= 1;
    }
}

/// A transform of one fixed size, with its twiddle factors worked out once.
///
/// [`fft`] advances its twiddles by repeated multiplication, which is fine at a
/// few thousand points. At tens of thousands the rounding adds up to something
/// a spectrum that is compared against a trained model can see, so large
/// transforms that run many times take theirs from a table computed in `f64`.
pub struct Plan {
    n: usize,
    /// `cos` and `sin` of `-2πk/n` for `k` in `0..n/2`.
    twiddles: Vec<(f32, f32)>,
}

impl Plan {
    /// A plan for transforms of length `n`, which must be a power of two.
    pub fn new(n: usize) -> Self {
        debug_assert!(n.is_power_of_two(), "the transform length must be 2^k");
        let twiddles = (0..n / 2)
            .map(|k| {
                let angle = -2.0 * std::f64::consts::PI * k as f64 / n as f64;
                (angle.cos() as f32, angle.sin() as f32)
            })
            .collect();
        Plan { n, twiddles }
    }

    /// In place, same conventions as [`fft`].
    pub fn run(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.n;
        debug_assert_eq!(re.len(), n);
        debug_assert_eq!(im.len(), n);
        if n < 2 {
            return;
        }
        let mut j = 0usize;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j |= bit;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let stride = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (wr, wi) = self.twiddles[k * stride];
                    let (a, b) = (start + k, start + k + len / 2);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len <<= 1;
        }
    }
}

/// A Hann window of `n` points.
///
/// Any window would do for finding where a spectrum stops; Hann is chosen for
/// its sidelobes, which fall away fast enough that a loud low frequency does not
/// smear across the top of the spectrum and hide the very cutoff being looked
/// for. A rectangular window leaks badly enough to make a brickwalled file look
/// like it has content up to Nyquist.
pub fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Magnitudes of the first half, which is all real input has to say.
    fn spectrum(samples: &[f32]) -> Vec<f32> {
        let mut re = samples.to_vec();
        let mut im = vec![0.0; samples.len()];
        fft(&mut re, &mut im);
        (0..samples.len() / 2)
            .map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt())
            .collect()
    }

    fn tone(n: usize, cycles: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * cycles * i as f32 / n as f32).sin())
            .collect()
    }

    /// A tone at an exact bin frequency must land in that bin and nowhere else.
    #[test]
    fn a_tone_lands_in_its_own_bin() {
        let mags = spectrum(&tone(1024, 64.0));
        let peak = mags
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(peak, 64);

        // And the neighbours are far below it, rather than a smear.
        assert!(mags[64] > mags[60] * 100.0, "{} vs {}", mags[64], mags[60]);
    }

    #[test]
    fn a_constant_signal_is_all_at_zero_hertz() {
        let mags = spectrum(&vec![1.0f32; 256]);
        assert!(mags[0] > 100.0);
        for m in &mags[1..] {
            assert!(*m < 0.01, "got {m}");
        }
    }

    #[test]
    fn silence_transforms_to_silence() {
        for m in spectrum(&vec![0.0f32; 256]) {
            assert_eq!(m, 0.0);
        }
    }

    /// Two tones must both be visible: the whole analysis rests on reading a
    /// spectrum with many components at once.
    #[test]
    fn two_tones_are_both_found() {
        let n = 1024;
        let a = tone(n, 32.0);
        let b = tone(n, 200.0);
        let mixed: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let mags = spectrum(&mixed);

        assert!(mags[32] > 100.0);
        assert!(mags[200] > 100.0);
        assert!(mags[120] < 1.0, "nothing between them");
    }

    #[test]
    fn the_window_is_zero_at_the_edges_and_one_in_the_middle() {
        let w = hann(64);
        assert!(w[0].abs() < 1e-6);
        assert!((w[32] - 1.0).abs() < 1e-6);
        assert_eq!(w.len(), 64);
    }

    /// The planned transform gives what the unplanned one does.
    #[test]
    fn a_plan_matches_the_plain_transform() {
        let signal: Vec<f32> = (0..1024)
            .map(|i| ((i * 7919) % 101) as f32 / 50.0 - 1.0)
            .collect();
        let (mut re1, mut im1) = (signal.clone(), vec![0.0; 1024]);
        fft(&mut re1, &mut im1);
        let (mut re2, mut im2) = (signal, vec![0.0; 1024]);
        Plan::new(1024).run(&mut re2, &mut im2);
        for i in 0..1024 {
            assert!(
                (re1[i] - re2[i]).abs() < 1e-2,
                "re {i}: {} vs {}",
                re1[i],
                re2[i]
            );
            assert!(
                (im1[i] - im2[i]).abs() < 1e-2,
                "im {i}: {} vs {}",
                im1[i],
                im2[i]
            );
        }
    }

    /// Parseval, near enough: the energy is the same on both sides.
    #[test]
    fn the_transform_conserves_energy() {
        let signal = tone(512, 40.0);
        let time: f32 = signal.iter().map(|s| s * s).sum();

        let mut re = signal.clone();
        let mut im = vec![0.0; 512];
        fft(&mut re, &mut im);
        let freq: f32 = re.iter().zip(&im).map(|(r, i)| r * r + i * i).sum::<f32>() / 512.0;

        assert!((time - freq).abs() / time < 1e-3, "{time} vs {freq}");
    }
}
