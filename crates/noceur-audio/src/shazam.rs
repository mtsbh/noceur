//! A Shazam signature: what a few seconds of a track sound like, in the form
//! Shazam's recogniser takes.
//!
//! Ported from SongRec (`marin-m/SongRec`, GPL-3.0, `src/core/fingerprinting`),
//! which worked the format out from the Android app. Twelve seconds from the
//! middle of the track are brought down to 16 kHz mono, transformed every 128
//! samples, and the spectral peaks that stand out from their neighbours in both
//! time and frequency are kept, in four bands between 250 Hz and 5.5 kHz. What
//! is sent is those peaks, not the audio: they cannot be turned back into it.
//!
//! The network half is `noceur_online::shazam`.

use std::path::Path;

/// The rate the signature is computed at.
const RATE: u32 = 16_000;

/// How much of the track is listened to: what SongRec sends, and about what the
/// phone app records before it gives up.
const LENGTH_SECS: u32 = 12;

/// Transform size, and the hop between transforms.
const N: usize = 2048;
const BINS: usize = N / 2 + 1;
const HOP: usize = 128;

/// Transforms kept for looking back over. Peak recognition reaches 53 back and
/// the ring is indexed modulo its length, so this has to be at least that and a
/// power of two.
const RING: usize = 256;

/// A signature ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// `data:audio/vnd.shazam.sig;base64,…`, the form the request carries.
    pub uri: String,
    /// Length of the audio it was made from, in milliseconds.
    pub sample_ms: u32,
}

/// Makes a signature from twelve seconds in the middle of a file.
///
/// The middle rather than the start: the opening of a club track is a beat
/// that a thousand others share, and the middle is where its own sound is.
pub fn signature(path: &Path) -> Result<Signature, crate::AudioError> {
    let decode_error = |reason: String| crate::AudioError::Decode {
        path: path.display().to_string(),
        reason,
    };

    let full = crate::noceur_extended(path);
    let opened = crate::source::open(&full).map_err(|source| crate::AudioError::Open {
        path: path.display().to_string(),
        source,
    })?;
    let mut builder = rodio::Decoder::builder()
        .with_data(opened.reader)
        .with_seekable(true);
    if let Some(len) = opened.byte_len {
        builder = builder.with_byte_len(len);
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(ext);
    }
    let mut decoder = builder.build().map_err(|e| decode_error(e.to_string()))?;

    let channels = rodio::Source::channels(&decoder).get() as usize;
    let rate = rodio::Source::sample_rate(&decoder).get();
    let total = rodio::Source::total_duration(&decoder);

    // Seek where the container says how long it is; a file that will not seek
    // is read from the start up to the same point instead.
    let start = total
        .map(|t| t.saturating_sub(std::time::Duration::from_secs(u64::from(LENGTH_SECS))) / 2)
        .unwrap_or_default();
    let mut skip = 0;
    if !start.is_zero() && rodio::Source::try_seek(&mut decoder, start).is_err() {
        skip = (start.as_secs_f64() * f64::from(rate)) as usize * channels;
    }

    let wanted = rate as usize * channels * LENGTH_SECS as usize;
    let interleaved: Vec<f32> = decoder.skip(skip).take(wanted).collect();
    let mono: Vec<f32> = interleaved
        .chunks_exact(channels.max(1))
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect();

    // A few seconds is enough for Shazam to answer; less than one is not.
    if mono.len() < rate as usize {
        return Err(decode_error("the file is too short to identify".into()));
    }
    Ok(from_samples(&resample(&mono, rate)))
}

/// Makes a signature from mono samples already at 16 kHz.
pub fn from_samples(mono_16k: &[f32]) -> Signature {
    let peaks = Generator::run(mono_16k);
    let processed = mono_16k.len() / HOP * HOP;
    let bytes = encode(&peaks, processed as u32);
    Signature {
        uri: format!("data:audio/vnd.shazam.sig;base64,{}", base64(&bytes)),
        sample_ms: (processed as u64 * 1000 / u64::from(RATE)) as u32,
    }
}

/// One spectral peak, as Shazam stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Peak {
    /// Which transform it was found in, counting from the first.
    pass: u32,
    /// Log magnitude, scaled the way the app scales it.
    magnitude: u16,
    /// Frequency bin times 64, refined between bins by a parabola through the
    /// neighbours.
    bin: u16,
}

/// Peaks per band: 250–520 Hz, 520–1450 Hz, 1450–3500 Hz, 3500–5500 Hz.
type Bands = [Vec<Peak>; 4];

