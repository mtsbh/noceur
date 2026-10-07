//! Writing tags back to audio files.
//!
//! Kept apart from [`crate::meta`] because the risk is not the same. Reading a
//! file that turns out to be nonsense costs a row of dashes; writing to one
//! rewrites something in the user's library that no undo stack covers. Every
//! decision here leans the same way: change what was asked for, leave everything
//! else exactly as it was, and refuse rather than guess.

use crate::meta::MetaError;
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::Tag;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error(transparent)]
    Meta(#[from] MetaError),
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: lofty::error::LoftyError,
    },
    #[error("cannot save {path}: {source}")]
    Save {
        path: String,
        #[source]
        source: lofty::error::LoftyError,
    },
    #[error("{field} must be a number, got {value:?}")]
    NotANumber { field: &'static str, value: String },
    #[error("{field} is out of range: {value}")]
    OutOfRange { field: &'static str, value: String },
    #[error("what came back for {path} is not an image this can embed")]
    NotAnImage { path: String },
    #[error(
        "{name:?} cannot be a field name: ID3 reads any four capitals as a frame          identifier. Use a longer name."
    )]
    BadFieldName { name: String },
}

/// What to do with one field.
///
/// Three states rather than `Option<String>`, because editing several files at
/// once needs all three and conflating any two of them loses data. A field left
/// alone must stay as it is in each file — which is not the same as writing the
/// same value to all of them, and not the same as emptying it. The multi-select
/// editor shows a blank box for fields that disagree, and that blank has to mean
/// "leave each file's own value" rather than "erase them all".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Change {
    /// Leave whatever the file already has.
    #[default]
    Keep,
    /// Write this value.
    Set(String),
    /// Remove the field.
    Clear,
}

impl Change {
    /// Builds a change from a UI field: `None` means untouched, `Some("")`
    /// means the user emptied the box, which is a request to clear the tag.
    pub fn from_input(value: Option<String>) -> Change {
        match value {
            None => Change::Keep,
            Some(v) if v.trim().is_empty() => Change::Clear,
            Some(v) => Change::Set(v.trim().to_string()),
        }
    }
}

/// The edits to apply to a file.
///
/// Every field defaults to [`Change::Keep`], so a caller constructs one with
/// `..Default::default()` and names only what it means to change. Adding a field
/// here cannot silently start clearing tags in files nobody meant to touch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    pub artist: Change,
    pub title: Change,
    pub album: Change,
    pub album_artist: Change,
    pub genre: Change,
    pub comment: Change,
    pub isrc: Change,
    /// The record label: `TPUB` in ID3, `LABEL` in Vorbis comments.
    pub label: Change,
    /// Four digits. Written as the tag's year field.
    pub year: Change,
    pub track_no: Change,
    /// Beats per minute. Written to both the integer and freeform keys, so the
    /// value survives whichever one the next program reads.
    pub bpm: Change,
    /// Musical key, as text. Stored verbatim — this crate normalises keys for
    /// display and sorting, but what goes in the file is what the user typed.
    pub key: Change,
    /// Stars, `"0"` to `"5"`. ID3 gets a real `POPM` frame on the scale DJ
    /// software uses; every other format gets the star count as text.
    pub rating: Change,
    /// Named fields this crate does not model: `ENERGY`, `MOOD`, whatever a
    /// quick-tag group is pointed at. Written as a user-defined text frame in
    /// ID3 and as a plain comment key elsewhere, which is where every other tag
    /// editor looks for them.
    pub custom: Vec<(String, Change)>,
    /// Cuts a full date such as `2016-03-04` down to its year. Separate from
    /// `year` because setting the year keeps the month and day after it.
    pub date_to_year: bool,
}

impl Edit {
    /// Whether this would change anything at all.
    ///
    /// Checked before opening the file: a save with nothing in it should not
    /// rewrite a perfectly good file, bump its mtime and cost it its cached
    /// artwork and waveform for no reason.
    pub fn is_empty(&self) -> bool {
        *self == Edit::default()
    }
}

