//! Finding out what a file actually is, rather than what it says it is.
//!
//! Every lossy encoder throws away the top of the spectrum, and the higher the
//! bitrate the less it throws away. That leaves a signature: a brickwall, a
//! frequency above which there is *nothing at all* rather than merely little.
//! Real recordings do not have one — they taper.
//!
//! So the top of the spectrum answers two different questions depending on what
//! the file claims to be:
//!
//! - A **lossy** file that claims 320 kbps and stops dead at 16 kHz was 128 kbps
//!   before somebody re-encoded it. The header is the only thing that changed.
//! - A **lossless** file with a brickwall was never lossless. A FLAC that stops
//!   at 16 kHz is an MP3 in a different container, which is the expensive
//!   mistake when buying from a private tracker or an unfamiliar shop.
//!
//! What this cannot do, and does not pretend to: tell a transcode from a
//! recording that genuinely has no treble. A 1962 mono jazz record, a vinyl rip,
//! a track built entirely from bass — all of them stop early because the music
//! does. Those come back as [`Verdict::Unsure`] with the measurement attached,
//! because a wrong accusation about somebody's library is worse than no answer.

use crate::fft::{fft, hann};
use crate::AudioError;
use std::path::Path;

/// Transform size.
///
/// 4096 at 44.1 kHz is a bin every 10.8 Hz and a window of 93 ms — fine enough
/// to place a brickwall to within a bin, long enough that a kick drum does not
/// dominate the frame it lands in.
pub const FFT_SIZE: usize = 4096;

/// Frames to take across the file.
///
/// Spread over the whole track rather than taken from the start: intros are
/// quiet, quiet is unrepresentative, and a file whose first ten seconds are a
/// pad would be judged on the pad.
const FRAMES: usize = 240;

/// Width of the bands the spectrum is scanned in, in hertz.
///
/// Wide enough that single-bin noise does not look like an edge, narrow enough
/// that a wall is still a wall rather than a slope spread over several bands.
const BAND_HZ: f32 = 200.0;

/// What the spectrum of a file looks like.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    pub sample_rate: u32,
    /// Average power per bin, in dB, relative to the loudest bin.
    pub bins_db: Vec<f32>,
    /// Frames that had enough signal in them to be worth measuring.
    pub frames: usize,
}

impl Spectrum {
    /// Hertz at the centre of a bin.
    pub fn hz(&self, bin: usize) -> f32 {
        bin as f32 * self.sample_rate as f32 / FFT_SIZE as f32
    }

    /// The frequency a brickwall sits at, if there is one.
    ///
    /// Found as the steepest drop in the top half of the spectrum, rather than
    /// as "the highest bin above some level". That distinction is the whole
    /// method: real music tapers, so *every* level-based rule finds the taper
    /// instead of the wall and reports a cutoff somewhere down in the midrange.
    /// An encoder does not taper — it leaves a cliff — and a cliff is a local
    /// property that a taper never imitates however far it falls overall.
    pub fn wall_hz(&self) -> Option<f32> {
        let bands = self.bands();
        if bands.is_empty() {
            return None;
        }
        let per = BAND_HZ;
        let nyquist = self.sample_rate as f32 / 2.0;

        // Below this no encoder in circulation cuts, and the midrange has
        // legitimate dips that would otherwise be read as walls.
        let first = (8_000.0 / per) as usize;
        let last = ((nyquist - 600.0) / per) as usize;
        if last <= first || last >= bands.len() {
            return None;
        }

        let mut best = (0.0f32, 0usize);
        for i in first..last {
            let below = mean(&bands[i.saturating_sub(5)..=i]);
            let above = mean(&bands[i + 1..(i + 8).min(bands.len())]);
            let drop = below - above;
            if drop > best.0 {
                best = (drop, i);
            }
        }

        // A wall is tens of dB deep. Anything shallower is music.
        if best.0 > 25.0 {
            Some(best.1 as f32 * per)
        } else {
            None
        }
    }

