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

use crate::damage::{Damage, Inspector};
use crate::fft::{fft, hann};
use crate::loudness::{Loudness, Meter};
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

/// Level at which a sample counts as pinned to full scale.
///
/// Not exactly 1.0: a decoder that resamples, or one that converts from 16-bit
/// integers through a gain stage, lands a hair under it and would hide every
/// clipped sample in the file behind a rounding error.
const CLIP_LEVEL: f32 = 0.9985;

/// Consecutive full-scale samples before it counts as clipping.
///
/// A single sample touching full scale is a loud peak that happened to land
/// there, and a mastered track has thousands of them. Three in a row is a
/// waveform with its top cut off, which is a different thing and the thing worth
/// reporting.
const CLIP_RUN: u32 = 3;

/// Identical samples in a row, in one channel, before a flat top below full
/// scale counts.
///
/// Stricter than [`CLIP_RUN`]: below full scale nothing pins a sample, so equal
/// neighbours also come from the slow crest of a deep bass note, which at 16-bit
/// resolution repeats a value two or three times. Eight is a cut top.
const PLATEAU_RUN: u32 = 8;

/// How far under the file's own peak a flat top may sit and still be the one
/// the gain moved down. A few stray samples above it, from an edit or a
/// transient added later, should not hide it.
const PLATEAU_RANGE_DB: f32 = 1.0;

/// Resolution samples are compared at: 16-bit, so a top that went through a
/// 24-bit gain stage still reads as flat.
const STEPS: f32 = 32768.0;

/// Flat tops lower than this (−12 dBFS) are not tracked. Nobody normalises a
/// clipped master that far down, and it keeps the tally small.
const PLATEAU_FLOOR: u32 = 8192;

/// How much of a file is clipped.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Clipping {
    /// Portion of all samples inside a clipped run, as a percentage.
    pub percent: f32,
    /// Longest unbroken run of clipped samples, in samples.
    ///
    /// The more telling of the two: a track can clip on a thousand isolated
    /// transients and sound fine, while one run of two hundred samples is
    /// audible distortion.
    pub longest_run: u32,
    /// Where the flat tops sit, in dBFS: 0 for full scale, below it for a track
    /// that clipped and was then turned down.
    pub level_db: f32,
}

/// What the spectrum of a file looks like.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    pub sample_rate: u32,
    /// Average power per bin, in dB, relative to the loudest bin.
    pub bins_db: Vec<f32>,
    /// Frames that had enough signal in them to be worth measuring.
    pub frames: usize,
    /// Measured across every sample, not the sampled windows the spectrum uses.
    pub clipping: Clipping,
    /// Integrated loudness and true peak, from the same pass.
    pub loudness: Loudness,
    /// Whatever is broken, as opposed to lossy.
    pub damage: Damage,
}

/// Counts full-scale runs as samples arrive.
///
/// Per channel and on the raw samples rather than the downmix: averaging two
/// channels together pulls a clipped one back under full scale and hides exactly
/// what this is looking for.
struct ClipCounter {
    total: u64,
    clipped: u64,
    run: u32,
    longest: u32,
    plateaus: Plateaus,
}

/// Samples inside flat runs, and the longest, at one level.
#[derive(Default, Clone, Copy)]
struct Flat {
    samples: u64,
    longest: u32,
}

/// Runs of identical samples below full scale, by level.
///
/// A track that clipped and was then turned down keeps its flat tops, just
/// lower; where they sit is only known once the file's peak is, so every level
/// near the top is tallied and the one that matters picked at the end.
struct Plateaus {
    channels: usize,
    channel: usize,
    /// Per channel: the last sample, in steps, and how many times in a row.
    last: Vec<(i32, u32)>,
    peak: u32,
    flats: std::collections::HashMap<u32, Flat>,
}

impl Plateaus {
    fn new(channels: usize) -> Plateaus {
        let channels = channels.max(1);
        Plateaus {
            channels,
            channel: 0,
            last: vec![(i32::MIN, 0); channels],
            peak: 0,
            flats: std::collections::HashMap::new(),
        }
    }

