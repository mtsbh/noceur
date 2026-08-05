//! Reading normalised metadata out of audio files.

use crate::key::MusicalKey;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: lofty::error::LoftyError,
    },
}

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::{Accessor, ItemKey};
use lofty::probe::Probe;

/// Everything a file claims about itself, normalised.
///
/// Every field is optional: a DJ library contains plenty of files with no tags
/// at all, and a missing field is normal, not an error.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMeta {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub comment: Option<String>,
    pub isrc: Option<String>,
    /// BPM as written in the tag.
    pub bpm: Option<f32>,
    /// Key exactly as written in the tag.
    pub key_tag: Option<String>,
    /// Parsed key, if the text was recognised.
    pub key: Option<MusicalKey>,
    /// Canonical sort value for `key`, 0..=23.
    pub key_norm: Option<i32>,
    pub duration_ms: Option<u64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bit_depth: Option<u8>,
    /// Whether the file carries embedded artwork. The bytes are not loaded here.
    pub has_artwork: bool,
}

/// Reads and normalises the tags and audio properties of one file.
///
/// Returns `Err` only when the file cannot be parsed as audio at all. A valid
/// audio file with no tags yields a `TrackMeta` with empty fields.
pub fn read(path: &Path) -> Result<TrackMeta, MetaError> {
    let tagged = Probe::open(path)
        .and_then(|p| p.read())
        .map_err(|source| MetaError::Read {
            path: path.display().to_string(),
            source,
        })?;

    let props = tagged.properties();
    let mut meta = TrackMeta {
        duration_ms: Some(props.duration().as_millis() as u64),
        sample_rate: props.sample_rate(),
        channels: props.channels(),
        bit_depth: props.bit_depth(),
        ..Default::default()
    };

    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(meta);
    };

    apply_tag(&mut meta, tag);

    Ok(meta)
}