/// Applies `edit` to the file at `path`.
///
/// Edits the file's existing tag in place rather than building a new one, so
/// everything not named in the edit — artwork, replay gain, comments this crate
/// does not model, whatever a DJ program wrote for its own use — survives
/// untouched. A file with no tag at all gets one of whatever type its container
/// prefers.
///
/// Numbers are parsed before anything is written, so a typo in the year cannot
/// leave the file half-edited.
pub fn write(path: &Path, edit: &Edit) -> Result<(), EditError> {
    if edit.is_empty() {
        return Ok(());
    }

    // Parse first, write second. Everything that can be rejected is rejected
    // before the file is opened for writing.
    let year = number(&edit.year, "year")?;
    let track_no = number(&edit.track_no, "track number")?;
    let bpm = decimal(&edit.bpm, "BPM")?;

    let mut tagged = crate::riff::read(path).map_err(|source| EditError::Read {
        path: path.display().to_string(),
        source,
    })?;

    // The container's own preferred tag type, so an edit to a FLAC writes Vorbis
    // comments and an edit to an MP3 writes ID3v2 — rather than bolting a
    // foreign tag onto the file that half the world would then ignore.
    let tag_type = tagged.primary_tag_type();

    // Checked before anything is written, like the numbers above: a field name
    // ID3v2 cannot represent should be refused with a reason rather than
    // failing halfway through the encoder.
    if tag_type == lofty::tag::TagType::Id3v2 {
        for (name, change) in &edit.custom {
            let name = name.trim();
            if matches!(change, Change::Set(_))
                && looks_like_a_frame_id(name)
                && matches!(custom_key(tag_type, name), ItemKey::Unknown(_))
            {
                return Err(EditError::BadFieldName {
                    name: name.to_string(),
                });
            }
        }
    }

    if tagged.tag(tag_type).is_none() {
        tagged.insert_tag(Tag::new(tag_type));
    }
    let Some(tag) = tagged.tag_mut(tag_type) else {
        // Unreachable in practice: the tag was just inserted.
        return Ok(());
    };

    apply(tag, edit, year, track_no, bpm);

    tag.save_to_path(path, WriteOptions::default())
        .map_err(|source| EditError::Save {
            path: path.display().to_string(),
            source,
        })?;

    // The rating, for the containers that keep it in a frame the generic tag
    // cannot carry. A second pass over the file, but only when a rating is
    // actually being changed — which in practice means quick tagging, where it
    // is the only thing being changed.
    if !matches!(edit.rating, Change::Keep) {
        let stars = match &edit.rating {
            Change::Set(v) => v.trim().parse::<u8>().ok().filter(|n| *n <= 5),
            _ => None,
        };
        crate::rating::write_id3(path, stars).map_err(|source| EditError::Save {
            path: path.display().to_string(),
            source,
        })?;
    }

    Ok(())
}

