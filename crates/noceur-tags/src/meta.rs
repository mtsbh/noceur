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
    /// The date exactly as tagged, which may be `2016-03-04` where `year` is 2016.
    pub date: Option<String>,
    pub track_no: Option<u32>,
    pub comment: Option<String>,
    pub isrc: Option<String>,
    pub label: Option<String>,
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
    /// Bitrate in kbps, as the container reports it. `None` when it will not say.
    pub bitrate_kbps: Option<u32>,
    /// What the audio is actually encoded as: `MP3`, `FLAC`, `WAV`, `AIFF`.
    ///
    /// Sniffed from the container rather than taken from the extension, so a
    /// `.mp3` that is really a WAV says so.
    pub codec: Option<String>,
    /// Whether the container stores the audio losslessly.
    ///
    /// Kept apart from the bitrate because "no bitrate" is ambiguous otherwise:
    /// a FLAC has none because there is nothing to claim, while an MP3 with a
    /// broken header has one and it is unknown. Treating those alike would
    /// accuse ordinary files of being transcodes.
    pub lossless: bool,
    /// Whether the file carries embedded artwork. The bytes are not loaded here.
    pub has_artwork: bool,
    /// Stars, 1..=5, or `None` when the file carries no rating.
    pub rating: Option<u8>,
}

/// Reads and normalises the tags and audio properties of one file.
///
/// Returns `Err` only when the file cannot be parsed as audio at all. A valid
/// audio file with no tags yields a `TrackMeta` with empty fields.
pub fn read(path: &Path) -> Result<TrackMeta, MetaError> {
    // `guess_file_type` reads the container's own first bytes; without it lofty
    // takes the extension's word for what the file is, and a `.mp3` that is
    // really a WAV fails to read at all rather than reading as a WAV. Files
    // named wrongly are ordinary in a library that has been through several
    // programs, and refusing them is not a useful answer about them.
    let fail = |source| MetaError::Read {
        path: path.display().to_string(),
        source,
    };
    let tagged = crate::riff::read(path).map_err(fail)?;

    let props = tagged.properties();
    let mut meta = TrackMeta {
        duration_ms: Some(props.duration().as_millis() as u64),
        sample_rate: props.sample_rate(),
        channels: props.channels(),
        bit_depth: props.bit_depth(),
        bitrate_kbps: props.audio_bitrate(),
        codec: codec_of(tagged.file_type()),
        lossless: is_lossless(tagged.file_type()),
        ..Default::default()
    };

    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(meta);
    };

    apply_tag(&mut meta, tag);

    // Everything but ID3 keeps the rating as ordinary text. ID3's `POPM` is kept
    // out of lofty's generic tag, so for those containers it is read directly -
    // a second look at the tag, and only for the files that need it.
    meta.rating = tag
        .get_string(&ItemKey::Popularimeter)
        .and_then(|v| v.trim().parse::<u8>().ok())
        .map(crate::rating::stars_from_byte)
        .or_else(|| {
            crate::rating::uses_id3(tagged.file_type())
                .then(|| crate::rating::read_id3(path))
                .flatten()
        })
        .filter(|&stars| stars > 0);

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
    meta.date = tag
        .get_string(&ItemKey::RecordingDate)
        .or_else(|| tag.get_string(&ItemKey::Year))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    meta.track_no = tag.track();
    meta.comment = tag.comment().map(|s| s.to_string());
    meta.album_artist = tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string());
    meta.isrc = tag.get_string(&ItemKey::Isrc).map(|s| s.to_string());
    meta.label = tag.get_string(&ItemKey::Label).map(|s| s.to_string());
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

/// Embedded cover art, exactly as the file stores it.
///
/// Undecoded on purpose: this crate knows about tags, not about images. Whoever
/// wants pixels decodes these bytes themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artwork {
    /// The container's declared MIME type, when it declares one.
    pub mime: Option<String>,
    pub data: Vec<u8>,
}