    fn push(&mut self, sample: f32) {
        let step = (sample * STEPS).round() as i32;
        let (last, run) = &mut self.last[self.channel];
        self.channel = (self.channel + 1) % self.channels;
        if step == *last {
            *run += 1;
        } else {
            *last = step;
            *run = 1;
        }

        let level = step.unsigned_abs();
        self.peak = self.peak.max(level);
        if level < PLATEAU_FLOOR || *run < CLIP_RUN {
            return;
        }
        let flat = self.flats.entry(level).or_default();
        flat.samples += if *run == CLIP_RUN {
            u64::from(CLIP_RUN)
        } else {
            1
        };
        flat.longest = flat.longest.max(*run);
    }

    /// The highest flat top near the file's peak and below full scale, as
    /// (level in steps, samples in it, longest run).
    fn finish(&self) -> Option<(u32, u64, u32)> {
        let floor = self.peak as f32 * 10f32.powf(-PLATEAU_RANGE_DB / 20.0);
        let top = self
            .flats
            .iter()
            .filter(|(level, f)| f.longest >= PLATEAU_RUN && **level as f32 >= floor)
            .map(|(level, _)| *level)
            .max()?;
        if top as f32 >= CLIP_LEVEL * STEPS {
            return None;
        }
        // A step either side: dither or rounding can split one top in two.
        let (samples, longest) = (top - 1..=top + 1)
            .filter_map(|l| self.flats.get(&l))
            .fold((0, 0), |(s, l), f| (s + f.samples, l.max(f.longest)));
        Some((top, samples, longest))
    }
}

impl ClipCounter {
    fn new(channels: usize) -> ClipCounter {
        ClipCounter {
            total: 0,
            clipped: 0,
            run: 0,
            longest: 0,
            plateaus: Plateaus::new(channels),
        }
    }

    fn push(&mut self, sample: f32) {
        self.total += 1;
        self.plateaus.push(sample);
        if sample.abs() < CLIP_LEVEL {
            self.run = 0;
            return;
        }

        self.run += 1;
        self.longest = self.longest.max(self.run);
        // The run only becomes clipping once it is long enough, and the samples
        // that led up to it count too.
        match self.run.cmp(&CLIP_RUN) {
            std::cmp::Ordering::Less => {}
            std::cmp::Ordering::Equal => self.clipped += u64::from(CLIP_RUN),
            std::cmp::Ordering::Greater => self.clipped += 1,
        }
    }

    fn finish(self) -> Clipping {
        let percent = |n: u64| {
            if self.total == 0 {
                0.0
            } else {
                n as f32 / self.total as f32 * 100.0
            }
        };
        let full = Clipping {
            percent: percent(self.clipped),
            longest_run: self.longest,
            level_db: 0.0,
        };
        // Clipping at full scale is the answer whenever there is any to speak
        // of; a turned-down top is looked for only when there is not.
        if full.longest_run >= PLATEAU_RUN {
            return full;
        }
        match self.plateaus.finish() {
            Some((level, samples, longest)) => Clipping {
                percent: percent(samples),
                longest_run: longest,
                level_db: 20.0 * (level as f32 / STEPS).log10(),
            },
            None => full,
        }
    }
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

    /// The level of the flat shelf at the top of the spectrum, if there is one.
    ///
    /// This is the other half of the measurement, and the half a cliff detector
    /// misses. Not every lossy file leaves a cliff: a gentle encoder lowpass, or
    /// a resample after decoding, turns the wall into a slope three kilohertz
    /// wide. What survives either way is what lies *above* the slope — a region
    /// that is perfectly flat, because it holds dither and nothing else.
    ///
    /// Real music never does that. Music at 20 kHz varies with the music; a
    /// stretch of spectrum sitting within a few dB of itself across two and a
    /// half kilohertz is digital silence, and it means the content stopped
    /// somewhere below.
    pub fn shelf_db(&self) -> Option<f32> {
        let bands = self.bands();
        let nyquist = self.sample_rate as f32 / 2.0;
        // The last few hundred hertz are left out: the final bands sit against
        // the edge of the transform and misbehave there.
        let from = ((nyquist - 2_800.0) / BAND_HZ) as usize;
        let to = (((nyquist - 700.0) / BAND_HZ) as usize).min(bands.len());
        if to <= from + 3 {
            return None;
        }

        let region = &bands[from..to];
        let low = region.iter().cloned().fold(f32::MAX, f32::min);
        let high = region.iter().cloned().fold(f32::MIN, f32::max);

        // Three decibels across two kilohertz, after averaging a couple of
        // hundred frames. Dither holds that flat; music does not come close,
        // even where it is quiet — a first attempt allowed five and mistook the
        // gentle taper of a genuine full-band record for a floor, then reported
        // its content as ending at 14.6 kHz.
        if high - low < 3.0 {
            Some(mean(region))
        } else {
            None
        }
    }