/// Applies an edit to a tag in memory.
///
/// Split out so the whole field mapping can be tested against every tag system
/// without writing a file of each container format — the same reason
/// [`crate::meta::apply_tag`] exists on the reading side.
pub fn apply(
    tag: &mut Tag,
    edit: &Edit,
    year: Option<Option<u32>>,
    track_no: Option<Option<u32>>,
    bpm: Option<Option<f64>>,
) {
    // Every format except ID3 keeps the rating in an ordinary text field, so it
    // goes in with everything else. ID3's binary frame is handled by `write`,
    // which is the only place that has a file to write it to.
    // A rating is stored differently by every format, and lofty models that
    // difference by what the value *is* rather than by which key it is under:
    // ID3 holds a `POPM` structure, which arrives and leaves as raw bytes, while
    // everything else holds a plain number.
    //
    // Zero stars removes the rating rather than storing a zero. That is what
    // pressing the star a track already has means, and a rating of nothing is
    // not something the programs reading it agree about.
    match &edit.rating {
        Change::Keep => {}
        Change::Clear => tag.remove_key(&ItemKey::Popularimeter),
        Change::Set(stars) => match stars.trim().parse::<u8>() {
            Ok(0) => tag.remove_key(&ItemKey::Popularimeter),
            // ID3 is handled after the save, natively: its `POPM` frame is one
            // lofty keeps out of the format-independent tag entirely, so it can
            // neither be read nor written from here.
            Ok(n) if n <= 5 && tag.tag_type() != lofty::tag::TagType::Id3v2 => {
                // Stars as written, not a 0-255 byte. `RATING` in a Vorbis
                // comment is read as a small number by the programs that look
                // at it, and 255 there reads as wildly out of range.
                tag.insert_text(ItemKey::Popularimeter, n.to_string());
            }
            _ => {}
        },
    }

    for (name, change) in &edit.custom {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let key = custom_key(tag.tag_type(), name);
        match change {
            Change::Keep => {}
            Change::Clear => tag.remove_key(&key),
            Change::Set(value) => {
                let item = lofty::tag::TagItem::new(
                    key.clone(),
                    lofty::tag::ItemValue::Text(value.clone()),
                );
                if matches!(key, ItemKey::Unknown(_)) {
                    // `insert` verifies the key maps to a frame the container
                    // knows and silently refuses when it does not, which is
                    // every genuinely custom field. The unchecked form is what
                    // lofty documents for exactly this case.
                    tag.insert_unchecked(item);
                } else {
                    tag.insert(item);
                }
            }
        }
    }

    text(
        tag,
        &edit.artist,
        |t, v| t.set_artist(v),
        |t| t.remove_artist(),
    );
    text(
        tag,
        &edit.title,
        |t, v| t.set_title(v),
        |t| t.remove_title(),
    );
    text(
        tag,
        &edit.album,
        |t, v| t.set_album(v),
        |t| t.remove_album(),
    );
    text(
        tag,
        &edit.genre,
        |t, v| t.set_genre(v),
        |t| t.remove_genre(),
    );
    text(
        tag,
        &edit.comment,
        |t, v| t.set_comment(v),
        |t| t.remove_comment(),
    );
    item(tag, &edit.album_artist, ItemKey::AlbumArtist);
    item(tag, &edit.isrc, ItemKey::Isrc);
    item(tag, &edit.label, ItemKey::Label);
    item(tag, &edit.key, ItemKey::InitialKey);

    match year {
        Some(Some(v)) => tag.set_year(v),
        Some(None) => tag.remove_year(),
        None => {}
    }
    if edit.date_to_year {
        if let Some(y) = tag.year() {
            tag.remove_year();
            tag.set_year(y);
        }
    }
    match track_no {
        Some(Some(v)) => tag.set_track(v),
        Some(None) => tag.remove_track(),
        None => {}
    }

    // Written under both keys, and read back from either. ID3v2 and MP4's `tmpo`
    // atom use the integer key; Vorbis comments use the freeform one. A file
    // edited here should read correctly in a program that only looks at one.
    //
    // The exact value goes to both, including its decimals — despite the name,
    // the integer key is a text frame, every DJ program writes fractional BPMs
    // into it, and rounding here would silently discard what the user typed the
    // moment anything read that key first. Which our own reader does.
    match bpm {
        Some(Some(v)) => {
            tag.insert_text(ItemKey::IntegerBpm, trim_decimal(v));
            tag.insert_text(ItemKey::Bpm, trim_decimal(v));
        }
        Some(None) => {
            tag.remove_key(&ItemKey::IntegerBpm);
            tag.remove_key(&ItemKey::Bpm);
        }
        None => {}
    }
}

/// Applies a change through an accessor pair.
fn text(tag: &mut Tag, change: &Change, set: impl Fn(&mut Tag, String), clear: impl Fn(&mut Tag)) {
    match change {
        Change::Keep => {}
        Change::Set(v) => set(tag, v.clone()),
        Change::Clear => clear(tag),
    }
}

/// Applies a change to a field lofty has no named accessor for.
fn item(tag: &mut Tag, change: &Change, key: ItemKey) {
    match change {
        Change::Keep => {}
        Change::Set(v) => {
            tag.insert_text(key, v.clone());
        }
        Change::Clear => {
            tag.remove_key(&key);
        }
    }
}

/// `None` to leave alone, `Some(None)` to clear, `Some(Some(n))` to set.
fn number(change: &Change, field: &'static str) -> Result<Option<Option<u32>>, EditError> {
    match change {
        Change::Keep => Ok(None),
        Change::Clear => Ok(Some(None)),
        Change::Set(v) => {
            let n = v.trim().parse::<u32>().map_err(|_| EditError::NotANumber {
                field,
                value: v.clone(),
            })?;
            Ok(Some(Some(n)))
        }
    }
}