/// What a container holds, in the name anyone would use for it.
///
/// Read from the container's own bytes rather than from the extension. A file
/// named `.mp3` that is really a WAV is a thing that happens, and this is the
/// column that says so.
///
/// MP4 is reported as itself rather than guessed at: the same container holds
/// AAC and ALAC, and telling them apart means parsing the sample description
/// rather than the header lofty has already read.
pub fn codec_of(file_type: lofty::file::FileType) -> Option<String> {
    use lofty::file::FileType;
    Some(
        match file_type {
            FileType::Mpeg => "MP3",
            FileType::Flac => "FLAC",
            FileType::Wav => "WAV",
            FileType::Aiff => "AIFF",
            FileType::Mp4 => "MP4",
            FileType::Vorbis => "Vorbis",
            FileType::Opus => "Opus",
            FileType::Speex => "Speex",
            FileType::Ape => "APE",
            FileType::WavPack => "WavPack",
            FileType::Mpc => "Musepack",
            FileType::Aac => "AAC",
            _ => return None,
        }
        .to_string(),
    )
}

/// Whether a container stores its audio without loss.
///
/// By container rather than by extension: `.m4a` is ALAC or AAC depending on
/// what is inside it, and the difference is exactly the one being asked about.
pub fn is_lossless(file_type: lofty::file::FileType) -> bool {
    use lofty::file::FileType;
    matches!(
        file_type,
        FileType::Flac | FileType::Wav | FileType::Aiff | FileType::Ape | FileType::WavPack
    )
}

/// Reads the front cover out of a file, if it has one.
///
/// Separate from [`read`] because the bytes are large and almost never wanted:
/// a listing needs `has_artwork` for every row and the actual image for only the
/// handful of rows on screen.
///
/// Prefers a picture explicitly typed as the front cover, and otherwise falls
/// back to the first one present — plenty of files in a real library carry a
/// single untyped picture, and refusing to show it would be pedantic.
pub fn artwork(path: &Path) -> Result<Option<Artwork>, MetaError> {
    let tagged = crate::riff::read(path).map_err(|source| MetaError::Read {
        path: path.display().to_string(),
        source,
    })?;

    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(None);
    };

    let front = tag
        .pictures()
        .iter()
        .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
        .or_else(|| tag.pictures().first());

    Ok(front.map(|p| Artwork {
        mime: p.mime_type().map(|m| m.to_string()),
        data: p.data().to_vec(),
    }))
}

/// Reads named fields and the rating out of a file.
///
/// For quick tagging, which needs to show what a track already carries before
/// changing it. Kept apart from [`read`] because the names are the caller's
/// choice — they come from the user's own quick-tag groups — and baking them
/// into `TrackMeta` would mean this crate deciding what an energy field is
/// called.
///
/// Missing fields are simply absent rather than empty, so the caller can tell
/// "not set" from "set to nothing".
pub fn read_fields(path: &Path, names: &[String]) -> Result<Fields, MetaError> {
    let tagged = crate::riff::read(path).map_err(|source| MetaError::Read {
        path: path.display().to_string(),
        source,
    })?;

    let mut out = Fields::default();
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(out);
    };

    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        // The same resolution the writer uses, so a mood written to `TMOO` is
        // found again. Two different answers to "where does this field live"
        // would mean quick tag showing a track as untagged the moment after it
        // tagged it.
        let key = crate::edit::custom_key(tag.tag_type(), name);
        if let Some(value) = tag.get_string(&key) {
            if !value.is_empty() {
                out.values.push((name.to_string(), value.to_string()));
            }
        }
    }

    // Formats that keep the rating in an ordinary field are read here; ID3's
    // `POPM` never reaches the generic tag and is read from the file directly.
    out.rating = tag
        .get_string(&ItemKey::Popularimeter)
        .and_then(|v| v.trim().parse::<u8>().ok())
        .map(crate::rating::stars_from_byte)
        .or_else(|| crate::rating::read_id3(path));

    Ok(out)
}

