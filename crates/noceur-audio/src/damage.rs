//! Files that are broken, as opposed to files that are lossy.
//!
//! A transcode is a file that was always going to sound like what it is. A
//! damaged file is one that went wrong after it was made: a download that
//! stopped early, a disk sector that came back as zeros, a packet the decoder
//! could not read. These are worth catching before the track is played out,
//! because the first time anyone hears the gap is usually on a dance floor.
//!
//! Everything here is held to the same rule as the transcode check: a wrong
//! accusation is worse than no answer. Electronic music is full of things that
//! look like damage to a naive detector — hard stops, saw waves with a
//! discontinuity every cycle, bitcrushed hats — and each test below says which
//! of them it was written to leave alone.

/// Below this a sample counts as digital silence.
///
/// Half a 16-bit step: anything smaller rounds to zero at 16 bits, so a decoder
/// that converts through floats and lands a hair off zero still reads as zero.
const SILENT: f32 = 1.0 / 65_536.0;

/// Shortest run of silence that counts as a dropout, in seconds.
///
/// A waveform crossing zero sits there for one sample. Two milliseconds is
/// audible as a tick and far longer than any crossing.
const DROPOUT_MIN_SECS: f32 = 0.002;

/// How loud the music must be right up against a gap, on both sides, for the
/// gap to be a hole rather than a stop.
const DROPOUT_EDGE: f32 = 0.02;

/// And how much of its level it must still have there.
///
/// The test that separates a hole from a stop. A break written into the music
/// — the beat stops for a bar, then the drop — is edited in a DAW, and DAWs
/// fade clip edges by a few milliseconds by default, so the music arrives at
/// zero rather than falling off a cliff into it. Missing samples are cut
/// wherever the music happened to be.
const DROPOUT_SHARP: f32 = 0.5;

/// Audio that must follow a gap, in seconds, for the gap to be in the music.
///
/// Symphonia's AIFF reader runs on past the sound into an `ID3 ` chunk and
/// decodes its header as a few frames of audio, so the end of a tagged AIFF is
/// silence, then the letters "ID" at -5 dB. That is a gap with hard edges at
/// both ends, in every such file, and nothing to do with the recording.
const DROPOUT_AFTER_SECS: f32 = 0.1;

/// A click must stand this far above the waveform's own roughness.
const CLICK_RATIO: f32 = 25.0;

/// And this far above silence, absolutely: a spike in a near-silent passage
/// below this is not something anyone hears.
const CLICK_FLOOR: f32 = 0.03;

/// How much sharper a click is than the samples either side of it.
///
/// An impulse stands out from both neighbours; a step — a saw wave's reset, the
/// edge of a clipped top — stands out from neither, because it is a change of
/// level rather than a spike.
const CLICK_SHARPNESS: f32 = 1.6;

/// And far sharper than the samples two away, which a single bad sample
/// leaves alone. A short percussive tick — a rimshot, a synth click — rings for
/// a handful of samples and moves them all.
const CLICK_ALONE: f32 = 4.0;

/// Two clicks of the same sign within this factor of each other's size are the
/// same sound played twice. Generous, because a drum's level moves with the
/// velocity it was programmed at and the mix around it — and losing one of two
/// real clicks still leaves the file named, where a false one names a good file.
const CLICK_TWIN: f32 = 2.0;

/// A click must be at least this large to be reported.
///
/// Smaller ones are still collected, so that a drum whose spike is usually
/// small does not leave its one loud hit standing alone: but a lone -30 dB
/// tick is not what a damaged file sounds like.
const CLICK_REPORT: f32 = 0.1;

/// More clicks than this in one channel is the sound of the track, not damage,
/// and the comparison between them is quadratic.
const MAX_CLICKS: usize = 2_000;

/// How long after a click the waveform must be back to its usual roughness.
const CLICK_SETTLE_SECS: f32 = 0.002;

/// And how close to it: within this multiple of the roughness before.
const CLICK_SETTLE: f32 = 2.0;

