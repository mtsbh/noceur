//! Opening an audio file for decoding, past whatever is nailed to the front.
//!
//! Symphonia parses a file's ID3v2 tag on the way in and refuses the whole
//! stream if that tag is malformed. Plenty of real files have malformed tags —
//! a blog's downloader that wrote a `WXXX` frame with the wrong text encoding
//! byte, a tagger from 2008 that left reserved flag bits set — and none of that
//! has anything to do with the audio, which is perfectly good.
//!
//! The effect was severe and quiet: those files would not play, would not draw a
//! waveform, and could not be analysed, all with "the stream contained malformed
//! data". A whole folder of them looked like a folder of corrupt files.
//!
//! So the tag is skipped before the decoder ever sees it. An ID3v2 header states
//! its own length, and everything after it is the container the file actually
//! is — which is why this is a fix rather than a workaround: the decoder is
//! being handed the audio, and only the audio.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// A reader that hides a prefix of the underlying stream.
///
/// Offsets every seek, so position zero is the first byte of the container
/// rather than the first byte of the file. The decoder seeks to the start while
/// probing, and without this it would land back in the tag it was meant to skip.
pub struct Skipping<R> {
    inner: R,
    start: u64,
}

impl<R: Read + Seek> Skipping<R> {
    fn new(inner: R, start: u64) -> Skipping<R> {
        Skipping { inner, start }
    }
}

impl<R: Read> Read for Skipping<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<R: Seek> Seek for Skipping<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let at = match pos {
            SeekFrom::Start(n) => self.inner.seek(SeekFrom::Start(self.start + n))?,
            // Relative seeks need no adjustment; only the origin moved.
            other => self.inner.seek(other)?,
        };
        Ok(at.saturating_sub(self.start))
    }
}

/// A file opened for decoding, with its ID3v2 tag skipped if it had one.
pub struct Opened {
    pub reader: Skipping<io::BufReader<std::fs::File>>,
    /// Bytes of audio, excluding any skipped tag.
    ///
    /// Seeking needs this as much as the seekable flag does: a WAV seeks on
    /// frame arithmetic and manages without it, while FLAC binary-searches the
    /// stream and cannot.
    pub byte_len: Option<u64>,
}

/// Opens a file for decoding.
pub fn open(path: &Path) -> io::Result<Opened> {
    let mut file = std::fs::File::open(path)?;
    let total = file.metadata().ok().map(|m| m.len());

    let skip = id3v2_len(&mut file)?;
    file.seek(SeekFrom::Start(skip))?;

    Ok(Opened {
        // Large reads: a 96kHz 24-bit file is half a megabyte a second, and the
        // default 8KB buffer turns that into sixty system calls.
        reader: Skipping::new(io::BufReader::with_capacity(256 * 1024, file), skip),
        byte_len: total.map(|len| len.saturating_sub(skip)),
    })
}

