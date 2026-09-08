//! What a track *is*, regardless of what has been done to its tags.
//!
//! The index identifies a file by its bytes — size plus a hash of its head and
//! tail — which is cheap and answers "is this the same file". It is the wrong
//! answer to "is this the same track": retagging changes the bytes, so a copy on
//! the party SSD that Rekordbox has written to becomes, to the index, a
//! different piece of audio from the one it was copied from.
//!
//! This is the other identity: a hash of the decoded samples. Tags are not part
//! of the audio, so nothing anybody writes into them can change it. Two files
//! with the same fingerprint hold the same sound however differently they are
//! labelled, wrapped or dated.
//!
//! What it is not is a *fingerprint* in the AcoustID sense. It will not match a
//! 320 kbps encode to the FLAC it came from — those genuinely are different
//! audio, and telling them apart is most of the point. It matches copies.

use std::path::Path;

/// Samples are quantised to sixteen bits before hashing.
///
/// Not for tolerance — quantising cannot give that, since any step has
/// boundaries and a sample sitting on one flips either way. It is so the hash
/// depends on the *number* rather than on its float encoding: the same integer
/// sample can reach here as slightly different `f32` values depending on the
/// path a decoder took to produce it, and the integer is the thing that was
/// actually in the file.
///
/// The consequence is worth being plain about: this is an exact match on
/// decoded audio. It finds copies, which decode identically. It does not find
/// "nearly the same" — a re-encode, a normalised version, or the same track at
/// another bitrate are all different audio and all hash differently.
const SCALE: f32 = 32_767.0;

/// FNV-1a, 64-bit.
///
/// Written out rather than taken from `std`: `DefaultHasher` is explicitly not
/// stable across Rust versions, and this number is written to a database and
/// compared against numbers written by other builds. A hash that quietly changed
/// meaning between releases would silently un-match every file in the library.
struct Fnv(u64);

impl Fnv {
    fn new(seed: u64) -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325 ^ seed)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// Finishes with an avalanche, so neighbouring inputs do not produce
    /// neighbouring hashes — FNV alone leaves the low bits weak.
    fn finish(self) -> u64 {
        let mut h = self.0;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^= h >> 33;
        h
    }
}

/// The fingerprint of a block of samples.
///
/// The rate and channel count are folded in, because the same numbers played
/// twice as fast are not the same recording.
pub fn hash_samples(samples: &[f32], sample_rate: u32, channels: u16) -> String {
    let mut hasher = Fnv::new(u64::from(sample_rate) << 16 | u64::from(channels));

    for sample in samples {
        let quantised = (sample.clamp(-1.0, 1.0) * SCALE) as i16;
        hasher.write(&quantised.to_le_bytes());
    }
    format!("{:016x}", hasher.finish())
}

/// The fingerprint of a file's audio.
///
/// Decodes the whole track, which is close to a second for a long lossless file
/// — the same cost as the waveform and the quality check, and unavoidable: the
/// point is to read what is actually there rather than what the container says
/// about it.
pub fn audio_hash(path: &Path) -> Result<String, crate::AudioError> {
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

    let mut hasher = Fnv::new(u64::from(rate) << 16 | u64::from(channels));
    let mut counted = 0u64;

    for sample in decoder {
        let quantised = (sample.clamp(-1.0, 1.0) * SCALE) as i16;
        hasher.write(&quantised.to_le_bytes());
        counted += 1;
    }

    // A file that decoded to nothing has no identity to report, and giving every
    // such file the same hash would file them all together as duplicates.
    if counted == 0 {
        return Err(crate::AudioError::Decode {
            path: path.display().to_string(),
            reason: "the file decoded to no audio".into(),
        });
    }

    Ok(format!("{:016x}", hasher.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn tone(n: usize, hz: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * hz * i as f32 / 44_100.0).sin() * 0.5)
            .collect()
    }

    #[test]
    fn the_same_samples_hash_the_same_every_time() {
        let a = hash_samples(&tone(10_000, 440.0), 44_100, 2);
        let b = hash_samples(&tone(10_000, 440.0), 44_100, 2);
        assert_eq!(a, b);
        assert_eq!(a.len(), 16, "sixteen hex digits");
    }

    #[test]
    fn different_audio_hashes_differently() {
        let a = hash_samples(&tone(10_000, 440.0), 44_100, 2);
        let b = hash_samples(&tone(10_000, 441.0), 44_100, 2);
        assert_ne!(a, b);
    }

    /// The same numbers played at another rate are another recording.
    #[test]
    fn the_rate_and_channel_count_are_part_of_the_identity() {
        let samples = tone(10_000, 440.0);
        assert_ne!(
            hash_samples(&samples, 44_100, 2),
            hash_samples(&samples, 48_000, 2)
        );
        assert_ne!(
            hash_samples(&samples, 44_100, 2),
            hash_samples(&samples, 44_100, 1)
        );
    }

    /// What quantising is actually for: the same integer sample can arrive as
    /// slightly different floats depending on how a decoder produced it, and the
    /// integer is what was in the file.
    #[test]
    fn the_integer_value_is_what_is_hashed() {
        // Two float spellings of the same sixteen-bit sample.
        let exact = vec![0.5f32; 1000];
        let same_after_quantising: Vec<f32> = exact
            .iter()
            .map(|s| ((s * SCALE) as i16) as f32 / SCALE)
            .collect();

        assert_eq!(
            hash_samples(&exact, 44_100, 2),
            hash_samples(&same_after_quantising, 44_100, 2)
        );
    }

    /// And what it is not for. This is an exact match on decoded audio: it finds
    /// copies, not things that merely sound alike. A re-encode or a normalised
    /// version is different audio and is meant to hash differently — the whole
    /// value of the answer is that it is never a guess.
    #[test]
    fn audio_that_differs_at_all_is_a_different_identity() {
        let clean = tone(10_000, 440.0);
        let quieter: Vec<f32> = clean.iter().map(|s| s * 0.98).collect();

        assert_ne!(
            hash_samples(&clean, 44_100, 2),
            hash_samples(&quieter, 44_100, 2)
        );
    }

    /// Length is part of it: one track is not another with an extra bar.
    #[test]
    fn a_longer_recording_is_a_different_one() {
        assert_ne!(
            hash_samples(&tone(10_000, 440.0), 44_100, 2),
            hash_samples(&tone(20_000, 440.0), 44_100, 2)
        );
    }

    /// Silence still hashes, and two different lengths of it differ — otherwise
    /// every quiet intro in a library would file together.
    #[test]
    fn silence_is_not_a_single_identity() {
        assert_ne!(
            hash_samples(&vec![0.0; 1000], 44_100, 2),
            hash_samples(&vec![0.0; 2000], 44_100, 2)
        );
    }
}