    /// The highest frequency carrying signal above the file's own noise floor.
    ///
    /// Only meaningful when there is a shelf to measure against — without one,
    /// walking down from the top finds the taper rather than the end of the
    /// content, which is the mistake that made the first version of this report
    /// a cutoff somewhere in the midrange.
    pub fn rolloff_hz(&self) -> Option<f32> {
        let shelf = self.shelf_db()?;
        // And it has to be far below the music. A true dither floor sits sixty
        // decibels or more under the midrange; anything nearer is signal, however
        // flat it happens to look across one stretch.
        if self.reference_db() - shelf < 40.0 {
            return None;
        }

        let bands = self.bands();
        let nyquist = self.sample_rate as f32 / 2.0;
        let start = (((nyquist - 700.0) / BAND_HZ) as usize).min(bands.len());
        // Ten dB clear of the floor: enough to be signal rather than the ripple
        // in the dither, low enough to catch the tail of a gentle rolloff.
        let threshold = shelf + 10.0;

        (0..start)
            .rev()
            .find_map(|i| (bands[i] > threshold).then_some(i as f32 * BAND_HZ))
    }

    /// The highest frequency the file carries real signal at.
    ///
    /// A cliff when there is one, the end of a rolloff when there is not, and
    /// Nyquist when the spectrum simply keeps going.
    pub fn cutoff_hz(&self) -> f32 {
        self.wall_hz()
            .or_else(|| self.rolloff_hz())
            .unwrap_or(self.sample_rate as f32 / 2.0)
    }

    /// Whether the content ends somewhere below Nyquist, by either shape.
    pub fn ends_early(&self) -> bool {
        self.wall_hz().is_some() || self.rolloff_hz().is_some()
    }

    /// Whether the spectrum stops dead rather than sloping away.
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
    pub clipping: Clipping,
    pub loudness: Loudness,
    pub damage: Damage,
    pub verdict: Verdict,
    /// Why, in one line, for the interface to show as it is.
    pub reason: String,
}

