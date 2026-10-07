//! Star ratings, which every format stores differently and one stores badly.
//!
//! Vorbis comments and MP4 hold a rating as an ordinary number, so it travels
//! with every other field through the generic tag. ID3 holds a `POPM` frame —
//! an email address, a rating byte and a play counter — and lofty deliberately
//! keeps that frame out of its format-independent tag, which means it can be
//! neither read nor written there. Everything in this module exists for that.
//!
//! The scale is five even steps of 51, which is what Rekordbox, Serato and
//! Traktor read and write. It is not the only convention — Windows Media Player
//! uses 1/64/128/196/255 — but these files are going into DJ software, and
//! agreeing with the programs they will actually be opened in matters more than
//! agreeing with a media player from 2003.

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType};
use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, PopularimeterFrame};
use lofty::probe::Probe;
use std::borrow::Cow;
use std::io::Seek;
use std::path::Path;

/// The `POPM` frame identifier.
const POPM: FrameId<'static> = FrameId::Valid(Cow::Borrowed("POPM"));

/// Whether a container keeps its tags in ID3v2.
///
/// The three that matter to a DJ library: MP3, and the WAV and AIFF that come
/// off a CDJ export or a vinyl rip.
pub fn uses_id3(file_type: FileType) -> bool {
    matches!(
        file_type,
        FileType::Mpeg | FileType::Wav | FileType::Aiff | FileType::Aac
    )
}

/// The file type, when it can be determined.
pub fn file_type(path: &Path) -> Option<FileType> {
    Probe::open(path).ok()?.guess_file_type().ok()?.file_type()
}

/// Reads the star rating out of an ID3 file.
///
/// `None` when there is no rating, which is not the same as zero stars — a track
/// nobody has rated and a track someone rated nothing are different claims, and
/// only the first is ever written.
pub fn read_id3(path: &Path) -> Option<u8> {
    let byte = with_id3(path, |tag| {
        tag.get(&POPM).and_then(|frame| match frame {
            Frame::Popularimeter(popm) => Some(popm.rating),
            _ => None,
        })
    })?;
    byte.map(stars_from_byte)
}

/// Writes or removes the rating on an ID3 file.
///
/// `None`, and zero stars, both remove the frame. Pressing the star a track
/// already has is how anyone clears a rating, and a `POPM` holding zero is not
/// something the programs reading it agree about.
pub fn write_id3(path: &Path, stars: Option<u8>) -> Result<(), lofty::error::LoftyError> {
    let Some(file_type) = file_type(path) else {
        return Ok(());
    };
    if !uses_id3(file_type) {
        return Ok(());
    }

    let mut handle = std::fs::File::options().read(true).write(true).open(path)?;

    /// Reads one container, sets or removes the frame, and writes it back.
    ///
    /// A macro rather than a function because the three file types share no
    /// trait that exposes an ID3 tag — each has its own inherent `id3v2_mut` —
    /// and the alternative is the same twelve lines written three times.
    macro_rules! rate {
        ($ty:ty) => {{
            // Parsed through the mask, so a broken INFO list doesn't stop it;
            // written to the real file.
            let mut file =
                <$ty>::read_from(&mut crate::riff::open_masked(path)?, ParseOptions::new())?;

            // A file with no ID3 tag and no rating to write is left alone.
            // Adding an empty tag to clear a rating that was never there would
            // rewrite the file for no change.
            if file.id3v2().is_none() {
                if stars.is_none() {
                    return Ok(());
                }
                file.set_id3v2(Id3v2Tag::default());
            }

            if let Some(tag) = file.id3v2_mut() {
                match stars {
                    Some(n) if n > 0 => {
                        tag.insert(popm(n));
                    }
                    // Zero stars and no rating both remove the frame. Pressing
                    // the star a track already has is how anyone clears one.
                    _ => {
                        let removed: Vec<_> = tag.remove(&POPM).collect();
                        drop(removed);
                    }
                }
            }

            handle.seek(std::io::SeekFrom::Start(0))?;
            file.save_to(&mut handle, WriteOptions::default())?;
        }};
    }

    match file_type {
        FileType::Mpeg => rate!(lofty::mpeg::MpegFile),
        FileType::Wav => rate!(lofty::iff::wav::WavFile),
        FileType::Aiff => rate!(lofty::iff::aiff::AiffFile),
        _ => {}
    }
    Ok(())
}

/// Runs a closure over a file's ID3 tag, if it has one.
fn with_id3<T>(path: &Path, f: impl FnOnce(&Id3v2Tag) -> T) -> Option<T> {
    let file_type = file_type(path)?;
    if !uses_id3(file_type) {
        return None;
    }
    let mut handle = crate::riff::open_masked(path).ok()?;

    match file_type {
        FileType::Mpeg => lofty::mpeg::MpegFile::read_from(&mut handle, ParseOptions::new())
            .ok()?
            .id3v2()
            .map(f),
        FileType::Wav => lofty::iff::wav::WavFile::read_from(&mut handle, ParseOptions::new())
            .ok()?
            .id3v2()
            .map(f),
        FileType::Aiff => lofty::iff::aiff::AiffFile::read_from(&mut handle, ParseOptions::new())
            .ok()?
            .id3v2()
            .map(f),
        _ => None,
    }
}

/// Turns a stored rating byte into stars.
///
/// Tolerant of the scales in circulation, but where two disagree the DJ one
/// wins: 51 is one star to Rekordbox and 64 is two to Windows Media Player, with
/// nothing between them to split on, and these files are going into DJ software.
/// The cost is that a two-star rating written by WMP reads here as one.
pub fn stars_from_byte(value: u8) -> u8 {
    match value {
        0 => 0,
        // Small numbers are stars already: nothing writes a byte of 3.
        1..=5 => value,
        // Midway between the values 51-per-star writes.
        6..=76 => 1,
        77..=127 => 2,
        128..=178 => 3,
        179..=229 => 4,
        _ => 5,
    }
}

/// Turns stars into the byte to store.
pub fn byte_from_stars(stars: u8) -> u8 {
    stars.min(5).saturating_mul(51)
}

/// The frame to write for a star count.
pub fn popm(stars: u8) -> Frame<'static> {
    // The email identifies whose rating it is. This app has no opinion about
    // that, so it is left empty — which is what every other tagger does.
    Frame::Popularimeter(PopularimeterFrame::new(
        String::new(),
        byte_from_stars(stars),
        0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_map_to_the_scale_dj_software_reads() {
        for (stars, byte) in [(1, 51), (2, 102), (3, 153), (4, 204), (5, 255)] {
            assert_eq!(byte_from_stars(stars), byte, "{stars} stars");
            assert_eq!(stars_from_byte(byte), stars, "byte {byte}");
        }
        assert_eq!(byte_from_stars(0), 0);
    }

    /// Ratings arrive on several scales depending on what wrote them.
    #[test]
    fn other_scales_read_sensibly() {
        // A plain star count, as Vorbis comments often hold.
        assert_eq!(stars_from_byte(4), 4);
        // Windows Media Player agrees from three stars up.
        for (raw, stars) in [(128, 3), (196, 4), (255, 5)] {
            assert_eq!(stars_from_byte(raw), stars, "{raw}");
        }
        assert_eq!(stars_from_byte(0), 0);
    }

    #[test]
    fn only_id3_containers_are_claimed() {
        assert!(uses_id3(FileType::Mpeg));
        assert!(uses_id3(FileType::Wav));
        assert!(uses_id3(FileType::Aiff));
        assert!(!uses_id3(FileType::Flac));
        assert!(!uses_id3(FileType::Mp4));
    }
}