/// Clicks closer together than this, in seconds, are part of the sound.
///
/// Damage is sporadic. A click that repeats every few milliseconds is a pulse
/// wave or a bitcrusher, and the music is meant to sound like that.
const CLICK_ISOLATION_SECS: f32 = 0.05;

/// What was wrong with a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Damage {
    /// Packets the decoder could not read and skipped.
    pub decode_errors: u32,
    /// Why decoding stopped before the end of the file, when it did.
    pub stopped: Option<String>,
    /// Length the header states, in seconds, when it states one worth trusting.
    pub stated_secs: Option<f32>,
    /// The stated length is exact to the sample, as a lossless container's is.
    pub exact_length: bool,
    /// Length that actually decoded, in seconds.
    pub decoded_secs: f32,
    /// Gaps of digital silence that start or end mid-waveform.
    pub dropouts: u32,
    /// The longest of them, in milliseconds.
    pub longest_dropout_ms: f32,
    /// Isolated single-sample spikes, in whichever channel had most.
    pub clicks: u32,
    /// Bits the samples actually use, measured at 24-bit resolution.
    ///
    /// 16 for a file that was 16-bit before somebody wrote it out as 24.
    /// `None` when there was no signal to measure.
    pub used_bits: Option<u32>,
}

impl Damage {
    /// Seconds of audio the header promises and the file does not hold, when
    /// it is more than the format's own slack.
    ///
    /// Usually a download that stopped early, but not always from the end: a
    /// FLAC decoder that meets a corrupt frame resynchronises past it without
    /// complaint, and the only trace left is a track a little shorter than its
    /// header says.
    pub fn missing_secs(&self) -> Option<f32> {
        let stated = self.stated_secs?;
        let missing = stated - self.decoded_secs;
        // A lossless header counts samples, so ten milliseconds is already
        // something gone. An MP3's counts whole frames, and encoders pad: half
        // a second, or one percent of a long track, either way is ordinary.
        let slack = if self.exact_length {
            0.01
        } else {
            0.5_f32.max(stated * 0.01)
        };
        (missing > slack).then_some(missing)
    }

    /// Whether a file claiming `claimed_bits` really carries fewer.
    pub fn padded(&self, claimed_bits: Option<u8>) -> bool {
        claimed_bits == Some(24) && self.used_bits.is_some_and(|b| b <= 16)
    }