/// Where each encoder setting stops, roughly.
///
/// Measured, not quoted: a genuine WAV encoded by LAME 3.100 at each CBR rate,
/// decoded back and run through [`Spectrum::cutoff_hz`], stopped at 16.2, 16.8,
/// 18.2, 18.8 and 19.6 kHz for 128, 160, 192, 256 and 320. Each boundary sits
/// between two neighbours. The old table was quoted from LAME's nominal lowpass
/// values, which sit a few hundred hertz above where the measurement lands, and
/// it called every 256 kbps source 192.
const CUTOFFS: &[(f32, u32)] = &[
    (16_500.0, 128),
    (17_500.0, 160),
    (18_500.0, 192),
    (19_200.0, 256),
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
    let ends_early = spectrum.ends_early();
    let nyquist = spectrum.sample_rate as f32 / 2.0;
    let implied = implied_kbps(cutoff);

    let mut report = Report {
        sample_rate: spectrum.sample_rate,
        claimed_kbps,
        cutoff_hz: cutoff,
        implied_kbps: implied,
        brickwalled,
        clipping: spectrum.clipping,
        loudness: spectrum.loudness,
        damage: spectrum.damage.clone(),
        verdict: Verdict::Unsure,
        reason: String::new(),
    };

    if !spectrum.usable() {
        report.reason = "too quiet to measure".into();
        return report;
    }

    // Content running to within a whisker of Nyquist cannot have been through a
    // lossy encoder at any useful bitrate.
    let reaches_top = cutoff > nyquist * 0.92;

    // Checked before anything else, because it explains a wall that sits above
    // every lossy cutoff — which would otherwise be reported as "too high for a
    // transcode, too low for lossless", the least useful thing to be told.
    if ends_early {
        if let Some(source) = upsampled_from(spectrum.sample_rate, cutoff) {
            report.verdict = Verdict::Upsampled;
            report.implied_kbps = None;
            report.reason = format!("upsampled from {:.1} kHz", source as f32 / 1000.0);
            return report;
        }
    }

    match claim {
        // ── A lossy file whose header would not say ───────────────────────
        Claim::Lossy(None) => {
            report.verdict = Verdict::Unsure;
            report.reason = match implied {
                Some(kbps) => format!("bitrate unknown · ~{kbps} kbps"),
                None => "bitrate unknown".into(),
            };
        }

        // ── Lossless ──────────────────────────────────────────────────────
        Claim::Lossless => {
            if reaches_top || !ends_early {
                report.verdict = Verdict::Genuine;
                report.reason = "full range".into();
            } else if cutoff < 19_000.0 {
                report.verdict = Verdict::Transcoded;
                report.reason = match implied {
                    Some(kbps) => format!("lossy source · ~{kbps} kbps"),
                    None => "lossy source".into(),
                };
            } else {
                report.verdict = Verdict::Unsure;
                // Where a 256 or 320 kbps source would stop, and where a steep
                // mastering lowpass sometimes does too. Named, not accused.
                report.reason = match implied {
                    Some(kbps) => format!("maybe lossy · ~{kbps} kbps"),
                    None => "maybe lossy".into(),
                };
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
                    Some(kbps) => format!("really ~{kbps} kbps"),
                    None => format!("below {claimed} kbps"),
                };
            } else {
                report.verdict = Verdict::Genuine;
                report.reason = "as claimed".into();
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

    let mut clips = ClipCounter::new(1);
    let mut meter = Meter::new(1, sample_rate);
    let mut damage = Inspector::new(1, sample_rate);
    for sample in mono {
        clips.push(*sample);
        meter.push(*sample);
        damage.push(*sample);
    }

    finish(
        power,
        frames,
        sample_rate,
        clips.finish(),
        meter.finish(),
        damage.finish(),
    )
}

/// Everything measured in the one pass over a file's samples.
struct Pass {
    channels: usize,
    rate: u32,
    step: usize,
    window: Vec<f32>,
    power: Vec<f64>,
    frames: usize,
    block: Vec<f32>,
    at: usize,
    sum: f32,
    in_frame: usize,
    clips: ClipCounter,
    meter: Meter,
    damage: Inspector,
}

impl Pass {
    fn new(channels: usize, rate: u32, total: usize) -> Pass {
        let channels = channels.max(1);
        Pass {
            channels,
            rate,
            // Spread the blocks across the file, but never so far apart that a
            // short track yields too few to say anything.
            step: (total / FRAMES.max(1)).max(FFT_SIZE),
            window: hann(FFT_SIZE),
            power: vec![0f64; FFT_SIZE / 2],
            frames: 0,
            block: Vec::with_capacity(FFT_SIZE),
            at: 0,
            sum: 0.0,
            in_frame: 0,
            clips: ClipCounter::new(channels),
            meter: Meter::new(channels, rate),
            damage: Inspector::new(channels, rate),
        }
    }

    fn push(&mut self, sample: f32) {
        // Every sample, unlike the spectrum, which is measured from windows
        // spread across the file. Clipping and damage are rare and local:
        // sampling for them would miss most of it.
        self.clips.push(sample);
        self.meter.push(sample);
        self.damage.push(sample);

        // Down-mixed as it arrives: the spectrum of the sum is what a listener
        // hears, and analysing one channel would miss anything panned away.
        self.sum += sample;
        self.in_frame += 1;
        if self.in_frame < self.channels {
            return;
        }
        let mono = self.sum / self.channels as f32;
        self.sum = 0.0;
        self.in_frame = 0;

        // Collecting a block, or waiting for the next one to start.
        if self.block.is_empty() && !self.at.is_multiple_of(self.step) {
            self.at += 1;
            return;
        }
        self.block.push(mono);
        self.at += 1;

        if self.block.len() == FFT_SIZE {
            accumulate(&self.block, &self.window, &mut self.power, &mut self.frames);
            self.block.clear();
        }
    }

    fn finish(self) -> Spectrum {
        finish(
            self.power,
            self.frames,
            self.rate,
            self.clips.finish(),
            self.meter.finish(),
            self.damage.finish(),
        )
    }
}

/// An opened file, in the shape symphonia reads from.
struct Media {
    reader: crate::source::Skipping<std::io::BufReader<std::fs::File>>,
    len: Option<u64>,
}

impl std::io::Read for Media {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }
}

impl std::io::Seek for Media {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.reader.seek(pos)
    }
}

