//! Cue points: where a track's sections start, on its beat grid.
//!
//! Club music is built in phrases of eight bars, and a section — the kick
//! coming in, a breakdown, the drop — starts on a phrase line. So the grid is
//! found first (the tempo, then the beat it falls on, then which beat is the
//! one), and the cues are the phrase lines where the sound changes most.
//!
//! The grid: the tempo from [`tempo`], then refined with the beat's phase over
//! the whole track, mostly on the kick (the bands below about 250 Hz), since
//! a hi-hat on the offbeat lines up as well as a kick on the beat does.
//!
//! The changes: each beat is described by its spectrum in eight bands, and a
//! line scores how far the four bars after it differ from the four before.
//! Which beat is the one, and where the phrases start, are both read from the
//! same scores: the phrase lines are where the changes are.

use crate::tempo::{self, Range, HOP, MEL_BANDS, N_FFT};

/// Cues a track gets at most: rekordbox's eight hot cues.
pub const MAX_CUES: usize = 8;
/// The longest stretch read. Past twenty minutes a track is a mix.
const LONGEST_SECS: usize = 1200;
const BEATS_PER_BAR: usize = 4;
/// Beats in a phrase: eight bars.
const PHRASE: usize = 32;
/// Cues never sit closer together than this many beats: one phrase.
const MIN_GAP: usize = PHRASE;
/// Beats each side of a line that its change is measured over: four bars.
const WINDOW: usize = 16;
/// The mel bands that hold the kick and the bass, below about 250 Hz.
const LOW: std::ops::Range<usize> = 0..4;
/// The two lowest mel bands, below about 140 Hz: the kick without the pads
/// and chords that sit just above it.
const SUB: std::ops::Range<usize> = 0..2;
/// The mel bands a clap or snare is loudest in, about 1.4 to 4 kHz.
const CLAP: std::ops::Range<usize> = 14..26;
/// The mel bands above about 4 kHz: hats and shakers.
const HATS: std::ops::Range<usize> = 26..MEL_BANDS;
/// Bands a beat is described by: the forty mel bands, five at a time.
const GROUPS: usize = 8;
/// A line on a half phrase (four bars) counts this much of one on a phrase.
const HALF_PHRASE: f32 = 0.75;
/// A line between phrases counts this much.
const OFF_PHRASE: f32 = 0.6;
/// How much louder the claps on one set of alternate beats must be than on
/// the other to say which is the backbeat.
const BACKBEAT: f32 = 1.05;
/// A change weaker than this share of the strongest is not a cue.
const WEAKEST: f32 = 0.2;
/// What a beat's spectrum is read to, below the track's loudest: detail
/// under it is noise floor and reverb tails.
const FLOOR_DB: f32 = 40.0;
/// A beat this far under the track's median loudness is not playing.
const QUIET_DB: f32 = 12.0;
/// A section whose kick is under this share of the one before's, and no
/// fuller, is a break.
const KICK_IN: f32 = 0.85;
/// So is one this many dB under the section before it.
const QUIETER_DB: f32 = 2.0;
/// Below this the beats hardly line up: ambient, a field recording, speech.
const MIN_CONFIDENCE: f64 = 0.05;

/// A constant-tempo beat grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub bpm: f64,
    /// The first downbeat, in seconds.
    pub first: f64,
}

impl Grid {
    pub fn beat(&self) -> f64 {
        60.0 / self.bpm
    }
}

/// What a section is, from its sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Intro,
    /// The kick is in.
    Drop,
    /// The kick is out.
    Break,
    Outro,
}