    /// The highest frequency the file carries real signal at.
    ///
    /// The wall when there is one, and Nyquist when there is not — a spectrum
    /// that simply keeps going has no cutoff to report.
    pub fn cutoff_hz(&self) -> f32 {
        self.wall_hz().unwrap_or(self.sample_rate as f32 / 2.0)
    }

    /// Whether the spectrum stops dead rather than tapering.
    pub fn brickwalled(&self) -> bool {
        self.wall_hz().is_some()
    }

    /// The spectrum averaged into coarse bands.
    ///
    /// Single bins are far too noisy to difference against each other; a band a
    /// couple of hundred hertz wide is still narrower than any wall is sharp.
    fn bands(&self) -> Vec<f32> {
        let per = (BAND_HZ * FFT_SIZE as f32 / self.sample_rate as f32).round() as usize;
        if per == 0 {
            return Vec::new();
        }
        self.bins_db.chunks(per).map(mean).collect()
    }

    /// The level of the very top of the spectrum, in dB.
    fn floor_db(&self) -> f32 {
        let n = self.bins_db.len();
        let from = n - (n / 25).max(1);
        median(&self.bins_db[from..])
    }

    /// The level of the part of the spectrum music actually lives in.
    fn reference_db(&self) -> f32 {
        let low = self.bin_at(300.0);
        let high = self.bin_at(4000.0).min(self.bins_db.len());
        if low >= high {
            return 0.0;
        }
        median(&self.bins_db[low..high])
    }

    fn bin_at(&self, hz: f32) -> usize {
        ((hz * FFT_SIZE as f32) / self.sample_rate as f32).round() as usize
    }

    /// Whether there is enough here to say anything at all.
    ///
    /// A handful of frames, or a file with almost no spectral spread — a sine
    /// sweep, a test tone, a field recording of a hum — can produce a confident
    /// looking number that means nothing.
    pub fn usable(&self) -> bool {
        self.frames >= 8 && self.reference_db() > self.floor_db() + 6.0
    }
}

/// What the file turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The spectrum matches what the file claims.
    Genuine,
    /// Lossy, and encoded at a lower bitrate than it now claims.
    Upscaled,
    /// Lossless container, lossy history.
    Transcoded,
    /// Converted up from a lower sample rate, which adds no detail.
    Upsampled,
    /// Measured, but not confidently enough to accuse anyone.
    Unsure,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Genuine => "genuine",
            Verdict::Upscaled => "upscaled",
            Verdict::Transcoded => "transcoded",
            Verdict::Upsampled => "upsampled",
            Verdict::Unsure => "unsure",
        }
    }
}

/// What the container claims to be.
///
/// A bitrate that could not be read is not the same as no bitrate. A lossless
/// file has none because there is nothing to claim; a lossy file whose header
/// would not parse has one and it is unknown — and calling that second case
/// "lossless" would accuse an ordinary MP3 of being a transcode purely because
/// its tag was malformed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Lossless,
    Lossy(Option<u32>),
}

/// Everything measured about one file.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub sample_rate: u32,
    /// What the container says, in kbps. `None` for lossless.
    pub claimed_kbps: Option<u32>,
    /// The highest frequency carrying signal.
    pub cutoff_hz: f32,
    /// The bitrate that cutoff is typical of, when it is typical of one.
    pub implied_kbps: Option<u32>,
    pub brickwalled: bool,
    pub verdict: Verdict,
    /// Why, in one line, for the interface to show as it is.
    pub reason: String,
}

/// Where each encoder setting stops, roughly.
///
/// LAME's defaults, which is what nearly everything in circulation was made
/// with. The numbers are the low end of each band's usual range, so a file is
/// credited with the lower bitrate when it sits on a boundary — accusing a
/// genuine file is the expensive error here, not excusing a bad one.
const CUTOFFS: &[(f32, u32)] = &[
    (16_500.0, 128),
    (17_750.0, 160),
    (19_000.0, 192),
    (20_000.0, 256),
];