/// The rolling state of the peak finder, which is SongRec's
/// `SignatureGenerator` with the same buffers and the same look-back offsets.
struct Generator {
    window: Vec<f32>,
    plan: crate::fft::Plan,
    samples: [i16; N],
    samples_at: usize,
    /// Power spectra of the last `RING` transforms.
    ffts: Vec<[f32; BINS]>,
    ffts_at: usize,
    /// The same, each spread to the maximum of its neighbours in time and
    /// frequency, which is what a peak has to beat.
    spread: Vec<[f32; BINS]>,
    spread_at: usize,
    passes: u32,
    bands: Bands,
}

impl Generator {
    fn run(mono_16k: &[f32]) -> Bands {
        let mut g = Generator {
            // `hanning(2050)[1:-1]`, as the app computes it: zero at neither end.
            window: (1..=N)
                .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (N + 1) as f64).cos())
                .map(|w| w as f32)
                .collect(),
            plan: crate::fft::Plan::new(N),
            samples: [0; N],
            samples_at: 0,
            ffts: vec![[0.0; BINS]; RING],
            ffts_at: 0,
            spread: vec![[0.0; BINS]; RING],
            spread_at: 0,
            passes: 0,
            bands: Default::default(),
        };
        for chunk in mono_16k.chunks_exact(HOP) {
            g.transform(chunk);
            g.spread_peaks();
            g.passes += 1;
            if g.passes >= 46 {
                g.recognise();
            }
        }
        g.bands
    }

    fn back(at: usize, by: usize) -> usize {
        (at + RING - by) % RING
    }

    /// Adds 128 samples and transforms the last 2048.
    fn transform(&mut self, chunk: &[f32]) {
        for (slot, s) in self.samples[self.samples_at..self.samples_at + HOP]
            .iter_mut()
            .zip(chunk)
        {
            *slot = (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
        }
        self.samples_at = (self.samples_at + HOP) % N;

        // Oldest first, windowed. The samples stay at 16-bit scale: the
        // thresholds below were set by the app against those values.
        let mut re: Vec<f32> = (0..N)
            .map(|i| f32::from(self.samples[(i + self.samples_at) % N]) * self.window[i])
            .collect();
        let mut im = vec![0.0f32; N];
        self.plan.run(&mut re, &mut im);

        let out = &mut self.ffts[self.ffts_at];
        for (k, o) in out.iter_mut().enumerate() {
            *o = ((re[k] * re[k] + im[k] * im[k]) / (1 << 17) as f32).max(1e-10);
        }
        self.ffts_at = (self.ffts_at + 1) % RING;
    }

    /// Spreads the newest spectrum over its neighbours in frequency, then into
    /// the spectra one, three and six transforms before it.
    fn spread_peaks(&mut self) {
        let mut spread = self.ffts[Self::back(self.ffts_at, 1)];
        for k in 0..=BINS - 3 {
            spread[k] = spread[k].max(spread[k + 1]).max(spread[k + 2]);
        }
        for former in [1, 3, 6] {
            let earlier = &mut self.spread[Self::back(self.spread_at, former)];
            for (e, s) in earlier.iter_mut().zip(&spread) {
                *e = e.max(*s);
            }
        }
        self.spread[self.spread_at] = spread;
        self.spread_at = (self.spread_at + 1) % RING;
    }

    /// Finds the peaks of the spectrum 46 transforms back, now that what came
    /// after it is known.
    fn recognise(&mut self) {
        let current = &self.ffts[Self::back(self.ffts_at, 46)];
        let spread = &self.spread[Self::back(self.spread_at, 49)];

        for bin in 10..=1014 {
            let value = current[bin];
            if value < 1.0 / 64.0 || value < spread[bin - 1] {
                continue;
            }
            // Louder than its neighbours in frequency…
            let around = [-10, -7, -4, -3, 1, 2, 5, 8]
                .iter()
                .map(|o| spread[(bin as isize + o) as usize])
                .fold(0.0f32, f32::max);
            if value <= around {
                continue;
            }
            // …and in time.
            let around = [
                -53, -45, 165, 172, 179, 186, 193, 200, 214, 221, 228, 235, 242, 249,
            ]
            .iter()
            .map(|o| {
                self.spread[(self.spread_at as isize + o).rem_euclid(RING as isize) as usize]
                    [bin - 1]
            })
            .fold(around, f32::max);
            if value <= around {
                continue;
            }

            let level = |v: f32| v.max(1.0 / 64.0).ln() * 1477.3 + 6144.0;
            let (before, here, after) = (
                level(current[bin - 1]),
                level(value),
                level(current[bin + 1]),
            );
            let curvature = here * 2.0 - before - after;
            let shift = if curvature > 0.0 {
                (after - before) * 32.0 / curvature
            } else {
                0.0
            };
            let fine_bin = (bin as i32 * 64 + shift as i32) as u16;

            let hz = f32::from(fine_bin) * (RATE as f32 / 2.0 / 1024.0 / 64.0);
            let band = match hz as i32 {
                250..=519 => 0,
                520..=1449 => 1,
                1450..=3499 => 2,
                3500..=5500 => 3,
                _ => continue,
            };
            self.bands[band].push(Peak {
                pass: self.passes - 46,
                magnitude: here as u16,
                bin: fine_bin,
            });
        }
    }
}

