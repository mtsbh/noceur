//! An acoustic fingerprint: what a track sounds like, for looking it up.
//!
//! Unlike [`crate::identity`], which matches exact copies, this is built to
//! survive a re-encode. It is Chromaprint's fingerprint, computed in-process by
//! `rusty-chromaprint` with the same algorithm `fpcalc` uses by default, so the
//! string it produces is one AcoustID can look up.

use std::path::Path;

/// How much of the track is fingerprinted.
///
/// What `fpcalc` reads by default, and what AcoustID's database was built from.
/// Reading the whole track would not find more matches and would take longer.
const LENGTH_SECS: u32 = 120;

/// A track's fingerprint, ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// Compressed and base64-encoded, the form AcoustID's lookup takes.
    pub encoded: String,
    /// Length of the whole track, when the container says. AcoustID asks for it
    /// alongside the fingerprint.
    pub duration_secs: Option<u32>,
}

/// Fingerprints the first two minutes of a file.
pub fn fingerprint(path: &Path) -> Result<Fingerprint, crate::AudioError> {
    let full = crate::noceur_extended(path);
    let opened = crate::source::open(&full).map_err(|source| crate::AudioError::Open {
        path: path.display().to_string(),
        source,
    })?;

    let mut builder = rodio::Decoder::builder().with_data(opened.reader);
    if let Some(len) = opened.byte_len {
        builder = builder.with_byte_len(len);
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(ext);
    }
    let decoder = builder.build().map_err(|e| crate::AudioError::Decode {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    let channels = rodio::Source::channels(&decoder).get();
    let rate = rodio::Source::sample_rate(&decoder).get();
    let duration_secs = rodio::Source::total_duration(&decoder).map(|d| d.as_secs() as u32);

    let wanted = rate as usize * channels as usize * LENGTH_SECS as usize;
    let samples: Vec<i16> = decoder
        .take(wanted)
        .map(|s| (s.clamp(-1.0, 1.0) * 32_767.0) as i16)
        .collect();

    let encoded = encode(&samples, rate, channels).ok_or_else(|| crate::AudioError::Decode {
        path: path.display().to_string(),
        reason: "the file is too short to identify".into(),
    })?;
    Ok(Fingerprint {
        encoded,
        duration_secs,
    })
}

/// Fingerprints interleaved samples. `None` when there is too little audio to
/// produce one.
pub fn encode(samples: &[i16], rate: u32, channels: u16) -> Option<String> {
    let config = rusty_chromaprint::Configuration::preset_test2();
    let mut printer = rusty_chromaprint::Fingerprinter::new(&config);
    printer.start(rate, u32::from(channels)).ok()?;
    printer.consume(samples);
    printer.finish();

    let raw = printer.fingerprint();
    if raw.is_empty() {
        return None;
    }
    let compressed = rusty_chromaprint::FingerprintCompressor::from(&config).compress(raw);
    Some(base64_url(&compressed))
}

/// URL-safe base64 without padding, which is how Chromaprint writes a
/// fingerprint.
fn base64_url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_reference_encoding() {
        assert_eq!(base64_url(b""), "");
        assert_eq!(base64_url(b"f"), "Zg");
        assert_eq!(base64_url(b"fo"), "Zm8");
        assert_eq!(base64_url(b"foo"), "Zm9v");
        assert_eq!(base64_url(b"foobar"), "Zm9vYmFy");
        // The two characters that differ from standard base64.
        assert_eq!(base64_url(&[0xfb, 0xff]), "-_8");
    }

    /// Chromaprint writes the algorithm first: `AQA…` is algorithm 1, which is
    /// what `fpcalc` produces and what AcoustID indexes.
    #[test]
    fn a_tone_fingerprints_with_the_fpcalc_algorithm() {
        let rate = 44_100;
        let samples: Vec<i16> = (0..rate * 20)
            .map(|i| {
                let t = i as f32 / rate as f32;
                // A few notes, so the chroma changes over time.
                let hz = [220.0, 277.2, 329.6, 440.0][(t as usize / 2) % 4];
                ((2.0 * std::f32::consts::PI * hz * t).sin() * 12_000.0) as i16
            })
            .collect();
        let fp = encode(&samples, rate as u32, 1).expect("fingerprint");
        assert!(fp.starts_with("AQA"), "{fp}");
        assert_eq!(encode(&samples, rate as u32, 1).unwrap(), fp, "stable");
    }

    #[test]
    fn silence_too_short_to_measure_has_no_fingerprint() {
        assert_eq!(encode(&[0; 100], 44_100, 1), None);
    }
}