    /// A short description of everything found, worst first; empty when the
    /// file is sound.
    pub fn summary(&self, claimed_bits: Option<u8>) -> String {
        let mut parts = Vec::new();
        if let Some(missing) = self.missing_secs() {
            parts.push(format!("{} missing", clock(missing)));
        } else if let Some(why) = &self.stopped {
            parts.push(format!("stops at {}: {why}", clock(self.decoded_secs)));
        }
        if self.decode_errors > 0 {
            parts.push(plural(self.decode_errors, "decode error", "decode errors"));
        }
        if self.dropouts > 0 {
            parts.push(plural(self.dropouts, "dropout", "dropouts"));
        }
        if self.clicks > 0 {
            parts.push(plural(self.clicks, "click", "clicks"));
        }
        if self.padded(claimed_bits) {
            parts.push("16-bit in 24".into());
        }
        parts.join(" · ")
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// `0:42`, `1:05:00`, or `0.8 s` / `40 ms` for anything under two seconds.
fn clock(secs: f32) -> String {
    if secs < 0.1 {
        return format!("{:.0} ms", secs * 1000.0);
    }
    if secs < 2.0 {
        return format!("{secs:.1} s");
    }
    let s = secs.round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Click detection for one channel.
///
/// Works on how far each sample sits from the midpoint of its neighbours. Real
/// audio at 44.1 kHz is smooth on that scale, even a loud mix: its energy is in
/// the bottom few kilohertz. A click is one sample that is nowhere near where
/// its neighbours say it should be.
struct Clicks {
    /// The last three samples, oldest first.
    x: [f32; 3],
    /// Deviation of the last five judged samples from their neighbours'
    /// midpoint, oldest first.
    d: [f32; 5],
    /// Running mean of the deviation: how rough this waveform normally is.
    rough: f32,
    seen: u64,
    isolation: u64,
    /// Clicks that passed every test on their own, with their size.
    found: Vec<(u64, f32)>,
    /// A candidate being followed, to see whether the sound after it settles.
    probe: Option<Probe>,
    window: u32,
}

struct Probe {
    at: u64,
    /// The spike's deviation, signed: its fingerprint, for finding repeats.
    size: f32,
    /// How rough the waveform was before it.
    rough: f32,
    /// Roughness since.
    sum: f32,
    n: u32,
}

impl Clicks {
    fn new(rate: u32) -> Clicks {
        Clicks {
            x: [0.0; 3],
            d: [0.0; 5],
            rough: 0.0,
            seen: 0,
            found: Vec::new(),
            isolation: (rate as f32 * CLICK_ISOLATION_SECS) as u64,
            probe: None,
            window: ((rate as f32 * CLICK_SETTLE_SECS) as u32).max(8),
        }
    }

    fn push(&mut self, sample: f32) {
        self.x = [self.x[1], self.x[2], sample];
        self.seen += 1;
        if self.seen < 3 {
            return;
        }
        // Deviation of the middle sample, which now has both neighbours.
        let next = self.x[1] - (self.x[0] + self.x[2]) * 0.5;
        self.d = [self.d[1], self.d[2], self.d[3], self.d[4], next];
        if self.seen < 5 {
            return;
        }
        // The sample in the middle of the five deviations is judged: it now has
        // two either side.
        let at = self.seen - 4;
        let [far_before, before, mid, after, far_after] = self.d;
        let mag = mid.abs();
        if self.seen > 64
            && mag > CLICK_FLOOR
            && mag > self.rough * CLICK_RATIO
            && mag > before.abs().max(after.abs()) * CLICK_SHARPNESS
            && mag > far_before.abs().max(far_after.abs()) * CLICK_ALONE
        {
            if self.probe.is_none() {
                self.probe = Some(Probe {
                    at,
                    size: mid,
                    rough: self.rough,
                    sum: 0.0,
                    n: 0,
                });
            }
        } else if self.x.iter().any(|v| v.abs() >= SILENT) {
            // Only ordinary samples teach it what ordinary is: a click averaged
            // in would raise the bar for the next one, and a gap of silence
            // would lower it until the music coming back looked like clicks.
            self.rough += (mag - self.rough) / 512.0;
        }
        self.settle(at, mag);
    }

    /// Follows a candidate for a couple of milliseconds to see whether the
    /// waveform goes straight back to how it was.
    ///
    /// The test that tells a click from a drum. Both start with one sample
    /// nowhere near its neighbours, but a hi-hat or a clap goes on ringing for
    /// tens of milliseconds and a click is over at once.
    fn settle(&mut self, at: u64, mag: f32) {
        let Some(probe) = &mut self.probe else {
            return;
        };
        // The sample right after the spike is still part of its shape.
        if at <= probe.at + 1 {
            return;
        }
        probe.sum += mag;
        probe.n += 1;
        if probe.n < self.window {
            return;
        }
        let probe = self.probe.take().expect("just used");
        let settled = probe.sum / (probe.n as f32) < probe.rough.max(1e-6) * CLICK_SETTLE;
        if settled && self.found.len() < MAX_CLICKS {
            self.found.push((probe.at, probe.size));
        }
    }

    /// Clicks in this channel that are damage rather than part of the music.
    ///
    /// Two last tests, both against the other clicks found. Damage is
    /// sporadic, so a click with another a few milliseconds away in the same
    /// channel is a pulse wave or a bitcrusher. And damage is random, so a
    /// click with a twin of the same size and sign anywhere else in the track,
    /// in either channel, is a sample with a spike in it, playing every time
    /// the pattern comes round. The same moment in the other channel is the
    /// same click, not a twin.
    fn count(&self, every: &[&Clicks]) -> u32 {
        let twin = |at: u64, size: f32| {
            every
                .iter()
                .flat_map(|c| c.found.iter())
                .any(|(other_at, other)| {
                    at.abs_diff(*other_at) >= self.isolation
                        && size.signum() == other.signum()
                        && size.abs().max(other.abs()) <= size.abs().min(other.abs()) * CLICK_TWIN
                })
        };
        let crowded = |i: usize, at: u64| {
            self.found
                .iter()
                .enumerate()
                .any(|(j, (other_at, _))| i != j && at.abs_diff(*other_at) < self.isolation)
        };
        self.found
            .iter()
            .enumerate()
            .filter(|(i, (at, size))| {
                size.abs() >= CLICK_REPORT && !crowded(*i, *at) && !twin(*at, *size)
            })
            .count() as u32
    }
}

/// Dropout detection, a frame at a time.
///
/// Each edge of a gap is judged on the millisecond next to it against the five
/// around that. Missing samples leave the music at full level right up to the
/// hole; a fade leaves the last millisecond a fraction of the ones before it.
/// A single sample is not enough to go on: the hole can land on a zero
/// crossing.
struct Gaps {
    rate: u32,
    short: usize,
    long: usize,
    min_run: u64,
    after: u64,
    /// Levels of the last `long` frames, newest last.
    recent: std::collections::VecDeque<f32>,
    /// Frames of silence so far; `None` before anything has played.
    run: Option<u64>,
    /// Whether the music was cut off at the start of the current gap.
    lead_abrupt: bool,
    /// A gap that has ended, and the levels after it so far.
    trail: Option<(u64, Vec<f32>)>,
    /// A gap judged a dropout, waiting for the audio after it to prove it was
    /// in the music: the frame by which it must have.
    pending: Option<(u64, u64)>,
    frames: u64,
    dropouts: u32,
    longest: u64,
}

impl Gaps {
    fn new(rate: u32) -> Gaps {
        let ms = (rate as usize / 1000).max(1);
        Gaps {
            rate,
            short: ms,
            long: ms * 5,
            min_run: ((rate as f32 * DROPOUT_MIN_SECS) as u64).max(2),
            after: (rate as f32 * DROPOUT_AFTER_SECS) as u64,
            recent: std::collections::VecDeque::with_capacity(ms * 5),
            run: None,
            lead_abrupt: false,
            trail: None,
            pending: None,
            frames: 0,
            dropouts: 0,
            longest: 0,
        }
    }

    /// Whether levels, oldest first, show the music still at full level at
    /// their newest end (`at_end`) or their oldest.
    fn abrupt(&self, levels: &[f32], at_end: bool) -> bool {
        let near = if at_end {
            &levels[levels.len().saturating_sub(self.short)..]
        } else {
            &levels[..self.short.min(levels.len())]
        };
        let near = near.iter().cloned().fold(0.0, f32::max);
        let all = levels.iter().cloned().fold(0.0, f32::max);
        near >= DROPOUT_EDGE && near >= all * DROPOUT_SHARP
    }

    fn frame(&mut self, level: f32, silent: bool) {
        self.frames += 1;

        if let Some((_, levels)) = &mut self.trail {
            levels.push(level);
            if levels.len() >= self.long {
                let (run, levels) = self.trail.take().expect("just used");
                if self.abrupt(&levels, false) {
                    self.commit();
                    self.pending = Some((run, self.frames + self.after));
                }
            }
        }

        if silent {
            if let Some(run) = self.run {
                if run == 0 {
                    let levels: Vec<f32> = self.recent.iter().cloned().collect();
                    self.lead_abrupt = self.abrupt(&levels, true);
                }
                self.run = Some(run + 1);
            }
        } else {
            if let Some(run) = self.run {
                if run >= self.min_run && self.lead_abrupt {
                    let mut levels = Vec::with_capacity(self.long);
                    levels.push(level);
                    self.trail = Some((run, levels));
                }
            }
            self.run = Some(0);
        }

        if self.recent.len() == self.long {
            self.recent.pop_front();
        }
        self.recent.push_back(level);

        if self.pending.is_some_and(|(_, at)| self.frames >= at) {
            self.commit();
        }
    }

    fn commit(&mut self) {
        if let Some((run, _)) = self.pending.take() {
            self.dropouts += 1;
            self.longest = self.longest.max(run);
        }
    }

    /// Count, and the longest in milliseconds. A gap still open at the end of
    /// the file is a fade to silence, or a cut the length check names better.
    fn finish(&self) -> (u32, f32) {
        (
            self.dropouts,
            self.longest as f32 * 1000.0 / self.rate.max(1) as f32,
        )
    }
}

/// Looks for damage in samples as they are decoded, interleaved.
pub struct Inspector {
    channels: usize,
    rate: u32,
    clicks: Vec<Clicks>,
    gaps: Gaps,
    in_frame: usize,
    frame_silent: bool,
    frame_level: f32,
    /// Every sample's value at 24-bit resolution, ORed together.
    bits: u32,
}

impl Inspector {
    pub fn new(channels: usize, rate: u32) -> Inspector {
        let channels = channels.max(1);
        Inspector {
            channels,
            rate,
            clicks: (0..channels).map(|_| Clicks::new(rate)).collect(),
            gaps: Gaps::new(rate),
            in_frame: 0,
            frame_silent: true,
            frame_level: 0.0,
            bits: 0,
        }
    }

    pub fn push(&mut self, sample: f32) {
        self.clicks[self.in_frame].push(sample);

        // Exact for anything decoded from 24 bits or fewer: the integer the file
        // stored, scaled into the top of a 24-bit word.
        let scaled = (sample * 8_388_608.0).round() as i32;
        self.bits |= scaled as u32;

        // A frame is silent only when every channel is: one quiet channel is a
        // mix decision.
        let level = sample.abs();
        if level >= SILENT {
            self.frame_silent = false;
        }
        self.frame_level = self.frame_level.max(level);

        self.in_frame += 1;
        if self.in_frame == self.channels {
            self.in_frame = 0;
            let level = std::mem::take(&mut self.frame_level);
            let silent = std::mem::replace(&mut self.frame_silent, true);
            self.gaps.frame(level, silent);
        }
    }

    /// Everything the samples showed. Decoder-level findings are the caller's
    /// to fill in, since only the decoder sees them.
    pub fn finish(self) -> Damage {
        let (dropouts, longest_dropout_ms) = self.gaps.finish();
        let used_bits = (self.bits != 0).then(|| 24u32.saturating_sub(self.bits.trailing_zeros()));
        Damage {
            decoded_secs: self.gaps.frames as f32 / self.rate.max(1) as f32,
            dropouts,
            longest_dropout_ms,
            clicks: {
                let every: Vec<&Clicks> = self.clicks.iter().collect();
                self.clicks
                    .iter()
                    .map(|c| c.count(&every))
                    .max()
                    .unwrap_or(0)
            },
            used_bits,
            ..Damage::default()
        }
    }
}

/// Inspects a block of interleaved samples at once.
pub fn inspect(samples: &[f32], channels: usize, rate: u32) -> Damage {
    let mut inspector = Inspector::new(channels, rate);
    for s in samples {
        inspector.push(*s);
    }
    inspector.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// Something like music: a few partials and a little noise, loud enough
    /// that its roughness is realistic.
    fn music(secs: f32) -> Vec<f32> {
        let n = (RATE as f32 * secs) as usize;
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let noise = (state >> 40) as f32 / 8_388_608.0 - 1.0;
                let tau = std::f32::consts::TAU;
                0.3 * (tau * 55.0 * t).sin()
                    + 0.2 * (tau * 220.0 * t).sin()
                    + 0.1 * (tau * 1_760.0 * t).sin()
                    + 0.02 * noise
            })
            .collect()
    }

    fn mono(samples: &[f32]) -> Damage {
        inspect(samples, 1, RATE)
    }

    #[test]
    fn clean_music_has_nothing_wrong_with_it() {
        let d = mono(&music(3.0));
        assert_eq!((d.dropouts, d.clicks), (0, 0));
        assert_eq!(d.summary(Some(16)), "");
    }

    // ── Dropouts ──────────────────────────────────────────────────────────

    /// Samples missing mid-waveform: the music is cut off wherever it was.
    #[test]
    fn a_hole_in_the_middle_is_a_dropout() {
        let mut s = music(3.0);
        s[50_000..50_000 + 2_205].fill(0.0);
        let d = mono(&s);
        assert_eq!(d.dropouts, 1);
        assert!(
            (d.longest_dropout_ms - 50.0).abs() < 1.0,
            "{}",
            d.longest_dropout_ms
        );
        assert_eq!(d.summary(None), "1 dropout");
    }

    /// The break before a drop: the music fades to zero over a few
    /// milliseconds, as a DAW does at a clip edge, and comes back the same way.
    #[test]
    fn a_stop_written_into_the_music_is_not_a_dropout() {
        let mut s = music(3.0);
        let (from, to) = (50_000, 70_000);
        let fade = 200;
        for i in 0..fade {
            let g = 1.0 - i as f32 / fade as f32;
            s[from - fade + i] *= g;
            s[to + i] *= 1.0 - g;
        }
        s[from..to].fill(0.0);
        assert_eq!(mono(&s).dropouts, 0);
    }

    /// Silence at the start and the end is a lead-in and a tail.
    #[test]
    fn silence_at_either_end_is_not_a_dropout() {
        let mut s = vec![0.0; 10_000];
        s.extend(music(2.0));
        s.extend(vec![0.0; 10_000]);
        assert_eq!(mono(&s).dropouts, 0);
    }

    /// A waveform crossing zero touches it for a sample, not for two
    /// milliseconds.
    #[test]
    fn zero_crossings_are_not_dropouts() {
        let s: Vec<f32> = (0..RATE)
            .map(|i| (std::f32::consts::TAU * 30.0 * i as f32 / RATE as f32).sin() * 0.8)
            .collect();
        assert_eq!(mono(&s).dropouts, 0);
    }

    /// A gap only counts when every channel is silent: one quiet channel is a
    /// mix decision.
    #[test]
    fn one_silent_channel_is_not_a_dropout() {
        let m = music(2.0);
        let s: Vec<f32> = m
            .iter()
            .enumerate()
            .flat_map(|(i, v)| {
                let right = if (30_000..40_000).contains(&i) {
                    0.0
                } else {
                    *v
                };
                [*v, right]
            })
            .collect();
        assert_eq!(inspect(&s, 2, RATE).dropouts, 0);
    }

    // ── Clicks ────────────────────────────────────────────────────────────

    #[test]
    fn a_single_spike_is_a_click() {
        let mut s = music(3.0);
        s[40_000] += 0.4;
        s[90_000] -= 0.3;
        let d = mono(&s);
        assert_eq!(d.clicks, 2);
        assert_eq!(d.summary(None), "2 clicks");
    }

    /// A naive saw resets with a single-sample step every cycle. That is a
    /// change of level, not a spike, and a synth is supposed to sound like it.
    #[test]
    fn a_saw_wave_has_no_clicks() {
        let period = 400;
        let s: Vec<f32> = (0..RATE as usize * 2)
            .map(|i| ((i % period) as f32 / period as f32 - 0.5) * 1.2)
            .collect();
        assert_eq!(mono(&s).clicks, 0);
    }

    /// A pulse train is a spike every few milliseconds: a sound, not damage.
    #[test]
    fn a_regular_pulse_is_not_a_click() {
        let mut s = music(2.0);
        for i in (1_000..s.len()).step_by(441) {
            s[i] += 0.5;
        }
        assert_eq!(mono(&s).clicks, 0);
    }

    /// A sample with a glitch baked into it plays the same glitch every time,
    /// and a pan can put one hit in each channel.
    #[test]
    fn the_same_spike_twice_is_part_of_the_track() {
        let m = music(4.0);
        let mut s: Vec<f32> = m.iter().flat_map(|v| [*v, *v]).collect();
        s[2 * 40_000] -= 0.3;
        s[2 * 120_000 + 1] -= 0.32;
        assert_eq!(inspect(&s, 2, RATE).clicks, 0);
    }

    /// Bright noise — a hi-hat, a crash — is rough everywhere, so nothing in
    /// it stands out.
    #[test]
    fn loud_noise_is_not_full_of_clicks() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let s: Vec<f32> = (0..RATE as usize * 2)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state >> 40) as f32 / 8_388_608.0 - 1.0) * 0.7
            })
            .collect();
        assert_eq!(mono(&s).clicks, 0);
    }

    // ── Padded bit depth ──────────────────────────────────────────────────

    #[test]
    fn sixteen_bit_audio_in_a_24_bit_file_is_found() {
        let s: Vec<f32> = music(1.0)
            .iter()
            .map(|v| (v * 32_768.0).round() / 32_768.0)
            .collect();
        let d = mono(&s);
        assert_eq!(d.used_bits, Some(16));
        assert!(d.padded(Some(24)));
        assert!(!d.padded(Some(16)), "a 16-bit file is meant to be 16-bit");
        assert_eq!(d.summary(Some(24)), "16-bit in 24");
    }

    #[test]
    fn real_24_bit_audio_uses_its_bits() {
        let s: Vec<f32> = music(1.0)
            .iter()
            .map(|v| (v * 8_388_608.0).round() / 8_388_608.0)
            .collect();
        let d = mono(&s);
        assert_eq!(d.used_bits, Some(24));
        assert!(!d.padded(Some(24)));
    }

    // ── Length ────────────────────────────────────────────────────────────

    #[test]
    fn a_file_shorter_than_its_header_is_cut_short() {
        let d = Damage {
            stated_secs: Some(400.0),
            decoded_secs: 192.0,
            ..Damage::default()
        };
        assert_eq!(d.missing_secs(), Some(208.0));
        assert_eq!(d.summary(None), "3:28 missing");
    }

    /// A lossless header counts samples, so a tenth of a second short is a
    /// tenth of a second gone — what a corrupt FLAC frame skipped over leaves.
    #[test]
    fn a_lossless_file_a_little_short_is_missing_audio() {
        let d = Damage {
            stated_secs: Some(60.0),
            exact_length: true,
            decoded_secs: 59.9,
            ..Damage::default()
        };
        assert_eq!(d.summary(None), "100 ms missing");
    }

    /// Encoder delay and padding move the length by a frame or two.
    #[test]
    fn a_few_hundred_milliseconds_is_not_a_cut() {
        let d = Damage {
            stated_secs: Some(400.0),
            decoded_secs: 399.7,
            ..Damage::default()
        };
        assert_eq!(d.missing_secs(), None);
        assert_eq!(d.summary(None), "");
    }

    #[test]
    fn everything_found_is_named_worst_first() {
        let d = Damage {
            decode_errors: 3,
            stated_secs: Some(300.0),
            decoded_secs: 120.0,
            dropouts: 1,
            ..Damage::default()
        };
        assert_eq!(
            d.summary(None),
            "3:00 missing · 3 decode errors · 1 dropout"
        );
    }

    #[test]
    fn times_read_like_times() {
        assert_eq!(clock(0.04), "40 ms");
        assert_eq!(clock(0.84), "0.8 s");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(3_725.0), "1:02:05");
    }
}