/// The binary signature: a 48-byte header, then each band's peaks.
fn encode(bands: &Bands, samples: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let u32le = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());

    u32le(&mut out, 0xcafe_2580);
    u32le(&mut out, 0); // CRC-32, filled in last
    u32le(&mut out, 0); // size after the header, likewise
    u32le(&mut out, 0x9411_9c00);
    for _ in 0..3 {
        u32le(&mut out, 0);
    }
    u32le(&mut out, 3 << 27); // 16 kHz
    for _ in 0..2 {
        u32le(&mut out, 0);
    }
    u32le(&mut out, samples + (RATE as f32 * 0.24) as u32);
    u32le(&mut out, (15 << 19) + 0x40000);
    u32le(&mut out, 0x4000_0000);
    u32le(&mut out, 0); // size again

    for (band, peaks) in bands.iter().enumerate() {
        if peaks.is_empty() {
            continue;
        }
        let mut data = Vec::new();
        let mut pass = 0;
        for p in peaks {
            // Passes are stored as the step from the previous peak, in a byte;
            // a longer step is written out in full first.
            if p.pass - pass >= 255 {
                data.push(0xff);
                data.extend_from_slice(&p.pass.to_le_bytes());
                pass = p.pass;
            }
            data.push((p.pass - pass) as u8);
            data.extend_from_slice(&p.magnitude.to_le_bytes());
            data.extend_from_slice(&p.bin.to_le_bytes());
            pass = p.pass;
        }
        u32le(&mut out, 0x6003_0040 + band as u32);
        u32le(&mut out, data.len() as u32);
        let padded = data.len().next_multiple_of(4);
        out.extend_from_slice(&data);
        out.resize(out.len() + padded - data.len(), 0);
    }

    let size = (out.len() - 48) as u32;
    out[8..12].copy_from_slice(&size.to_le_bytes());
    out[52..56].copy_from_slice(&size.to_le_bytes());
    let crc = crc32(&out[8..]);
    out[4..8].copy_from_slice(&crc.to_le_bytes());
    out
}

/// Mono audio at any rate, brought to 16 kHz.
///
/// A windowed-sinc low-pass evaluated at each output instant. Only up to 5.5
/// kHz is looked at, so the filter passes that flat and is deep by 10.5 kHz,
/// the lowest frequency that would fold back below 5.5 kHz at the new rate.
fn resample(mono: &[f32], rate: u32) -> Vec<f32> {
    if rate == RATE {
        return mono.to_vec();
    }
    let ratio = f64::from(rate) / f64::from(RATE);
    let cutoff = 8_000.0f64.min(f64::from(rate) / 2.0) / f64::from(rate); // cycles per input sample
    let cutoff = cutoff * 0.98;
    // Transition from 5.5 kHz to the cutoff's mirror; Blackman needs about
    // 5.5 / width taps.
    let width = (cutoff - 5_500.0 / f64::from(rate)).max(0.01);
    let half = ((5.5 / width / 2.0).ceil() as isize).max(4);

    let len = (mono.len() as f64 / ratio) as usize;
    (0..len)
        .map(|m| {
            let centre = m as f64 * ratio;
            let first = centre.floor() as isize - half + 1;
            let mut acc = 0.0f64;
            let mut gain = 0.0f64;
            for i in first..first + 2 * half {
                if i < 0 || i as usize >= mono.len() {
                    continue;
                }
                let t = i as f64 - centre;
                let sinc = if t.abs() < 1e-9 {
                    2.0 * cutoff
                } else {
                    (2.0 * std::f64::consts::PI * cutoff * t).sin() / (std::f64::consts::PI * t)
                };
                let x = (t / half as f64 + 1.0) * std::f64::consts::PI;
                let w = 0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos();
                acc += sinc * w * f64::from(mono[i as usize]);
                gain += sinc * w;
            }
            // Normalised per sample so the level holds at any fractional offset.
            if gain.abs() > 1e-12 {
                (acc / gain) as f32
            } else {
                0.0
            }
        })
        .collect()
}