impl symphonia::core::io::MediaSource for Media {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

/// Whether a codec's stated length can be off by encoder padding.
fn lossy(codec: symphonia::core::codecs::CodecType) -> bool {
    use symphonia::core::codecs::{CODEC_TYPE_AAC, CODEC_TYPE_OPUS, CODEC_TYPE_VORBIS};
    matches!(codec, CODEC_TYPE_AAC | CODEC_TYPE_OPUS | CODEC_TYPE_VORBIS)
}

/// Undecodable packets before giving up on a file: past this it is not a file
/// with damage in it but damage with a file around it.
const MAX_DECODE_ERRORS: u32 = 10_000;

/// Measures the spectrum of a file.
///
/// Streamed rather than buffered. A ten-minute lossless track is twenty-six
/// million samples, and holding it as floats to measure two hundred windows of
/// it would cost a hundred megabytes to look at less than one percent of them.
/// Blocks are taken at intervals across the file instead, and everything between
/// them is decoded and dropped.
///
/// Symphonia directly rather than through rodio, which is what playback uses:
/// rodio skips packets that will not decode without saying so, and ends early
/// on a broken stream as though it had reached the end. For playing a file that
/// is the right thing to do. For checking one it hides exactly what is being
/// looked for.
pub fn analyse(path: &Path) -> Result<Spectrum, AudioError> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{
        DecoderOptions, CODEC_TYPE_MP1, CODEC_TYPE_MP2, CODEC_TYPE_MP3, CODEC_TYPE_NULL,
    };
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let fail = |reason: String| AudioError::Decode {
        path: path.display().to_string(),
        reason,
    };