/// The bitrate a cutoff is typical of.
fn implied_kbps(cutoff_hz: f32) -> Option<u32> {
    // Above this nothing lossy reaches, so there is no bitrate to name.
    if cutoff_hz >= 20_500.0 {
        return None;
    }
    CUTOFFS
        .iter()
        .find(|(hz, _)| cutoff_hz < *hz)
        .map(|(_, kbps)| *kbps)
        .or(Some(320))
}

/// Judges a spectrum against what the container claims.
///
/// `claimed_kbps` is `None` for a lossless file, which changes the question
/// being asked rather than merely the numbers.
pub fn judge(spectrum: &Spectrum, claim: Claim) -> Report {
    let claimed_kbps = match claim {
        Claim::Lossy(kbps) => kbps,
        Claim::Lossless => None,
    };
    let cutoff = spectrum.cutoff_hz();
    let brickwalled = spectrum.brickwalled();
    let nyquist = spectrum.sample_rate as f32 / 2.0;
    let implied = implied_kbps(cutoff);

    let mut report = Report {
        sample_rate: spectrum.sample_rate,
        claimed_kbps,
        cutoff_hz: cutoff,
        implied_kbps: implied,
        brickwalled,
        verdict: Verdict::Unsure,
        reason: String::new(),
    };

    if !spectrum.usable() {
        report.reason = "too quiet or too plain to measure".into();
        return report;
    }

    // Content running to within a whisker of Nyquist cannot have been through a
    // lossy encoder at any useful bitrate.
    let reaches_top = cutoff > nyquist * 0.92;

    // Checked before anything else, because it explains a wall that sits above
    // every lossy cutoff — which would otherwise be reported as "too high for a
    // transcode, too low for lossless", the least useful thing to be told.
    if brickwalled {
        if let Some(source) = upsampled_from(spectrum.sample_rate, cutoff) {
            report.verdict = Verdict::Upsampled;
            report.implied_kbps = None;
            report.reason = format!(
                "stops at {:.1} kHz — resampled up from {:.1} kHz, no detail added",
                cutoff / 1000.0,
                source as f32 / 1000.0
            );
            return report;
        }
    }

    match claim {
        // ── A lossy file whose header would not say ───────────────────────
        Claim::Lossy(None) => {
            report.verdict = Verdict::Unsure;
            report.reason = match implied {
                Some(kbps) => format!(
                    "bitrate unreadable; stops at {:.1} kHz, which suggests {kbps} kbps",
                    cutoff / 1000.0
                ),
                None => format!("bitrate unreadable; content to {:.1} kHz", cutoff / 1000.0),
            };
        }

        // ── Lossless ──────────────────────────────────────────────────────
        Claim::Lossless => {
            if reaches_top || !brickwalled {
                report.verdict = Verdict::Genuine;
                report.reason = format!("content to {:.1} kHz, no cutoff", cutoff / 1000.0);
            } else if cutoff < 19_000.0 {
                report.verdict = Verdict::Transcoded;
                report.reason = match implied {
                    Some(kbps) => format!(
                        "hard cutoff at {:.1} kHz — was lossy, about {kbps} kbps",
                        cutoff / 1000.0
                    ),
                    None => format!("hard cutoff at {:.1} kHz", cutoff / 1000.0),
                };
            } else {
                report.verdict = Verdict::Unsure;
                report.reason = format!(
                    "cutoff at {:.1} kHz — high for a transcode, low for lossless",
                    cutoff / 1000.0
                );
            }
        }

        // ── Lossy ─────────────────────────────────────────────────────────
        Claim::Lossy(Some(claimed)) => {
            let expected = expected_cutoff(claimed);
            // Two full bands below what it claims. One band is within the
            // spread of encoders, moods and source material; two is not.
            if cutoff + 2_000.0 < expected {
                report.verdict = Verdict::Upscaled;
                report.reason = match implied {
                    Some(kbps) => format!(
                        "claims {claimed} kbps but stops at {:.1} kHz — encoded around {kbps} kbps",
                        cutoff / 1000.0
                    ),
                    None => format!(
                        "claims {claimed} kbps but stops at {:.1} kHz",
                        cutoff / 1000.0
                    ),
                };
            } else {
                report.verdict = Verdict::Genuine;
                report.reason = format!(
                    "{claimed} kbps, cutoff at {:.1} kHz as expected",
                    cutoff / 1000.0
                );
            }
        }
    }

    report
}