/// Populates the tag-derived fields of `meta` from any lofty tag.
///
/// Separate from `read` so the mapping can be tested against every tag system
/// (ID3v2, Vorbis comments, MP4 ilst) without needing a real file of each
/// container format.
pub fn apply_tag(meta: &mut TrackMeta, tag: &lofty::tag::Tag) {
    meta.artist = tag.artist().map(|s| s.to_string());
    meta.title = tag.title().map(|s| s.to_string());
    meta.album = tag.album().map(|s| s.to_string());
    meta.genre = tag.genre().map(|s| s.to_string());
    meta.year = tag.year();
    meta.track_no = tag.track();
    meta.comment = tag.comment().map(|s| s.to_string());
    meta.album_artist = tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string());
    meta.isrc = tag.get_string(&ItemKey::Isrc).map(|s| s.to_string());
    meta.has_artwork = tag.picture_count() > 0;

    // ID3v2 (MP3, WAV, AIFF) and the MP4 `tmpo` atom store BPM under
    // `IntegerBpm`. Vorbis comments (FLAC, OGG) and MP4's freeform BPM atom
    // store it under `Bpm`. Try both so every container is covered.
    meta.bpm = tag
        .get_string(&ItemKey::IntegerBpm)
        .or_else(|| tag.get_string(&ItemKey::Bpm))
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0);

    if let Some(raw) = tag.get_string(&ItemKey::InitialKey) {
        let raw = raw.trim();
        if !raw.is_empty() {
            meta.key_tag = Some(raw.to_string());
            meta.key = MusicalKey::parse(raw);
            meta.key_norm = meta.key.map(|k| k.canonical());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::config::WriteOptions;
    use lofty::prelude::{Accessor, ItemKey, TagExt};
    use lofty::probe::Probe;
    use lofty::tag::{Tag, TagType};
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Writes a 0.1s silent 44.1kHz mono 16-bit WAV.
    fn write_wav(path: &PathBuf) {
        let sample_rate = 44_100u32;
        let samples = sample_rate / 10;
        let data_len = samples * 2;
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data_len).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes()); // PCM
        f.extend_from_slice(&1u16.to_le_bytes()); // mono
        f.extend_from_slice(&sample_rate.to_le_bytes());
        f.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        f.extend_from_slice(&2u16.to_le_bytes()); // block align
        f.extend_from_slice(&16u16.to_le_bytes()); // bits
        f.extend_from_slice(b"data");
        f.extend_from_slice(&data_len.to_le_bytes());
        f.extend_from_slice(&vec![0u8; data_len as usize]);
        std::fs::write(path, f).unwrap();
    }

    /// Writes a tagged WAV and returns its path.
    fn tagged(
        dir: &TempDir,
        name: &str,
        artist: &str,
        title: &str,
        bpm: &str,
        key: &str,
    ) -> PathBuf {
        let path = dir.path().join(name);
        write_wav(&path);

        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_artist(artist.to_string());
        tag.set_title(title.to_string());
        tag.set_album("So Unreleased Vol.1".to_string());
        tag.insert_text(ItemKey::IntegerBpm, bpm.to_string());
        tag.insert_text(ItemKey::InitialKey, key.to_string());
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
        path
    }

    #[test]
    fn reads_artist_title_and_album() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "G76", "it takes time", "123", "8A");
        let m = read(&p).unwrap();
        assert_eq!(m.artist.as_deref(), Some("G76"));
        assert_eq!(m.title.as_deref(), Some("it takes time"));
        assert_eq!(m.album.as_deref(), Some("So Unreleased Vol.1"));
    }

    #[test]
    fn reads_bpm_as_a_float() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "128.5", "8A");
        assert_eq!(read(&p).unwrap().bpm, Some(128.5));
    }

    #[test]
    fn normalises_the_key_and_keeps_the_original_text() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "120", "Am");
        let m = read(&p).unwrap();
        assert_eq!(m.key_tag.as_deref(), Some("Am"));
        assert_eq!(m.key, MusicalKey::parse("8A"));
        assert_eq!(m.key_norm, Some(14));
    }

    #[test]
    fn reads_audio_properties() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "120", "8A");
        let m = read(&p).unwrap();
        assert_eq!(m.sample_rate, Some(44_100));
        assert_eq!(m.channels, Some(1));
        assert!(m.duration_ms.unwrap_or(0) > 0);
    }

    #[test]
    fn untagged_files_yield_empty_fields_not_errors() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("bare.wav");
        write_wav(&p);
        let m = read(&p).unwrap();
        assert!(m.artist.is_none());
        assert!(m.bpm.is_none());
        assert!(m.key.is_none());
        assert_eq!(m.sample_rate, Some(44_100), "properties still readable");
    }

    #[test]
    fn unparseable_bpm_is_dropped_rather_than_failing() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "not a number", "8A");
        assert_eq!(read(&p).unwrap().bpm, None);
    }

    #[test]
    fn unparseable_key_keeps_the_text_but_has_no_canonical_value() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "120", "banana");
        let m = read(&p).unwrap();
        assert_eq!(m.key_tag.as_deref(), Some("banana"));
        assert_eq!(m.key, None);
        assert_eq!(m.key_norm, None);
    }

    #[test]
    fn errors_on_a_file_that_is_not_audio() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("cover.jpg");
        std::fs::write(&p, b"not audio at all").unwrap();
        assert!(read(&p).is_err());
    }

    #[test]
    fn probe_is_available_for_manual_checks() {
        // Guards against a lofty upgrade silently changing the probe API.
        let d = TempDir::new().unwrap();
        let p = d.path().join("a.wav");
        write_wav(&p);
        assert!(Probe::open(&p).unwrap().read().is_ok());
    }

    /// ID3v2 (MP3, WAV, AIFF) only maps `IntegerBpm` to a frame (`TBPM`).
    /// Writing `ItemKey::Bpm` here is silently dropped by lofty, which was
    /// the previous bug's blind spot in reverse.
    #[test]
    fn apply_tag_reads_id3v2_bpm_and_key() {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.insert_text(ItemKey::IntegerBpm, "128".to_string());
        tag.insert_text(ItemKey::InitialKey, "8A".to_string());

        let mut meta = TrackMeta::default();
        apply_tag(&mut meta, &tag);

        assert_eq!(meta.bpm, Some(128.0));
        assert_eq!(meta.key_tag.as_deref(), Some("8A"));
        assert_eq!(meta.key_norm, Some(14));
    }

    /// Vorbis comments (FLAC, OGG) only map `Bpm` to the `BPM` field, never
    /// `IntegerBpm`. This is the case the old code missed entirely.
    #[test]
    fn apply_tag_reads_vorbis_bpm_and_key() {
        let mut tag = Tag::new(TagType::VorbisComments);
        tag.insert_text(ItemKey::Bpm, "128".to_string());
        tag.insert_text(ItemKey::InitialKey, "8A".to_string());

        let mut meta = TrackMeta::default();
        apply_tag(&mut meta, &tag);

        assert_eq!(meta.bpm, Some(128.0));
        assert_eq!(meta.key_tag.as_deref(), Some("8A"));
        assert_eq!(meta.key_norm, Some(14));
    }

    /// MP4 ilst maps BPM two ways: the standard `tmpo` atom takes
    /// `IntegerBpm`, and a freeform iTunes atom takes `Bpm`. Exercise the
    /// freeform path, since that's the one the fallback exists for.
    #[test]
    fn apply_tag_reads_mp4_bpm_and_key() {
        let mut tag = Tag::new(TagType::Mp4Ilst);
        tag.insert_text(ItemKey::Bpm, "128".to_string());
        tag.insert_text(ItemKey::InitialKey, "8A".to_string());

        let mut meta = TrackMeta::default();
        apply_tag(&mut meta, &tag);

        assert_eq!(meta.bpm, Some(128.0));
        assert_eq!(meta.key_tag.as_deref(), Some("8A"));
        assert_eq!(meta.key_norm, Some(14));
    }
}