    let full = crate::noceur_extended(path);
    let opened = crate::source::open(&full).map_err(|source| AudioError::Open {
        path: path.display().to_string(),
        source,
    })?;
    let media = Media {
        reader: opened.reader,
        len: opened.byte_len,
    };
    let stream = MediaSourceStream::new(Box::new(media), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| fail(e.to_string()))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| fail("no audio track".into()))?;
    let track_id = track.id;
    let params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| fail(e.to_string()))?;

    let header_rate = params.sample_rate.unwrap_or(44_100);
    let total = params
        .n_frames
        .map(|n| n as usize)
        .filter(|n| *n > 0)
        .unwrap_or(header_rate as usize * 300);

    // The length the header states, when it is a statement rather than a
    // guess. FLAC and WAV write theirs down; an MP3 only does in a LAME/Info
    // tag, and without one symphonia estimates from the file size — which a
    // cut file shrinks along with it, and a VBR file gets wrong by a fifth.
    let mpeg = matches!(
        params.codec,
        CODEC_TYPE_MP3 | CODEC_TYPE_MP2 | CODEC_TYPE_MP1
    );
    let stated_frames = params
        .n_frames
        .filter(|n| *n > 0 && (!mpeg || params.delay.is_some()));

    let mut pass: Option<Pass> = None;
    let mut samples: Option<(SampleBuffer<f32>, u64)> = None;
    let mut errors = 0u32;
    // Where each one struck, in frames decoded before it.
    let mut struck: Vec<u64> = Vec::new();
    let mut frames = 0u64;
    let mut stopped = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // How every format says it has reached the end.
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(Error::DecodeError(_)) if errors < MAX_DECODE_ERRORS => {
                errors += 1;
                struck.push(frames);
                continue;
            }
            Err(e) => {
                stopped = Some(e.to_string());
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(Error::DecodeError(_)) if errors < MAX_DECODE_ERRORS => {
                errors += 1;
                struck.push(frames);
                continue;
            }
            Err(e) => {
                stopped = Some(e.to_string());
                break;
            }
        };
        if decoded.frames() == 0 {
            continue;
        }

        frames += decoded.frames() as u64;
        let spec = *decoded.spec();
        let capacity = decoded.capacity() as u64;
        if samples.as_ref().is_none_or(|(_, cap)| *cap < capacity) {
            samples = Some((SampleBuffer::new(capacity, spec), capacity));
        }
        let (buffer, _) = samples.as_mut().expect("just made");
        buffer.copy_interleaved_ref(decoded);

        let pass = pass.get_or_insert_with(|| Pass::new(spec.channels.count(), spec.rate, total));
        for sample in buffer.samples() {
            pass.push(*sample);
        }
    }

    let Some(pass) = pass else {
        return Err(fail(match stopped {
            Some(why) => why,
            None if errors > 0 => format!("no packet would decode ({errors} tried)"),
            None => "no audio in the file".into(),
        }));
    };

    let mut spectrum = pass.finish();
    let damage = &mut spectrum.damage;
    // Not in the last second. An MP3 commonly ends in a frame or two of
    // tag bytes or padding that happen to look like a frame header, and the
    // decoder trips over them after the music is over: across 100 downloaded
    // MP3s, every file with a single error had it there. Nothing is heard.
    // The same goes for the end the header states, when something has been
    // appended after it.
    let tail = stated_frames
        .map_or(frames, |stated| stated.min(frames))
        .saturating_sub(u64::from(spectrum.sample_rate));
    damage.decode_errors = struck.iter().filter(|at| **at < tail).count() as u32;
    damage.stopped = stopped;
    damage.stated_secs = stated_frames.map(|n| n as f32 / spectrum.sample_rate.max(1) as f32);
    damage.exact_length = !mpeg && !lossy(params.codec);
    Ok(spectrum)
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
fn finish(
    power: Vec<f64>,
    frames: usize,
    sample_rate: u32,
    clipping: Clipping,
    loudness: Loudness,
    damage: Damage,
) -> Spectrum {
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
        clipping,
        loudness,
        damage,
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

    /// Music that rolls away gently into a real dither floor, which is what a
    /// decoded lossy file looks like once something has resampled it: the cliff
    /// is gone and only the empty space above it survives.
    fn rolled_off(top_hz: f32) -> Vec<f32> {
        let mut re = noise(N);
        let mut im = vec![0f32; N];
        crate::fft::fft(&mut re, &mut im);

        let half = N / 2;
        for bin in 0..=half {
            let hz = bin as f32 * RATE as f32 / N as f32;
            let pink = if hz < 20.0 { 0.0 } else { (20.0 / hz).sqrt() };
            // Fifteen decibels per kilohertz above the corner, which is about
            // what a real decoded-and-resampled file shows: steep enough to
            // reach the floor within a couple of kilohertz, gentle enough that
            // no single band shows the step a cliff does.
            let slope = if hz > top_hz {
                10f32.powf(-1.5 * (hz - top_hz) / 1000.0)
            } else {
                1.0
            };
            let gain = pink * slope;
            re[bin] *= gain;
            im[bin] *= gain;
            if bin > 0 && bin < half {
                re[N - bin] *= gain;
                im[N - bin] *= gain;
            }
        }

        for v in im.iter_mut() {
            *v = -*v;
        }
        crate::fft::fft(&mut re, &mut im);
        let scale = 1.0 / N as f32;
        let signal: Vec<f32> = re.iter().map(|v| v * scale).collect();

        // Dither: flat, and far below the music. This is the floor the rolloff
        // is measured against, and without it there is nothing to measure.
        let peak = signal.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-9);
        let dither = noise(N);
        signal
            .iter()
            .zip(&dither)
            .map(|(s, d)| s + d * peak * 3e-3)
            .collect()
    }

    /// The bug this test exists for: a genuine full-band record tapers into its
    /// own quiet top end, and a floor-relative rule read that taper as a floor
    /// and reported the content as ending at 14.6 kHz. A taper has no floor, and
    /// inventing one is worse than finding nothing.
    #[test]
    fn a_tapering_spectrum_is_not_reported_as_ending() {
        let s = measured(&shaped(None));
        assert_eq!(s.rolloff_hz(), None, "a taper is not a floor");
        assert!(!s.ends_early());
    }

    /// The other half, and the case a cliff detector alone misses: a lossy file
    /// whose wall has been smoothed into a slope still leaves the empty space
    /// above it, and that space is flat in a way music never is.
    #[test]
    fn a_gentle_rolloff_into_a_floor_is_still_found() {
        let s = measured(&rolled_off(17_000.0));
        assert!(s.ends_early(), "it plainly ends");

        let found = s.rolloff_hz().expect("a rolloff point");
        assert!((found - 17_000.0).abs() < 1_500.0, "got {found}");
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

    // ── Clipping ──────────────────────────────────────────────────────────

    /// A mastered track touches full scale on thousands of transients and
    /// sounds perfect. Counting those as clipping would flag every loud record
    /// ever made.
    #[test]
    fn isolated_peaks_at_full_scale_are_not_clipping() {
        let mut samples = vec![0.2f32; 10_000];
        for i in (0..10_000).step_by(50) {
            samples[i] = 1.0;
        }
        let c = measured(&samples).clipping;
        assert_eq!(c.percent, 0.0);
        assert_eq!(c.longest_run, 1);
    }

    /// A run is a waveform with its top cut off, which is the thing worth
    /// reporting.
    #[test]
    fn a_flat_topped_waveform_is_clipping() {
        let mut samples = vec![0.2f32; 10_000];
        samples[100..140].fill(1.0);
        samples[500..520].fill(-1.0);

        let c = measured(&samples).clipping;
        assert_eq!(c.longest_run, 40, "the longest run, in samples");
        // Sixty of ten thousand samples.
        assert!((c.percent - 0.6).abs() < 0.01, "got {}", c.percent);
    }

    #[test]
    fn a_clean_file_reports_nothing() {
        let c = measured(&shaped(None)).clipping;
        assert_eq!(c.percent, 0.0);
    }

    /// Decoders land a hair under 1.0 after resampling or a gain stage, and an
    /// exact comparison would hide every clipped sample behind a rounding error.
    #[test]
    fn samples_just_under_full_scale_still_count() {
        let mut samples = vec![0.2f32; 1000];
        samples[10..30].fill(0.999);
        assert_eq!(measured(&samples).clipping.longest_run, 20);
    }

    /// Clipped, then normalised to −0.3 dB: the same flat tops, lower down.
    #[test]
    fn a_clipped_track_turned_down_is_still_clipping() {
        let gain = 10f32.powf(-0.3 / 20.0);
        let mut samples: Vec<f32> = (0..10_000)
            .map(|i| (i as f32 * 0.05).sin() * 1.4)
            .map(|s| s.clamp(-1.0, 1.0) * gain)
            .collect();
        // A higher stray sample does not hide the tops under it.
        samples[5] = 0.99;

        let c = measured(&samples).clipping;
        assert!((c.level_db + 0.3).abs() < 0.01, "got {}", c.level_db);
        assert!(c.longest_run >= PLATEAU_RUN, "got {}", c.longest_run);
        assert!(c.percent > 10.0, "got {}", c.percent);
    }

    /// The crest of a deep bass note repeats a 16-bit value a few times. That
    /// is a round top, not a cut one.
    #[test]
    fn a_slow_round_peak_is_not_a_flat_top() {
        let rate = 44_100.0;
        let samples: Vec<f32> = (0..44_100)
            .map(|i| (std::f32::consts::TAU * 30.0 * i as f32 / rate).sin() * 0.9)
            .collect();
        let c = measured(&samples).clipping;
        assert_eq!(c.level_db, 0.0);
        assert_eq!(c.percent, 0.0);
    }

    /// A flat top in one channel only is found in interleaved stereo too.
    #[test]
    fn a_flat_top_in_one_channel_is_found() {
        let mut c = ClipCounter::new(2);
        for i in 0..2000 {
            let left = if (100..140).contains(&i) { 0.8 } else { 0.3 };
            c.push(left);
            c.push((i as f32 * 0.37).sin() * 0.5);
        }
        let c = c.finish();
        assert_eq!(c.longest_run, 40);
        assert!((c.level_db - 20.0 * 0.8f32.log10()).abs() < 0.01);
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
        assert_eq!(implied_kbps(18_200.0), Some(192));
        assert_eq!(implied_kbps(18_800.0), Some(256));
        assert_eq!(implied_kbps(19_600.0), Some(320));
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