/// The same, for a value that may carry decimals.
fn decimal(change: &Change, field: &'static str) -> Result<Option<Option<f64>>, EditError> {
    match change {
        Change::Keep => Ok(None),
        Change::Clear => Ok(Some(None)),
        Change::Set(v) => {
            let n =
                v.trim()
                    .replace(',', ".")
                    .parse::<f64>()
                    .map_err(|_| EditError::NotANumber {
                        field,
                        value: v.clone(),
                    })?;
            if !n.is_finite() || n <= 0.0 || n > 1000.0 {
                return Err(EditError::OutOfRange {
                    field,
                    value: v.clone(),
                });
            }
            Ok(Some(Some(n)))
        }
    }
}

/// The key a named field should be written under.
///
/// Prefers a real mapping when one exists: `MOOD` belongs in ID3v2's `TMOO`
/// frame and MP4's iTunes mood atom, not in a user-defined field that only this
/// app would look in.
///
/// Looked up in the Vorbis namespace first, whatever is being written. Vorbis
/// comments are the one format whose keys are the plain readable words people
/// actually type — `MOOD`, `GENRE`, `LABEL` — where ID3 calls the same things
/// `TMOO`, `TCON` and `TPUB`. So a name typed by a person is far more likely to
/// be a Vorbis key than an ID3 frame identifier, and looking there first is what
/// turns `MOOD` into the mood field rather than a field called MOOD.
pub fn custom_key(tag_type: lofty::tag::TagType, name: &str) -> ItemKey {
    let by_readable_name = ItemKey::from_key(lofty::tag::TagType::VorbisComments, name);
    if !matches!(by_readable_name, ItemKey::Unknown(_)) {
        return by_readable_name;
    }
    let key = ItemKey::from_key(tag_type, name);
    // MP4 has no plain named fields: an atom is four bytes, and anything else
    // lives in a freeform atom under a reverse-DNS mean. iTunes' own namespace
    // is the one foobar2000, Mp3tag and every DJ program read, and without it
    // the field is dropped on save because it is not a valid atom.
    if tag_type == lofty::tag::TagType::Mp4Ilst && !name.starts_with("----") {
        if let ItemKey::Unknown(_) = key {
            return ItemKey::Unknown(format!("----:com.apple.iTunes:{name}"));
        }
    }
    key
}