/// What a file carries for the fields that were asked about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    /// Only the fields that are actually set, in the order they were asked for.
    pub values: Vec<(String, String)>,
    /// Stars, 0 to 5, when the file carries a rating.
    pub rating: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::config::WriteOptions;
    use lofty::file::FileType;
    use lofty::picture::{MimeType, Picture, PictureType};
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
        assert_eq!(m.bit_depth, Some(16), "fixture WAV is 16-bit PCM");
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

    /// Populates `album_artist`, `genre`, `year`, `track_no`, `comment` and
    /// `isrc` on a tag and asserts they all come through `apply_tag`.
    ///
    /// `year` is written via `ItemKey::RecordingDate` rather than
    /// `ItemKey::Year`: real ID3v2 (`TDRC`) and MP4 (`\u{a9}day`) tags only
    /// populate `RecordingDate`, and `Accessor::year()` (which `apply_tag`
    /// calls) already falls back to it when `Year` is absent. Vorbis
    /// comments map `RecordingDate` too (`DATE`), so this is the one
    /// ItemKey that round-trips identically across all three tag systems.
    fn assert_metadata_fields_round_trip(mut tag: Tag) {
        tag.insert_text(ItemKey::AlbumArtist, "Various Artists".to_string());
        tag.insert_text(ItemKey::Genre, "Techno".to_string());
        tag.insert_text(ItemKey::RecordingDate, "2024".to_string());
        tag.insert_text(ItemKey::TrackNumber, "7".to_string());
        tag.insert_text(ItemKey::Comment, "banger".to_string());
        tag.insert_text(ItemKey::Isrc, "USRC17607839".to_string());

        let mut meta = TrackMeta::default();
        apply_tag(&mut meta, &tag);

        assert_eq!(meta.album_artist.as_deref(), Some("Various Artists"));
        assert_eq!(meta.genre.as_deref(), Some("Techno"));
        assert_eq!(meta.year, Some(2024));
        assert_eq!(meta.track_no, Some(7));
        assert_eq!(meta.comment.as_deref(), Some("banger"));
        assert_eq!(meta.isrc.as_deref(), Some("USRC17607839"));
    }

    #[test]
    fn apply_tag_reads_id3v2_metadata_fields() {
        assert_metadata_fields_round_trip(Tag::new(TagType::Id3v2));
    }

    #[test]
    fn apply_tag_reads_vorbis_metadata_fields() {
        assert_metadata_fields_round_trip(Tag::new(TagType::VorbisComments));
    }

    #[test]
    fn apply_tag_reads_mp4_metadata_fields() {
        assert_metadata_fields_round_trip(Tag::new(TagType::Mp4Ilst));
    }

    #[test]
    fn has_artwork_reflects_picture_presence() {
        let mut tag = Tag::new(TagType::Id3v2);

        let mut meta = TrackMeta::default();
        apply_tag(&mut meta, &tag);
        assert!(!meta.has_artwork, "no picture inserted yet");

        tag.push_picture(Picture::new_unchecked(
            PictureType::CoverFront,
            Some(MimeType::Jpeg),
            None,
            vec![0xFFu8; 4],
        ));

        let mut meta = TrackMeta::default();
        apply_tag(&mut meta, &tag);
        assert!(meta.has_artwork, "picture was inserted");
    }

    /// Writes a WAV carrying the given pictures and returns its path.
    fn with_pictures(dir: &TempDir, name: &str, pics: Vec<Picture>) -> PathBuf {
        let p = dir.path().join(name);
        write_wav(&p);
        let mut tag = Tag::new(TagType::Id3v2);
        for pic in pics {
            tag.push_picture(pic);
        }
        tag.save_to_path(&p, WriteOptions::default()).unwrap();
        p
    }

    fn picture(kind: PictureType, byte: u8) -> Picture {
        Picture::new_unchecked(kind, Some(MimeType::Jpeg), None, vec![byte; 8])
    }

    #[test]
    fn artwork_is_none_when_the_file_carries_no_picture() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("bare.wav");
        write_wav(&p);
        assert_eq!(artwork(&p).unwrap(), None);
    }

    #[test]
    fn artwork_prefers_the_front_cover_over_other_pictures() {
        let d = TempDir::new().unwrap();
        let p = with_pictures(
            &d,
            "many.wav",
            vec![
                picture(PictureType::Media, 0x11),
                picture(PictureType::CoverFront, 0x22),
                picture(PictureType::CoverBack, 0x33),
            ],
        );

        let art = artwork(&p).unwrap().expect("a picture is present");
        assert_eq!(art.data, vec![0x22u8; 8], "must be the front cover");
        assert_eq!(art.mime.as_deref(), Some("image/jpeg"));
    }

    /// Plenty of real files carry one untyped picture; refusing it would leave
    /// rows blank that plainly do have art.
    #[test]
    fn artwork_falls_back_to_the_first_picture_when_none_is_a_front_cover() {
        let d = TempDir::new().unwrap();
        let p = with_pictures(
            &d,
            "untyped.wav",
            vec![
                picture(PictureType::Other, 0x44),
                picture(PictureType::CoverBack, 0x55),
            ],
        );

        let art = artwork(&p).unwrap().expect("a picture is present");
        assert_eq!(art.data, vec![0x44u8; 8]);
    }

    /// Quick tag has to show what a track already carries before changing it,
    /// so what the editor writes must read back through this — including a name
    /// like `MOOD`, which is written to ID3's real `TMOO` frame rather than to a
    /// field literally called MOOD.
    #[test]
    fn custom_fields_read_back_after_being_written() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.wav");
        write_wav(&path);

        crate::edit::write(
            &path,
            &crate::Edit {
                custom: vec![
                    ("ENERGY".into(), crate::Change::Set("5".into())),
                    ("MOOD".into(), crate::Change::Set("Dark, Rolling".into())),
                ],
                ..Default::default()
            },
        )
        .unwrap();

        let fields = read_fields(&path, &["ENERGY".into(), "MOOD".into()]).unwrap();
        assert_eq!(
            fields.values,
            vec![
                ("ENERGY".to_string(), "5".to_string()),
                ("MOOD".to_string(), "Dark, Rolling".to_string()),
            ]
        );
    }

    /// A field that is not set must be absent rather than empty, so the caller
    /// can tell "never tagged" from "tagged with nothing".
    #[test]
    fn fields_that_are_not_set_are_absent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.wav");
        write_wav(&path);

        let fields = read_fields(&path, &["ENERGY".into()]).unwrap();
        assert!(fields.values.is_empty());
    }

    /// The scale DJ software uses. Getting it wrong shows a five-star track as
    /// one star in Rekordbox, which is exactly the sort of thing nobody notices
    /// until a set.
    #[test]
    fn a_rating_round_trips_through_a_real_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.wav");
        write_wav(&path);

        for stars in 1..=5u8 {
            crate::edit::write(
                &path,
                &crate::Edit {
                    rating: crate::Change::Set(stars.to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                read_fields(&path, &[]).unwrap().rating,
                Some(stars),
                "{stars} stars did not survive"
            );
        }

        // Zero stars is the absence of a rating, not a rating of nothing —
        // which is what pressing the star you already gave a track means.
        crate::edit::write(
            &path,
            &crate::Edit {
                rating: crate::Change::Set("0".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(read_fields(&path, &[]).unwrap().rating, None);
    }

    /// A rating written here has to be the one Rekordbox reads: five even steps
    /// of 51, not the media-player scale.
    #[test]
    fn stars_are_stored_on_the_scale_dj_software_reads() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.wav");
        write_wav(&path);

        crate::edit::write(
            &path,
            &crate::Edit {
                rating: crate::Change::Set("4".into()),
                ..Default::default()
            },
        )
        .unwrap();

        // A WAV keeps its tags in ID3, so the rating goes into a real `POPM`
        // frame on the byte scale DJ software reads.
        assert_eq!(crate::rating::read_id3(&path), Some(4));
        assert_eq!(
            crate::rating::byte_from_stars(4),
            204,
            "four stars is 4 x 51"
        );
    }

    #[test]
    fn a_rating_can_be_cleared() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.wav");
        write_wav(&path);

        crate::edit::write(
            &path,
            &crate::Edit {
                rating: crate::Change::Set("5".into()),
                ..Default::default()
            },
        )
        .unwrap();
        crate::edit::write(
            &path,
            &crate::Edit {
                rating: crate::Change::Clear,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(read_fields(&path, &[]).unwrap().rating, None);
    }

    /// Ratings arrive on several scales depending on what wrote them.
    #[test]
    fn every_rating_scale_in_circulation_reads_sensibly() {
        // Stars written directly, as Vorbis comments often hold them.
        assert_eq!(crate::rating::stars_from_byte(4), 4);
        // The DJ scale: 51 per star.
        for (raw, stars) in [(51, 1), (102, 2), (153, 3), (204, 4), (255, 5)] {
            assert_eq!(
                crate::rating::stars_from_byte(raw),
                stars,
                "{raw} should be {stars}"
            );
        }
        // Windows Media Player's scale agrees from three stars up. Its two-star
        // 64 reads as one here, which is the documented cost of following the
        // scale this app actually writes.
        for (raw, stars) in [(128, 3), (196, 4), (255, 5)] {
            assert_eq!(
                crate::rating::stars_from_byte(raw),
                stars,
                "{raw} should be {stars}"
            );
        }
        assert_eq!(crate::rating::stars_from_byte(0), 0);
    }

    /// Writing a rating must not disturb the rest of the tag: the ID3 path
    /// converts the whole tag to its format-specific form to add one frame, and
    /// dropping everything else on the way would be a quiet disaster.
    #[test]
    fn a_rating_does_not_cost_the_other_tags() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.wav");
        write_wav(&path);

        crate::edit::write(
            &path,
            &crate::Edit {
                artist: crate::Change::Set("Skee Mask".into()),
                title: crate::Change::Set("Rev8617".into()),
                bpm: crate::Change::Set("143".into()),
                ..Default::default()
            },
        )
        .unwrap();
        crate::edit::write(
            &path,
            &crate::Edit {
                rating: crate::Change::Set("5".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let meta = read(&path).unwrap();
        assert_eq!(meta.artist.as_deref(), Some("Skee Mask"));
        assert_eq!(meta.title.as_deref(), Some("Rev8617"));
        assert_eq!(meta.bpm, Some(143.0));
        assert_eq!(read_fields(&path, &[]).unwrap().rating, Some(5));
    }

    /// The column this exists for was empty in every library for months: the
    /// schema declared it and nothing ever wrote to it.
    #[test]
    fn every_container_worth_naming_has_a_name() {
        for (kind, name) in [
            (FileType::Mpeg, "MP3"),
            (FileType::Flac, "FLAC"),
            (FileType::Wav, "WAV"),
            (FileType::Aiff, "AIFF"),
            (FileType::Vorbis, "Vorbis"),
            (FileType::Ape, "APE"),
        ] {
            assert_eq!(codec_of(kind).as_deref(), Some(name), "{kind:?}");
        }
    }

    /// Read from the bytes rather than the extension, which is the only reason
    /// it is worth a column at all: a file named `.mp3` that is really a WAV
    /// says WAV.
    #[test]
    fn the_codec_comes_from_the_container_not_the_name() {
        let dir = TempDir::new().unwrap();
        let lying = dir.path().join("actually-a-wav.mp3");
        write_wav(&lying);

        let meta = read(&lying).unwrap();
        assert_eq!(meta.codec.as_deref(), Some("WAV"));
    }

    /// MP4 holds AAC and ALAC alike, and saying which would mean parsing more
    /// than the header. Naming the container is honest; guessing is not.
    #[test]
    fn mp4_is_named_as_the_container_it_is() {
        assert_eq!(codec_of(FileType::Mp4).as_deref(), Some("MP4"));
    }
}