impl Section {
    pub fn name(self) -> &'static str {
        match self {
            Section::Intro => "Intro",
            Section::Drop => "Drop",
            Section::Break => "Break",
            Section::Outro => "Outro",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cue {
    /// Seconds.
    pub at: f64,
    pub section: Section,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cues {
    pub grid: Grid,
    /// In time order; the first is the first downbeat.
    pub cues: Vec<Cue>,
}

/// The finder's version: cues stored by another are found again.
pub const VERSION: i64 = 1;

impl Section {
    fn parse(s: &str) -> Option<Section> {
        [
            Section::Intro,
            Section::Drop,
            Section::Break,
            Section::Outro,
        ]
        .into_iter()
        .find(|v| v.name() == s)
    }
}

impl Cues {
    /// As the index keeps them: `bpm first`, then `seconds section` a line.
    pub fn to_text(&self) -> String {
        let mut out = format!("{:.4} {:.4}", self.grid.bpm, self.grid.first);
        for c in &self.cues {
            out.push_str(&format!("\n{:.4} {}", c.at, c.section.name()));
        }
        out
    }

    /// Reads [`to_text`](Self::to_text); `None` for anything else, the empty
    /// text of a track with no grid among it.
    pub fn from_text(text: &str) -> Option<Cues> {
        let mut lines = text.lines();
        let mut head = lines.next()?.split(' ');
        let grid = Grid {
            bpm: head.next()?.parse().ok()?,
            first: head.next()?.parse().ok()?,
        };
        let cues = lines
            .map(|l| {
                let (at, section) = l.split_once(' ')?;
                Some(Cue {
                    at: at.parse().ok()?,
                    section: Section::parse(section)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Cues { grid, cues })
    }
}

/// Finds a file's grid and cues.
pub fn detect(path: &std::path::Path) -> Result<Option<Cues>, crate::AudioError> {
    let d = crate::decode(path, LONGEST_SECS)?;
    Ok(analyse(&d.samples, d.rate, d.channels))
}

/// Finds the grid and cues of interleaved samples. `None` when there is no
/// steady beat to put a grid on.
pub fn analyse(samples: &[f32], rate: u32, channels: usize) -> Option<Cues> {
    let (spec, rate) = spectrogram(samples, rate, channels);
    from_spectrogram(&spec, rate)
}

/// The mel spectrogram the cues are found in, and the rate its frames are
/// counted at; kept apart from [`from_spectrogram`] so a tuning run can keep it.
pub fn spectrogram(samples: &[f32], rate: u32, channels: usize) -> (Vec<f32>, u32) {
    let (mono, rate) = tempo::downmix(samples, rate, channels.max(1));
    (tempo::mel_spectrogram(&mono, rate), rate)
}

/// [`analyse`], from a [`spectrogram`].
pub fn from_spectrogram(spec: &[f32], rate: u32) -> Option<Cues> {
    let frames = spec.len() / MEL_BANDS;
    let fps = rate as f64 / HOP as f64;
    let full = tempo::detrend(&tempo::flux(spec, 0..MEL_BANDS), rate);
    let estimate = tempo::tempo(&full, fps, Range::default())?;
    if estimate.confidence < MIN_CONFIDENCE {
        return None;
    }
    let low = tempo::detrend(&tempo::flux(spec, LOW), rate);
    let (period, phase) = lock(&low, &full, 60.0 * fps / estimate.bpm);

    // Every beat, by the frame it starts on.
    let beats: Vec<f64> = (0..)
        .map(|k| phase + k as f64 * period)
        .take_while(|f| f + period <= frames as f64)
        .collect();
    if beats.len() < PHRASE * 2 {
        return None;
    }
    let loudest = spec.iter().copied().fold(f32::MIN, f32::max);
    let features: Vec<[f32; GROUPS]> = beats
        .iter()
        .map(|&b| describe(spec, b, period, loudest - FLOOR_DB))
        .collect();

    // Where the music is: a fade or a tail of silence changes a great deal in
    // dB and is no section. A beat is as loud as its loudest band, so a sparse
    // track's quiet bands do not make its silence look like music.
    let loudness: Vec<f32> = features
        .iter()
        .map(|f| f.iter().copied().fold(f32::MIN, f32::max))
        .collect();
    let mut sorted = loudness.clone();
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let playing = |b: &usize| loudness[*b] > median - QUIET_DB;
    let start = (0..beats.len()).find(playing).unwrap_or(0);
    let end = (0..beats.len())
        .rev()
        .find(playing)
        .unwrap_or(beats.len() - 1);

    // What a beat is made of: its spectrum, and how busy its kick, clap and
    // hats are. Each is scaled to its spread over the track, so the hats
    // coming in counts as much as the bass going out.
    let busy = |env: &[f32], b: usize| {
        let (from, to) = (
            beats[b].round() as usize,
            (beats[b] + period).round() as usize,
        );
        let s = &env[from.min(env.len())..to.min(env.len())];
        s.iter().map(|v| v.max(0.0)).sum::<f32>() / s.len().max(1) as f32
    };
    let onsets = [
        tempo::flux(spec, LOW),
        tempo::flux(spec, CLAP),
        tempo::flux(spec, HATS),
    ];
    let sub = tempo::flux(spec, SUB);
    let kicks: Vec<f32> = (0..beats.len()).map(|b| busy(&sub, b)).collect();
    let mut rich: Vec<Vec<f32>> = (0..beats.len())
        .map(|b| {
            let mut v = features[b].to_vec();
            v.extend(onsets.iter().map(|e| busy(e, b)));
            v
        })
        .collect();
    for d in 0..rich[0].len() {
        let n = (end + 1 - start) as f32;
        let mean = (start..=end).map(|b| rich[b][d]).sum::<f32>() / n;
        let sd = ((start..=end)
            .map(|b| (rich[b][d] - mean).powi(2))
            .sum::<f32>()
            / n)
            .sqrt()
            .max(1e-6);
        for r in &mut rich {
            r[d] = (r[d] - mean) / sd;
        }
    }
    // A line's change, over four bars each side and over one.
    let wide = novelty(&rich, WINDOW);
    let sharp = novelty(&rich, BEATS_PER_BAR);
    let novelty: Vec<f32> = wide.iter().zip(&sharp).map(|(w, s)| w + s).collect();
    let sum = |o: usize, step: usize, f: &dyn Fn(usize) -> f32| {
        (o..beats.len())
            .step_by(step)
            .filter(|b| (start..=end).contains(b))
            .map(f)
            .sum::<f32>()
    };

    // Which beat is the one. A track starts on a downbeat, and the start is
    // found to within a beat: the first beat loud enough, or the one before it
    // when that one is quieter. The backbeat, a clap or snare on two and four,
    // tells the two apart.
    let hit = |b: usize| {
        [-1.0, 0.0, 1.0]
            .iter()
            .map(|d| at(&onsets[1], beats[b] + d))
            .fold(f32::MIN, f32::max)
            .max(0.0)
    };
    let (even, odd) = (sum(0, 2, &hit), sum(1, 2, &hit));
    let options = [
        start % BEATS_PER_BAR,
        (start + BEATS_PER_BAR - 1) % BEATS_PER_BAR,
    ];
    let downbeat = if even > odd * BACKBEAT {
        options[1 - options[0] % 2]
    } else if odd > even * BACKBEAT {
        options[options[0] % 2]
    } else {
        options[0]
    };
    // Where the phrases start: the bar, of the first eight, whose every
    // eighth bar changes most.
    let origin = (0..PHRASE / BEATS_PER_BAR)
        .map(|k| downbeat + k * BEATS_PER_BAR)
        .max_by(|&a, &b| {
            let s = |o: usize| sum(o, PHRASE, &|b| novelty[b]);
            s(a).total_cmp(&s(b))
        })
        .unwrap_or(downbeat);
    let first = (start.saturating_sub(1)..beats.len())
        .find(|b| b % BEATS_PER_BAR == downbeat)
        .unwrap_or(start);

    // The lines: every bar line where a change peaks, those on a phrase
    // counted in full, on a half phrase less, and between less again; the
    // strongest taken first, each at least a phrase from the others.
    let mut lines: Vec<(usize, f32)> = (first + 1..beats.len())
        .filter(|&b| b % BEATS_PER_BAR == downbeat && b + BEATS_PER_BAR * 4 <= end)
        .filter(|&b| {
            let (lo, hi) = (b.saturating_sub(BEATS_PER_BAR), b + BEATS_PER_BAR);
            novelty[lo] <= novelty[b] && novelty.get(hi).is_none_or(|n| *n <= novelty[b])
        })
        .map(|b| {
            let weight = match (b + PHRASE - origin) % PHRASE {
                0 => 1.0,
                16 => HALF_PHRASE,
                _ => OFF_PHRASE,
            };
            (b, novelty[b] * weight)
        })
        .collect();
    lines.sort_by(|a, b| b.1.total_cmp(&a.1));
    let strongest = lines.first().map(|l| l.1).unwrap_or(0.0);
    let mut chosen = vec![first];
    for (b, score) in lines {
        if chosen.len() == MAX_CUES || score < strongest * WEAKEST {
            break;
        }
        if chosen.iter().all(|&c| c.abs_diff(b) >= MIN_GAP) {
            chosen.push(b);
        }
    }
    chosen.sort_unstable();

    let seconds = |frame: f64| seconds_of(frame, rate);
    let grid = Grid {
        bpm: 60.0 * fps / period,
        first: seconds(beats[first % BEATS_PER_BAR]),
    };
    let fullness: Vec<f32> = features
        .iter()
        .map(|f| f.iter().sum::<f32>() / GROUPS as f32)
        .collect();
    let sections = name(&chosen, &kicks, &fullness);
    let cues = chosen
        .iter()
        .zip(sections)
        .map(|(&b, section)| Cue {
            at: seconds(beats[b]),
            section,
        })
        .collect();
    Some(Cues { grid, cues })
}

/// The time of a spectrogram frame: its window's centre.
fn seconds_of(frame: f64, rate: u32) -> f64 {
    (frame * HOP as f64 + N_FFT as f64 / 2.0) / rate as f64
}

/// The beat period and phase, in frames, that line up best with the onsets
/// over the whole track: the period within a fifth of a percent of `period`.
fn lock(low: &[f32], full: &[f32], period: f64) -> (f64, f64) {
    let score = |p: f64, phase: f64| -> f64 {
        let (mut sum, mut n) = (0.0, 0);
        let mut f = phase;
        while f + 1.0 < low.len() as f64 {
            sum += at(low, f) as f64 + 0.5 * at(full, f) as f64;
            n += 1;
            f += p;
        }
        if n == 0 {
            0.0
        } else {
            sum / n as f64
        }
    };
    let mut best = (period, 0.0, f64::MIN);
    for i in 0..=40 {
        let p = period * (0.998 + 0.004 * i as f64 / 40.0);
        let mut phase = 0.0;
        while phase < p {
            let s = score(p, phase);
            if s > best.2 {
                best = (p, phase, s);
            }
            phase += 0.25;
        }
    }
    (best.0, best.1)
}

/// `values` at a fractional index, linearly.
fn at(values: &[f32], pos: f64) -> f32 {
    let i = pos.floor() as usize;
    let t = (pos - i as f64) as f32;
    match (values.get(i), values.get(i + 1)) {
        (Some(a), Some(b)) => a + (b - a) * t,
        (Some(a), None) => *a,
        _ => 0.0,
    }
}

/// A beat's spectrum: the mean dB of each group of bands over its frames.
fn describe(spec: &[f32], start: f64, period: f64, floor: f32) -> [f32; GROUPS] {
    let (from, to) = (start.round() as usize, (start + period).round() as usize);
    let per = MEL_BANDS / GROUPS;
    let mut out = [0.0f32; GROUPS];
    let n = to.saturating_sub(from).max(1) as f32;
    for f in from..to {
        let frame = &spec[f * MEL_BANDS..][..MEL_BANDS];
        for (g, o) in out.iter_mut().enumerate() {
            *o += frame[g * per..][..per]
                .iter()
                .map(|v| v.max(floor))
                .sum::<f32>()
                / per as f32;
        }
    }
    out.map(|v| v / n)
}

/// How far the `window` beats from each beat on differ from those before it.
fn novelty(features: &[Vec<f32>], window: usize) -> Vec<f32> {
    let dims = features.first().map_or(0, Vec::len);
    let mean = |range: std::ops::Range<usize>| {
        let n = range.len().max(1) as f32;
        let mut m = vec![0.0f32; dims];
        for f in &features[range] {
            for g in 0..dims {
                m[g] += f[g] / n;
            }
        }
        m
    };
    (0..features.len())
        .map(|b| {
            let w = window.min(b).min(features.len() - b);
            if w < window / 2 || w == 0 {
                return 0.0;
            }
            let (before, after) = (mean(b - w..b), mean(b..b + w));
            before
                .iter()
                .zip(after)
                .map(|(x, y)| (x - y) * (x - y))
                .sum::<f32>()
                .sqrt()
        })
        .collect()
}

/// Each cue's section: a break thins out from the section before it, its kick
/// or its whole sound; the first is the intro, and a last one in the final
/// quarter the outro. Against Mixed In Key's energy, about half the breaks it
/// marks (two or more under the track's usual) are called breaks here, and
/// about half of these are such.
fn name(cues: &[usize], kicks: &[f32], fullness: &[f32]) -> Vec<Section> {
    let beats = kicks.len();
    let mean = |v: &[f32], from: usize, to: usize| {
        let s = &v[from..to.max(from + 1).min(beats)];
        s.iter().sum::<f32>() / s.len().max(1) as f32
    };

    let last = cues.len().saturating_sub(1);
    cues.iter()
        .enumerate()
        .map(|(i, &from)| {
            let to = cues.get(i + 1).copied().unwrap_or(beats);
            let before = cues[i.saturating_sub(1)];
            let fuller = mean(fullness, from, to) - mean(fullness, before, from);
            // The kick thinning out counts only when nothing fills in for it: a
            // full drop after a bare kick intro also makes the kick look thinner.
            let thin = (mean(kicks, from, to) < mean(kicks, before, from) * KICK_IN
                && fuller < 0.0)
                || fuller < -QUIETER_DB;
            if i == 0 {
                Section::Intro
            } else if i == last && i > 1 && from * 4 > beats * 3 {
                Section::Outro
            } else if thin {
                Section::Break
            } else {
                Section::Drop
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 22_050;

    /// A second of silence, as a file starts, then a track at `bpm` built
    /// from sections of `(bars, kick, clap, hats, pad)`.
    fn track(bpm: f64, sections: &[(usize, bool, bool, bool, bool)]) -> Vec<f32> {
        let beat = 60.0 / bpm * RATE as f64;
        let bars: usize = sections.iter().map(|s| s.0).sum();
        let n = (bars as f64 * 4.0 * beat) as usize;
        let mut out = vec![0.0f32; n];
        let mut noise = 0x9e37_79b9u32;
        let mut white = move || {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            noise as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        let mut b = 0usize;
        for &(len, kick, clap, hats, pad) in sections {
            for _ in 0..len * 4 {
                let at = (b as f64 * beat) as usize;
                let next = (((b + 1) as f64 * beat) as usize).min(n);
                for (i, o) in out.iter_mut().enumerate().take(next).skip(at) {
                    let t = (i - at) as f32 / RATE as f32;
                    let mut s = 0.0;
                    if kick {
                        s +=
                            (2.0 * std::f32::consts::PI * 55.0 * t).sin() * (-t * 25.0).exp() * 0.7;
                    }
                    if clap && b % 2 == 1 {
                        s += white() * (-t * 40.0).exp() * 0.4;
                    }
                    if hats {
                        let off = t - (beat as f32 / 2.0) / RATE as f32;
                        if off >= 0.0 {
                            s += white() * (-off * 200.0).exp() * 0.15;
                        }
                    }
                    if pad {
                        let g = i as f32 / RATE as f32;
                        s += (2.0 * std::f32::consts::PI * 220.0 * g).sin() * 0.15;
                    }
                    *o = s;
                }
                b += 1;
            }
        }
        let mut lead = vec![0.0f32; RATE as usize];
        lead.extend(out);
        lead
    }

    #[test]
    fn sections_are_cued_on_their_first_downbeat() {
        let bpm = 125.0;
        let samples = track(
            bpm,
            &[
                (16, true, false, true, false),
                (16, true, true, true, true),
                (16, false, false, true, true),
                (32, true, true, true, true),
                (16, true, false, false, false),
            ],
        );
        let c = analyse(&samples, RATE, 1).expect("a grid");
        assert!((c.grid.bpm - bpm).abs() < 0.05, "bpm {}", c.grid.bpm);
        let bar = 4.0 * c.grid.beat();
        assert!(
            (c.grid.first - 1.0).abs() < 0.03,
            "first downbeat at {}",
            c.grid.first
        );
        let at: Vec<f64> = c.cues.iter().map(|q| (q.at - 1.0) / bar).collect();
        for want in [0.0, 16.0, 32.0, 48.0, 80.0] {
            assert!(
                at.iter().any(|a| (a - want).abs() < 0.1),
                "no cue at bar {want}: {at:?}"
            );
        }
        let section = |bar_no: f64| {
            c.cues
                .iter()
                .find(|q| ((q.at - 1.0) / bar - bar_no).abs() < 0.1)
                .map(|q| q.section)
        };
        assert_eq!(section(0.0), Some(Section::Intro));
        assert_eq!(section(32.0), Some(Section::Break));
        assert_eq!(section(48.0), Some(Section::Drop));
        assert_eq!(section(80.0), Some(Section::Outro));
    }

    #[test]
    fn silence_has_no_grid() {
        assert_eq!(analyse(&vec![0.0; RATE as usize * 60], RATE, 1), None);
    }

    #[test]
    fn cues_survive_the_index() {
        let c = Cues {
            grid: Grid {
                bpm: 124.0,
                first: 0.1234,
            },
            cues: vec![
                Cue {
                    at: 0.1234,
                    section: Section::Intro,
                },
                Cue {
                    at: 62.0,
                    section: Section::Break,
                },
            ],
        };
        assert_eq!(Cues::from_text(&c.to_text()), Some(c));
        assert_eq!(Cues::from_text(""), None);
        assert_eq!(Cues::from_text("124 0.1\n3.0 Chorus"), None);
    }
}