/// CRC-32 (IEEE), as zlib computes it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

/// Standard base64 with padding, which is what the data URI carries.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_zlib() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn base64_is_padded_standard_base64() {
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    /// A few seconds of shifting notes over a beat, at 16 kHz.
    fn music(secs: usize) -> Vec<f32> {
        (0..RATE as usize * secs)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let hz = [330.0, 660.0, 990.0, 1760.0, 2640.0, 4000.0][(t * 4.0) as usize % 6];
                let kick = if (t * 2.0).fract() < 0.05 { 0.5 } else { 0.0 };
                (2.0 * std::f32::consts::PI * hz * t).sin() * 0.4 + kick * (t * 300.0).sin()
            })
            .collect()
    }

    #[test]
    fn notes_turn_into_peaks_in_their_bands() {
        let bands = Generator::run(&music(12));
        for (i, band) in bands.iter().enumerate() {
            assert!(!band.is_empty(), "band {i} has no peaks");
            assert!(band.windows(2).all(|w| w[0].pass <= w[1].pass), "in order");
        }
        // A 660 Hz note lands in the second band, at bin 660 / 7.8125 ≈ 84.5.
        let near = bands[1]
            .iter()
            .any(|p| (f32::from(p.bin) / 64.0 - 84.5).abs() < 1.5);
        assert!(near, "{:?}", &bands[1][..bands[1].len().min(5)]);
    }

    #[test]
    fn the_header_describes_the_body() {
        let sig = from_samples(&music(12));
        assert_eq!(sig.sample_ms, 12_000);
        let b64 = sig
            .uri
            .strip_prefix("data:audio/vnd.shazam.sig;base64,")
            .unwrap();
        assert!(b64.len().is_multiple_of(4));

        let bytes = encode(&Generator::run(&music(12)), 12 * RATE);
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!(word(0), 0xcafe_2580);
        assert_eq!(word(4), crc32(&bytes[8..]));
        assert_eq!(word(8) as usize, bytes.len() - 48);
        assert_eq!(word(12), 0x9411_9c00);
        assert_eq!(word(28), 0x1800_0000);
        assert_eq!(word(40), 12 * RATE + 3840);
        assert_eq!(word(48), 0x4000_0000);
        assert_eq!(word(56), 0x6003_0040, "first band follows");
        assert_eq!(bytes.len() % 4, 0);
    }

    /// A long gap between peaks is written as an escape and the full number.
    #[test]
    fn a_long_gap_between_peaks_is_written_in_full() {
        let peak = |pass| Peak {
            pass,
            magnitude: 7000,
            bin: 3000,
        };
        let bands: Bands = [vec![peak(2), peak(400)], vec![], vec![], vec![]];
        let bytes = encode(&bands, 16_000);
        let data = &bytes[64..];
        assert_eq!(data[0], 2);
        assert_eq!(&data[5..10], &[0xff, 0x90, 0x01, 0, 0]);
        assert_eq!(data[10], 0);
    }

    #[test]
    fn resampling_keeps_a_tone_and_its_level() {
        let rate = 44_100;
        let tone: Vec<f32> = (0..rate)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect();
        let out = resample(&tone, rate);
        assert_eq!(out.len(), 16_000);
        let peak = out[1000..15_000].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.02, "{peak}");
        // Zero crossings: a 1 kHz tone crosses 2000 times a second.
        let crossings = out[..]
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        assert!((1990..=2010).contains(&crossings), "{crossings}");
    }

    /// What would fold back into the analysed band is filtered out first.
    #[test]
    fn resampling_removes_what_would_alias() {
        let rate = 44_100;
        let high: Vec<f32> = (0..rate)
            .map(|i| (2.0 * std::f32::consts::PI * 12_000.0 * i as f32 / rate as f32).sin())
            .collect();
        let out = resample(&high, rate);
        let peak = out[1000..15_000].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 0.01, "{peak}");
    }
}