/// The length of a leading ID3v2 tag, or zero when there is not one.
///
/// The header is ten bytes: `ID3`, two version bytes, a flags byte, and four
/// sync-safe length bytes — seven bits each, because a byte with its top bit set
/// could be mistaken for the start of an MPEG frame. A footer, when the flags
/// say there is one, is another ten.
fn id3v2_len(file: &mut std::fs::File) -> io::Result<u64> {
    let mut header = [0u8; 10];
    file.seek(SeekFrom::Start(0))?;
    if file.read_exact(&mut header).is_err() {
        file.seek(SeekFrom::Start(0))?;
        return Ok(0);
    }
    file.seek(SeekFrom::Start(0))?;

    if &header[..3] != b"ID3" {
        return Ok(0);
    }
    // A version byte of 0xFF is invalid, and reading a length out of a header
    // that is not one would skip an arbitrary distance into the audio.
    if header[3] == 0xFF || header[4] == 0xFF {
        return Ok(0);
    }
    if header[6..10].iter().any(|b| b & 0x80 != 0) {
        return Ok(0);
    }

    let size = header[6..10]
        .iter()
        .fold(0u64, |acc, b| (acc << 7) | u64::from(*b));
    let footer = if header[5] & 0x10 != 0 { 10 } else { 0 };

    Ok(size + 10 + footer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Builds an ID3v2 header of the given payload size.
    fn tag(size: u32, flags: u8) -> Vec<u8> {
        let mut v = vec![b'I', b'D', b'3', 4, 0, flags];
        // Sync-safe: seven bits per byte.
        v.push(((size >> 21) & 0x7F) as u8);
        v.push(((size >> 14) & 0x7F) as u8);
        v.push(((size >> 7) & 0x7F) as u8);
        v.push((size & 0x7F) as u8);
        v.extend(std::iter::repeat_n(0u8, size as usize));
        v
    }

    fn written(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mp3");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        (dir, path)
    }

    #[test]
    fn a_file_with_no_tag_is_opened_whole() {
        let (_d, path) = written(b"\xff\xfbaudio");
        let mut opened = open(&path).unwrap();
        let mut out = Vec::new();
        opened.reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"\xff\xfbaudio");
        assert_eq!(opened.byte_len, Some(7));
    }

    #[test]
    fn a_leading_tag_is_skipped() {
        let mut bytes = tag(100, 0);
        bytes.extend(b"\xff\xfbaudio");
        let (_d, path) = written(&bytes);

        let mut opened = open(&path).unwrap();
        let mut out = Vec::new();
        opened.reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"\xff\xfbaudio", "the tag must not be handed over");
        assert_eq!(opened.byte_len, Some(7), "nor counted");
    }

    /// The decoder seeks to the start while probing, and would otherwise land
    /// back inside the tag this exists to skip.
    #[test]
    fn seeking_to_the_start_lands_after_the_tag() {
        let mut bytes = tag(64, 0);
        bytes.extend(b"\xff\xfbaudio");
        let (_d, path) = written(&bytes);

        let mut opened = open(&path).unwrap();
        let mut first = [0u8; 2];
        opened.reader.read_exact(&mut first).unwrap();

        opened.reader.seek(SeekFrom::Start(0)).unwrap();
        let mut again = [0u8; 2];
        opened.reader.read_exact(&mut again).unwrap();
        assert_eq!(first, again);
        assert_eq!(&again, b"\xff\xfb");
    }

    #[test]
    fn positions_are_reported_relative_to_the_audio() {
        let mut bytes = tag(64, 0);
        bytes.extend(b"\xff\xfbaudio");
        let (_d, path) = written(&bytes);

        let mut opened = open(&path).unwrap();
        assert_eq!(opened.reader.seek(SeekFrom::Start(3)).unwrap(), 3);
        let mut one = [0u8; 1];
        opened.reader.read_exact(&mut one).unwrap();
        assert_eq!(one[0], b'u', "fourth byte of the audio");
    }

    #[test]
    fn a_footer_is_skipped_too() {
        let mut bytes = tag(20, 0x10);
        bytes.extend(std::iter::repeat_n(0u8, 10)); // the footer itself
        bytes.extend(b"\xff\xfbaudio");
        let (_d, path) = written(&bytes);

        let mut opened = open(&path).unwrap();
        let mut out = Vec::new();
        opened.reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"\xff\xfbaudio");
    }

    /// A length read out of something that is not a header would skip an
    /// arbitrary distance into the audio, which is worse than not skipping.
    #[test]
    fn something_that_only_looks_like_a_tag_is_left_alone() {
        // Sync-safe bytes may not have their top bit set.
        let mut bytes = vec![b'I', b'D', b'3', 4, 0, 0, 0xFF, 0, 0, 0];
        bytes.extend(b"audio");
        let (_d, path) = written(&bytes);

        let mut opened = open(&path).unwrap();
        let mut out = Vec::new();
        opened.reader.read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), bytes.len(), "nothing skipped");
    }

    #[test]
    fn a_file_shorter_than_a_header_is_left_alone() {
        let (_d, path) = written(b"ID3");
        let mut opened = open(&path).unwrap();
        let mut out = Vec::new();
        opened.reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"ID3");
    }
}