/// The sample rate a file appears to have been converted up from.
///
/// A 48 kHz file whose content stops dead at 21.4 kHz was 44.1 kHz before
/// somebody resampled it: that is where 44.1's own Nyquist sits, and no 48 kHz
/// recording has a reason to stop just short of 22.05. Resampling adds no
/// detail, so the file is larger than its content and nothing more.
///
/// Only 44.1 and above are considered as sources. 32 kHz would put a candidate
/// at 16 kHz, which is exactly where a 128 kbps MP3 stops — and calling that an
/// upsample rather than a low bitrate would be the wrong answer to the more
/// important question.
fn upsampled_from(sample_rate: u32, wall_hz: f32) -> Option<u32> {
    for source in [88_200u32, 48_000, 44_100] {
        if source >= sample_rate {
            continue;
        }
        let nyquist = source as f32 / 2.0;
        if wall_hz > nyquist * 0.95 && wall_hz <= nyquist + 300.0 {
            return Some(source);
        }
    }
    None
}

/// Where a genuine file at this bitrate should stop.
fn expected_cutoff(kbps: u32) -> f32 {
    match kbps {
        0..=112 => 15_000.0,
        113..=136 => 16_000.0,
        137..=168 => 17_000.0,
        169..=200 => 18_500.0,
        201..=264 => 19_500.0,
        _ => 20_500.0,
    }
}

/// Measures the spectrum of a block of mono samples.
///
/// Frames are spread across the whole signal and near-silent ones are skipped:
/// silence has no spectrum, and averaging it in would pull every bin down
/// together and make a quiet track look like a brickwalled one.
pub fn spectrum(mono: &[f32], sample_rate: u32) -> Spectrum {
    let window = hann(FFT_SIZE);
    let mut power = vec![0f64; FFT_SIZE / 2];
    let mut frames = 0usize;

    if mono.len() >= FFT_SIZE {
        let span = mono.len() - FFT_SIZE;
        let step = (span / FRAMES.max(1)).max(1);
        for start in (0..=span).step_by(step) {
            accumulate(
                &mono[start..start + FFT_SIZE],
                &window,
                &mut power,
                &mut frames,
            );
        }
    }

    finish(power, frames, sample_rate)
}