/// Whether ID3v2 would mistake a field name for a frame identifier.
///
/// ID3v2 frames are four characters. A user-defined field named `VIBE` is
/// indistinguishable from a frame called `VIBE`, and since no such frame exists
/// the write fails deep inside the encoder with `BadFrame`. Catching it here
/// turns that into something a person can act on.
fn looks_like_a_frame_id(name: &str) -> bool {
    name.len() == 4
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Formats a BPM without a trailing `.0`, so a whole number stays whole in the
/// file — plenty of programs display the tag verbatim.
fn trim_decimal(v: f64) -> String {
    if (v.fract()).abs() < f64::EPSILON {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Replaces a file's front cover.
///
/// The existing front cover is removed before the new one is added, rather than
/// appended alongside it: a file carrying two front covers shows whichever one
/// the reading program happens to reach first, which makes the change look like
/// it silently failed. Other picture types — a back cover, a liner note — are
/// left alone, because nothing about choosing a front cover says anything about
/// them.
///
/// The MIME type is sniffed from the bytes rather than taken on trust. It gets
/// written into the tag, and a picture labelled as something it is not is a
/// picture some programs refuse to draw.
pub fn set_front_cover(path: &Path, image: &[u8]) -> Result<(), EditError> {
    let mime = sniff_mime(image).ok_or_else(|| EditError::NotAnImage {
        path: path.display().to_string(),
    })?;

    let mut tagged = crate::riff::read(path).map_err(|source| EditError::Read {
        path: path.display().to_string(),
        source,
    })?;

    let tag_type = tagged.primary_tag_type();
    if tagged.tag(tag_type).is_none() {
        tagged.insert_tag(Tag::new(tag_type));
    }
    let Some(tag) = tagged.tag_mut(tag_type) else {
        return Ok(());
    };

    tag.remove_picture_type(PictureType::CoverFront);
    tag.push_picture(Picture::new_unchecked(
        PictureType::CoverFront,
        Some(mime),
        None,
        image.to_vec(),
    ));

    tag.save_to_path(path, WriteOptions::default())
        .map_err(|source| EditError::Save {
            path: path.display().to_string(),
            source,
        })
}

/// Removes every picture from a file.
///
/// All of them, not only the one typed as the front cover. A file that shows the
/// wrong artwork usually carries two — an old cover some program left typed as
/// "other", and the one on top of it — and removing only the front one makes the
/// other reappear, which reads as the removal having failed.
///
/// A file with no pictures is left completely alone rather than rewritten: this
/// runs across a selection, and rewriting an untouched file would cost it its
/// cached thumbnail and waveform for no change.
pub fn remove_pictures(path: &Path) -> Result<(), EditError> {
    let mut tagged = crate::riff::read(path).map_err(|source| EditError::Read {
        path: path.display().to_string(),
        source,
    })?;

    let tag_type = tagged.primary_tag_type();
    let Some(tag) = tagged.tag_mut(tag_type) else {
        return Ok(());
    };
    if tag.pictures().is_empty() {
        return Ok(());
    }

    while !tag.pictures().is_empty() {
        tag.remove_picture(0);
    }

    tag.save_to_path(path, WriteOptions::default())
        .map_err(|source| EditError::Save {
            path: path.display().to_string(),
            source,
        })
}

/// Identifies an image from its first bytes.
///
/// Only the formats that actually turn up as cover art. Anything else is
/// refused rather than stored as "unknown": a tag full of bytes no reader can
/// identify is worse than no artwork.
fn sniff_mime(bytes: &[u8]) -> Option<MimeType> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(MimeType::Jpeg)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(MimeType::Png)
    } else if bytes.starts_with(b"GIF8") {
        Some(MimeType::Gif)
    } else if bytes.starts_with(b"BM") {
        Some(MimeType::Bmp)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::{apply_tag, TrackMeta};
    use lofty::tag::TagType;

    /// Runs an edit through a tag and reads the result back the way the app
    /// does, so the two halves are checked against each other rather than
    /// against my idea of what lofty stores.
    fn round_trip(tag: &mut Tag, edit: &Edit) -> TrackMeta {
        let year = number(&edit.year, "year").unwrap();
        let track = number(&edit.track_no, "track number").unwrap();
        let bpm = decimal(&edit.bpm, "BPM").unwrap();
        apply(tag, edit, year, track, bpm);

        let mut meta = TrackMeta::default();
        apply_tag(&mut meta, tag);
        meta
    }

    fn tag() -> Tag {
        Tag::new(TagType::Id3v2)
    }

    fn write_wav(path: &std::path::PathBuf) {
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

    #[test]
    fn a_full_date_can_be_cut_to_its_year() {
        for kind in [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst] {
            let mut t = Tag::new(kind);
            t.insert_text(ItemKey::RecordingDate, "2016-03-04".into());
            let meta = round_trip(
                &mut t,
                &Edit {
                    date_to_year: true,
                    ..Default::default()
                },
            );
            assert_eq!(meta.date.as_deref(), Some("2016"), "{kind:?}");
            assert_eq!(meta.year, Some(2016), "{kind:?}");
        }
    }

    #[test]
    fn setting_a_field_writes_it() {
        let mut t = tag();
        let meta = round_trip(
            &mut t,
            &Edit {
                artist: Change::Set("Akiko Kiyama".into()),
                title: Change::Set("Rabbits".into()),
                ..Default::default()
            },
        );
        assert_eq!(meta.artist.as_deref(), Some("Akiko Kiyama"));
        assert_eq!(meta.title.as_deref(), Some("Rabbits"));
    }

    /// The whole reason `Change` has three states: a field nobody touched must
    /// come out unchanged, not blanked.
    #[test]
    fn keeping_a_field_leaves_it_alone() {
        let mut t = tag();
        t.set_artist("Original".into());
        let meta = round_trip(
            &mut t,
            &Edit {
                title: Change::Set("New".into()),
                ..Default::default()
            },
        );
        assert_eq!(meta.artist.as_deref(), Some("Original"));
    }

    #[test]
    fn clearing_a_field_removes_it() {
        let mut t = tag();
        t.set_artist("Original".into());
        let meta = round_trip(
            &mut t,
            &Edit {
                artist: Change::Clear,
                ..Default::default()
            },
        );
        assert_eq!(meta.artist, None);
    }

    /// BPM lives under a different key in each tag system, so what matters is
    /// that it survives a write and a read in every one of them — not which key
    /// lofty happened to put it under.
    #[test]
    fn bpm_round_trips_in_every_tag_system() {
        for kind in [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst] {
            let mut t = Tag::new(kind);
            let meta = round_trip(
                &mut t,
                &Edit {
                    bpm: Change::Set("128".into()),
                    ..Default::default()
                },
            );
            assert_eq!(meta.bpm, Some(128.0), "{kind:?} lost its BPM");
        }
    }

    /// A DJ library is full of fractional BPMs, and the key our own reader looks
    /// at first is named "integer" — rounding on the way in would throw away
    /// what the user typed before anything ever read it back.
    #[test]
    fn a_fractional_bpm_keeps_its_decimals() {
        for kind in [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst] {
            let mut t = Tag::new(kind);
            let meta = round_trip(
                &mut t,
                &Edit {
                    bpm: Change::Set("127.5".into()),
                    ..Default::default()
                },
            );
            assert_eq!(meta.bpm, Some(127.5), "{kind:?} rounded the BPM");
        }
    }

    /// European keyboards produce a comma, and refusing it would be pedantry.
    #[test]
    fn a_comma_decimal_is_accepted() {
        let mut t = tag();
        let meta = round_trip(
            &mut t,
            &Edit {
                bpm: Change::Set("127,5".into()),
                ..Default::default()
            },
        );
        assert_eq!(meta.bpm, Some(127.5));
    }

    #[test]
    fn a_written_key_is_parsed_back_into_its_canonical_form() {
        let mut t = tag();
        let meta = round_trip(
            &mut t,
            &Edit {
                key: Change::Set("8A".into()),
                ..Default::default()
            },
        );
        assert_eq!(meta.key_tag.as_deref(), Some("8A"));
        assert!(meta.key_norm.is_some(), "8A must parse as a Camelot key");
    }

    #[test]
    fn year_and_track_round_trip() {
        let mut t = tag();
        let meta = round_trip(
            &mut t,
            &Edit {
                year: Change::Set("2019".into()),
                track_no: Change::Set("4".into()),
                ..Default::default()
            },
        );
        assert_eq!(meta.year, Some(2019));
        assert_eq!(meta.track_no, Some(4));
    }

    /// Rejected before the file is opened, so a typo cannot leave a file with
    /// half an edit applied to it.
    #[test]
    fn a_year_that_is_not_a_number_is_refused() {
        let err = number(&Change::Set("nineteen".into()), "year").unwrap_err();
        assert!(matches!(err, EditError::NotANumber { .. }));
    }

    #[test]
    fn an_absurd_bpm_is_refused() {
        assert!(matches!(
            decimal(&Change::Set("0".into()), "BPM").unwrap_err(),
            EditError::OutOfRange { .. }
        ));
        assert!(matches!(
            decimal(&Change::Set("99999".into()), "BPM").unwrap_err(),
            EditError::OutOfRange { .. }
        ));
    }

    /// An edit with nothing in it must not rewrite the file: that would bump its
    /// mtime and cost it its cached artwork and waveform for no change at all.
    #[test]
    fn an_empty_edit_is_empty() {
        assert!(Edit::default().is_empty());
        assert!(!Edit {
            artist: Change::Clear,
            ..Default::default()
        }
        .is_empty());
    }

    /// Everything above works on a tag in memory. This is the one that touches a
    /// real file, because that is where the risk actually is: lofty has to rewrite
    /// the container around the tag, and a file that comes back unreadable is a
    /// track lost out of someone's library.
    #[test]
    fn an_edit_survives_a_round_trip_through_a_real_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("track.wav");
        write_wav(&path);

        write(
            &path,
            &Edit {
                artist: Change::Set("Afriqua".into()),
                title: Change::Set("Cheep Spouse".into()),
                bpm: Change::Set("125".into()),
                key: Change::Set("4A".into()),
                ..Default::default()
            },
        )
        .expect("writing tags to a plain WAV must work");

        let meta = crate::meta::read(&path).expect("the file must still be readable");
        assert_eq!(meta.artist.as_deref(), Some("Afriqua"));
        assert_eq!(meta.title.as_deref(), Some("Cheep Spouse"));
        assert_eq!(meta.bpm, Some(125.0));
        assert_eq!(meta.key_tag.as_deref(), Some("4A"));

        // A second edit must not disturb the first: this is the multi-pass case,
        // where someone fixes the artist now and the key later.
        write(
            &path,
            &Edit {
                album: Change::Set("Cheep".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let meta = crate::meta::read(&path).unwrap();
        assert_eq!(meta.album.as_deref(), Some("Cheep"));
        assert_eq!(
            meta.artist.as_deref(),
            Some("Afriqua"),
            "the earlier edit must survive the later one"
        );
    }

    /// An empty edit must not rewrite the file at all — checked by watching the
    /// bytes, since the cost of a needless rewrite is a changed mtime and a lost
    /// waveform cache rather than a wrong tag.
    #[test]
    fn an_empty_edit_does_not_touch_the_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("track.wav");
        write_wav(&path);
        let before = std::fs::read(&path).unwrap();

        write(&path, &Edit::default()).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    /// A minimal but real JPEG, so the picture path can be exercised without a
    /// fixture file: the SOI marker is what identifies it, and lofty stores the
    /// bytes rather than decoding them.
    fn jpeg() -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0];
        v.extend_from_slice(&[0u8; 64]);
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    #[test]
    fn a_front_cover_can_be_embedded_and_read_back() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("track.wav");
        write_wav(&path);

        set_front_cover(&path, &jpeg()).expect("embedding a JPEG must work");

        let meta = crate::meta::read(&path).unwrap();
        assert!(meta.has_artwork, "the file must now report artwork");
        let art = crate::meta::artwork(&path).unwrap().expect("a picture");
        assert_eq!(art.data, jpeg());
        assert_eq!(art.mime.as_deref(), Some("image/jpeg"));
    }

    /// Two front covers in one file means whichever the reader reaches first
    /// wins, which makes a change look as though it did nothing.
    #[test]
    fn replacing_a_cover_leaves_exactly_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("track.wav");
        write_wav(&path);

        set_front_cover(&path, &jpeg()).unwrap();
        // The PNG signature, spelled out: 137 P N G CR LF SUB LF.
        let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0u8; 32]);
        set_front_cover(&path, &png).unwrap();

        let art = crate::meta::artwork(&path).unwrap().expect("a picture");
        assert_eq!(
            art.mime.as_deref(),
            Some("image/png"),
            "the newer cover wins"
        );
        assert_eq!(art.data, png);
    }

    /// A tag full of bytes no reader can identify is worse than no artwork, and
    /// an HTML error page dressed up as a cover is exactly what a failed
    /// download looks like.
    #[test]
    fn something_that_is_not_an_image_is_refused() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("track.wav");
        write_wav(&path);

        let err = set_front_cover(&path, b"<html>404</html>").unwrap_err();
        assert!(matches!(err, EditError::NotAnImage { .. }));
        assert!(
            !crate::meta::read(&path).unwrap().has_artwork,
            "a refused cover must not have been written"
        );
    }

    /// Quick tag writes named fields this crate does not model. They have to
    /// survive a round trip through whatever the container uses — a TXXX frame
    /// in ID3, a plain key in Vorbis — or the values vanish the moment anything
    /// reads the file back.
    #[test]
    fn a_custom_field_round_trips_in_every_tag_system() {
        for kind in [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst] {
            let mut t = Tag::new(kind);
            round_trip(
                &mut t,
                &Edit {
                    custom: vec![("ENERGY".into(), Change::Set("4".into()))],
                    ..Default::default()
                },
            );
            // Looked up the way the reader does: MP4 keeps it in a freeform
            // atom, whose key is not the bare name.
            assert_eq!(
                t.get_string(&custom_key(kind, "ENERGY")),
                Some("4"),
                "{kind:?} lost its custom field"
            );
        }
    }

    /// A quick-tag row pointed at Composer or Lyricist fills the real field -
    /// `TCOM` and `TEXT` in ID3, `COMPOSER` and `LYRICIST` in Vorbis - which is
    /// what rekordbox reads from an AIFF, not a user-defined one it ignores.
    #[test]
    fn composer_and_lyricist_go_to_their_own_fields() {
        for kind in [TagType::Id3v2, TagType::VorbisComments] {
            let mut t = Tag::new(kind);
            round_trip(
                &mut t,
                &Edit {
                    custom: vec![
                        ("COMPOSER".into(), Change::Set("peak".into())),
                        ("LYRICIST".into(), Change::Set("vocal".into())),
                    ],
                    ..Default::default()
                },
            );
            assert_eq!(t.get_string(&ItemKey::Composer), Some("peak"), "{kind:?}");
            assert_eq!(t.get_string(&ItemKey::Lyricist), Some("vocal"), "{kind:?}");
        }
    }

    /// And on a real file, read back the way quick tag reads it.
    #[test]
    fn composer_survives_a_real_wav() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.wav");
        write_wav(&path);
        write(
            &path,
            &Edit {
                custom: vec![("COMPOSER".into(), Change::Set("peak".into()))],
                ..Default::default()
            },
        )
        .unwrap();
        let f = crate::meta::read_fields(&path, &["COMPOSER".into()]).unwrap();
        assert_eq!(f.values, vec![("COMPOSER".to_string(), "peak".to_string())]);
    }

    /// The label goes where DJ software looks for it — `TPUB` in ID3, `LABEL`
    /// in Vorbis comments — and is read back as the label, not as a comment.
    #[test]
    fn the_label_round_trips_in_every_tag_system() {
        for kind in [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst] {
            let mut t = Tag::new(kind);
            round_trip(
                &mut t,
                &Edit {
                    label: Change::Set("Orac Records".into()),
                    ..Default::default()
                },
            );
            let mut meta = crate::TrackMeta::default();
            crate::meta::apply_tag(&mut meta, &t);
            assert_eq!(meta.label.as_deref(), Some("Orac Records"), "{kind:?}");
            assert_eq!(meta.comment, None, "{kind:?} put the label in the comment");
        }
    }

    /// Written through `custom`, ReplayGain lands in the field every player
    /// reads it from: `TXXX:REPLAYGAIN_TRACK_GAIN` in ID3, the plain key in
    /// Vorbis comments, the iTunes freeform atom in MP4.
    #[test]
    fn replay_gain_round_trips_in_every_tag_system() {
        for kind in [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst] {
            let mut t = Tag::new(kind);
            round_trip(
                &mut t,
                &Edit {
                    custom: vec![
                        (
                            "REPLAYGAIN_TRACK_GAIN".into(),
                            Change::Set("-8.50 dB".into()),
                        ),
                        (
                            "REPLAYGAIN_TRACK_PEAK".into(),
                            Change::Set("0.988553".into()),
                        ),
                    ],
                    ..Default::default()
                },
            );
            assert_eq!(
                t.get_string(&ItemKey::ReplayGainTrackGain),
                Some("-8.50 dB"),
                "{kind:?}"
            );
            assert_eq!(
                t.get_string(&ItemKey::ReplayGainTrackPeak),
                Some("0.988553"),
                "{kind:?}"
            );
        }
    }

    /// A bare name is not a valid MP4 atom and is dropped on save, so custom
    /// fields go where foobar2000, Mp3tag and DJ software look for them.
    #[test]
    fn mp4_custom_fields_are_freeform_itunes_atoms() {
        assert_eq!(
            custom_key(TagType::Mp4Ilst, "ENERGY"),
            ItemKey::Unknown("----:com.apple.iTunes:ENERGY".into())
        );
        // Fields with a real mapping keep it.
        assert_eq!(custom_key(TagType::Mp4Ilst, "MOOD"), ItemKey::Mood);
    }

    #[test]
    fn a_custom_field_can_be_cleared() {
        let mut t = tag();
        t.insert_text(ItemKey::Unknown("MOOD".into()), "Dark".into());
        round_trip(
            &mut t,
            &Edit {
                custom: vec![("MOOD".into(), Change::Clear)],
                ..Default::default()
            },
        );
        assert_eq!(t.get_string(&ItemKey::Unknown("MOOD".into())), None);
    }

    /// A blank field name would write an unnamed frame that nothing can read
    /// back, which is worse than doing nothing.
    #[test]
    fn a_custom_field_with_no_name_is_ignored() {
        let mut t = tag();
        round_trip(
            &mut t,
            &Edit {
                custom: vec![("  ".into(), Change::Set("x".into()))],
                ..Default::default()
            },
        );
        assert_eq!(t.len(), 0);
    }

    /// What the editor's boxes mean: untouched is untouched, emptied is cleared.
    #[test]
    fn an_emptied_box_clears_and_an_untouched_one_keeps() {
        assert_eq!(Change::from_input(None), Change::Keep);
        assert_eq!(Change::from_input(Some("   ".into())), Change::Clear);
        assert_eq!(
            Change::from_input(Some("  Bicep ".into())),
            Change::Set("Bicep".into())
        );
    }
}