/// Measures the spectrum of a file.
///
/// Streamed rather than buffered. A ten-minute lossless track is twenty-six
/// million samples, and holding it as floats to measure two hundred windows of
/// it would cost a hundred megabytes to look at less than one percent of them.
/// Blocks are taken at intervals across the file instead, and everything between
/// them is decoded and dropped.
pub fn analyse(path: &Path) -> Result<Spectrum, AudioError> {
    let full = crate::noceur_extended(path);
    let opened = crate::source::open(&full).map_err(|source| AudioError::Open {
        path: path.display().to_string(),
        source,
    })?;
    let byte_len = opened.byte_len;

    let mut builder = rodio::Decoder::builder().with_data(opened.reader);
    if let Some(len) = byte_len {
        builder = builder.with_byte_len(len);
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(ext);
    }
    let decoder = builder.build().map_err(|e| AudioError::Decode {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    let channels = rodio::Source::channels(&decoder).get() as usize;
    let rate = rodio::Source::sample_rate(&decoder).get();
    let duration = rodio::Source::total_duration(&decoder);

    let total = duration
        .map(|d| (d.as_secs_f64() * rate as f64) as usize)
        .filter(|f| *f > 0)
        .unwrap_or(rate as usize * 300);
    // Spread the blocks across the file, but never so far apart that a short
    // track yields too few to say anything.
    let step = (total / FRAMES.max(1)).max(FFT_SIZE);

    let window = hann(FFT_SIZE);
    let mut power = vec![0f64; FFT_SIZE / 2];
    let mut frames = 0usize;

    let mut block: Vec<f32> = Vec::with_capacity(FFT_SIZE);
    let mut at = 0usize;
    let mut sum = 0f32;
    let mut in_frame = 0usize;

    for sample in decoder {
        // Down-mixed as it arrives: the spectrum of the sum is what a listener
        // hears, and analysing one channel would miss anything panned away.
        sum += sample;
        in_frame += 1;
        if in_frame < channels {
            continue;
        }
        let mono = sum / channels as f32;
        sum = 0.0;
        in_frame = 0;

        // Collecting a block, or waiting for the next one to start.
        if block.is_empty() && !at.is_multiple_of(step) {
            at += 1;
            continue;
        }
        block.push(mono);
        at += 1;

        if block.len() == FFT_SIZE {
            accumulate(&block, &window, &mut power, &mut frames);
            block.clear();
        }
    }

    Ok(finish(power, frames, rate))
}

/// Adds one block's power to the running total, unless it is silence.
fn accumulate(block: &[f32], window: &[f32], power: &mut [f64], frames: &mut usize) {
    let rms = (block.iter().map(|s| (s * s) as f64).sum::<f64>() / block.len() as f64).sqrt();
    if rms < 1e-4 {
        return;
    }

    let mut re: Vec<f32> = block.iter().zip(window).map(|(s, w)| s * w).collect();
    let mut im = vec![0f32; block.len()];
    fft(&mut re, &mut im);

    for (bin, p) in power.iter_mut().enumerate() {
        *p += (re[bin] * re[bin] + im[bin] * im[bin]) as f64;
    }
    *frames += 1;
}

/// Turns accumulated power into a spectrum in dB relative to its own peak.
fn finish(power: Vec<f64>, frames: usize, sample_rate: u32) -> Spectrum {
    let bins_db = if frames == 0 {
        vec![-200.0; FFT_SIZE / 2]
    } else {
        let mean: Vec<f64> = power.iter().map(|p| p / frames as f64).collect();
        let peak = mean.iter().cloned().fold(f64::MIN, f64::max).max(1e-30);
        mean.iter()
            .map(|p| (10.0 * (p / peak).max(1e-20).log10()) as f32)
            .collect()
    };

    Spectrum {
        sample_rate,
        bins_db,
        frames,
    }
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return -200.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

fn median(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted[sorted.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;
    /// A power of two, so the test signals can be built with the same transform
    /// the analysis uses, and long enough to give the analysis its frames.
    const N: usize = 1 << 16;

    /// Deterministic broadband noise. A fixed generator rather than a crate:
    /// these tests must give the same answer on every machine forever.
    fn noise(n: usize) -> Vec<f32> {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 40) as f32 / 8_388_608.0 - 1.0
            })
            .collect()
    }

    /// Noise shaped like music, optionally cut off dead at `top_hz`.
    ///
    /// Built in the frequency domain, because that is what the thing being
    /// detected does. A cascade of filters produces a slope, and a slope is
    /// exactly what a lossy encoder does *not* leave behind — it zeroes the
    /// bins. Testing against a gentle roll-off would be testing against a signal
    /// that never occurs.
    ///
    /// The shaping is pink, falling 3 dB per octave, which is roughly how real
    /// music sits. White noise would be flat to Nyquist and would make the top
    /// of the spectrum as loud as the middle, which no recording is.
    fn shaped(top_hz: Option<f32>) -> Vec<f32> {
        shaped_at(RATE, top_hz)
    }

    /// The same, for a file at some other sample rate. The rate decides what
    /// frequency each bin stands for, so a signal built for one rate and read at
    /// another has its wall in the wrong place.
    fn shaped_at(rate: u32, top_hz: Option<f32>) -> Vec<f32> {
        let mut re = noise(N);
        let mut im = vec![0f32; N];
        crate::fft::fft(&mut re, &mut im);

        let half = N / 2;
        for bin in 0..=half {
            let hz = bin as f32 * rate as f32 / N as f32;
            let pink = if hz < 20.0 { 0.0 } else { (20.0 / hz).sqrt() };
            let gain = match top_hz {
                Some(top) if hz > top => 0.0,
                _ => pink,
            };
            re[bin] *= gain;
            im[bin] *= gain;
            // The mirrored half gets the same real gain, which keeps the
            // spectrum conjugate-symmetric and the inverse transform real.
            if bin > 0 && bin < half {
                re[N - bin] *= gain;
                im[N - bin] *= gain;
            }
        }

        // Inverse transform, by conjugating either side of a forward one.
        for v in im.iter_mut() {
            *v = -*v;
        }
        crate::fft::fft(&mut re, &mut im);
        let scale = 1.0 / N as f32;
        re.iter().map(|v| v * scale).collect()
    }

    fn measured(samples: &[f32]) -> Spectrum {
        spectrum(samples, RATE)
    }

    #[test]
    fn music_shaped_noise_reaches_the_top_and_has_no_wall() {
        let s = measured(&shaped(None));
        assert!(s.usable(), "should be measurable");
        assert!(!s.brickwalled(), "nothing cut this");
        assert!(
            s.cutoff_hz() > 20_000.0,
            "content should reach the top, got {}",
            s.cutoff_hz()
        );
    }

    /// The measurement the whole feature rests on.
    #[test]
    fn a_cutoff_is_found_where_it_was_put() {
        for hz in [16_000.0, 18_000.0, 19_500.0] {
            let found = measured(&shaped(Some(hz))).cutoff_hz();
            assert!((found - hz).abs() < 800.0, "cut at {hz}, measured {found}");
        }
    }

    #[test]
    fn a_cut_signal_reads_as_brickwalled() {
        assert!(measured(&shaped(Some(16_000.0))).brickwalled());
    }

    // ── Lossless files ────────────────────────────────────────────────────

    #[test]
    fn a_real_lossless_file_is_left_alone() {
        let r = judge(&measured(&shaped(None)), Claim::Lossless);
        assert_eq!(r.verdict, Verdict::Genuine, "{}", r.reason);
    }

    /// The expensive mistake: an MP3 in a FLAC container.
    #[test]
    fn a_transcoded_lossless_file_is_caught() {
        let r = judge(&measured(&shaped(Some(16_000.0))), Claim::Lossless);
        assert_eq!(r.verdict, Verdict::Transcoded, "{}", r.reason);
        assert_eq!(r.implied_kbps, Some(128));
        assert!(r.reason.contains("128"));
    }

    // ── Lossy files ───────────────────────────────────────────────────────

    #[test]
    fn a_genuine_320_is_left_alone() {
        let r = judge(&measured(&shaped(Some(20_500.0))), Claim::Lossy(Some(320)));
        assert_eq!(r.verdict, Verdict::Genuine, "{}", r.reason);
    }

    #[test]
    fn a_genuine_128_is_left_alone() {
        let r = judge(&measured(&shaped(Some(16_000.0))), Claim::Lossy(Some(128)));
        assert_eq!(r.verdict, Verdict::Genuine, "{}", r.reason);
    }

    /// The one everybody wants: 128 re-encoded to 320.
    #[test]
    fn a_128_wearing_a_320_header_is_caught() {
        let r = judge(&measured(&shaped(Some(16_000.0))), Claim::Lossy(Some(320)));
        assert_eq!(r.verdict, Verdict::Upscaled, "{}", r.reason);
        assert!(r.reason.contains("128"), "should name the real bitrate");
    }

    #[test]
    fn a_192_wearing_a_320_header_is_caught() {
        let r = judge(&measured(&shaped(Some(18_500.0))), Claim::Lossy(Some(320)));
        assert_eq!(r.verdict, Verdict::Upscaled, "{}", r.reason);
    }

    /// A band of slack, because encoders, versions and source material all move
    /// this number a little. Accusing a genuine file is the worse error.
    #[test]
    fn a_file_slightly_below_its_band_is_not_accused() {
        let r = judge(&measured(&shaped(Some(19_500.0))), Claim::Lossy(Some(320)));
        assert_eq!(r.verdict, Verdict::Genuine, "{}", r.reason);
    }

    // ── Resampled up from a lower rate ────────────────────────────────────

    /// A 48 kHz file stopping just short of 22.05 kHz was 44.1 kHz before
    /// somebody resampled it. That is a real finding, and reporting it as "too
    /// high for a transcode, too low for lossless" was the least useful thing
    /// the analysis could have said about it.
    #[test]
    fn a_file_resampled_up_from_44_1_is_named_as_such() {
        // Built at 44.1 with content to its own Nyquist, then measured as
        // though the file were 48 kHz — which is exactly what resampling
        // produces: the old Nyquist sitting inside the new one.
        let samples = shaped_at(48_000, Some(21_400.0));
        let s = spectrum(&samples, 48_000);
        let r = judge(&s, Claim::Lossless);

        assert_eq!(r.verdict, Verdict::Upsampled, "{}", r.reason);
        assert!(r.reason.contains("44.1"), "{}", r.reason);
        assert_eq!(r.implied_kbps, None, "no bitrate to imply");
    }

    /// The candidate rates deliberately stop at 44.1. Allowing 32 kHz would put
    /// a candidate at 16 kHz, which is where a 128 kbps MP3 stops — and that is
    /// the more important answer.
    #[test]
    fn a_low_bitrate_mp3_is_not_called_an_upsample() {
        let r = judge(&measured(&shaped(Some(16_000.0))), Claim::Lossy(Some(320)));
        assert_eq!(r.verdict, Verdict::Upscaled, "{}", r.reason);
    }

    #[test]
    fn a_genuine_48k_file_is_left_alone() {
        let s = spectrum(&shaped_at(48_000, None), 48_000);
        let r = judge(&s, Claim::Lossless);
        assert_eq!(r.verdict, Verdict::Genuine, "{}", r.reason);
    }

    /// A 44.1 kHz file cannot have been resampled up from 44.1 kHz.
    #[test]
    fn a_file_at_the_source_rate_is_not_a_candidate() {
        assert_eq!(upsampled_from(44_100, 21_400.0), None);
        assert_eq!(upsampled_from(48_000, 21_400.0), Some(44_100));
        assert_eq!(upsampled_from(96_000, 23_500.0), Some(48_000));
        // Well below any standard Nyquist: that is an encoder, not a resample.
        assert_eq!(upsampled_from(48_000, 16_000.0), None);
    }

    // ── Refusing to guess ─────────────────────────────────────────────────

    #[test]
    fn silence_is_not_judged() {
        let r = judge(&measured(&vec![0.0; N]), Claim::Lossy(Some(320)));
        assert_eq!(r.verdict, Verdict::Unsure);
        assert!(r.reason.contains("measure"));
    }

    #[test]
    fn a_file_too_short_to_measure_is_not_judged() {
        let r = judge(&measured(&noise(1000)), Claim::Lossless);
        assert_eq!(r.verdict, Verdict::Unsure);
    }

    /// A pure tone has no spectrum worth reading, and must not be called a
    /// transcode because everything above 1 kHz happens to be empty.
    #[test]
    fn a_test_tone_is_not_called_a_transcode() {
        let tone: Vec<f32> = (0..N)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / RATE as f32).sin() * 0.5)
            .collect();
        let r = judge(&measured(&tone), Claim::Lossless);
        assert_ne!(r.verdict, Verdict::Transcoded, "{}", r.reason);
    }

    #[test]
    fn a_cutoff_maps_to_the_bitrate_it_suggests() {
        assert_eq!(implied_kbps(16_000.0), Some(128));
        assert_eq!(implied_kbps(17_000.0), Some(160));
        assert_eq!(implied_kbps(18_500.0), Some(192));
        assert_eq!(implied_kbps(19_500.0), Some(256));
        assert_eq!(implied_kbps(20_200.0), Some(320));
        assert_eq!(implied_kbps(21_000.0), None, "lossless territory");
    }

    #[test]
    fn every_verdict_has_a_name() {
        for v in [
            Verdict::Genuine,
            Verdict::Upscaled,
            Verdict::Transcoded,
            Verdict::Unsure,
        ] {
            assert!(!v.as_str().is_empty());
        }
    }
}
