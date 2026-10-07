//! WAV files whose `LIST INFO` lofty refuses.
//!
//! lofty gives up on a whole file when one RIFF INFO key is anything but four
//! capitals or digits, whatever parsing mode it is asked for. Such files are
//! common: a tagger that counted an artist like `Åcception` in characters
//! rather than bytes leaves every key after it one byte off, and the file
//! cannot be read at all, though its ID3 tag next to it is perfectly fine.
//!
//! The answer here is to read such a file again with that one chunk relabelled
//! `JUNK`, which every RIFF reader skips. Nothing on disk changes; the INFO
//! list is simply not read, which it could not have been anyway.

use lofty::error::LoftyError;
use lofty::file::TaggedFile;
use lofty::probe::Probe;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// Reads a file's tags, past an INFO list lofty cannot parse.
///
/// The container is recognised from its own first bytes, falling back to the
/// extension, so a file named wrongly still reads as what it is.
pub fn read(path: &Path) -> Result<TaggedFile, LoftyError> {
    let plain = Probe::open(path).and_then(|p| p.guess_file_type()?.read());
    match plain {
        Err(e) => match broken_info(path) {
            Some(at) => Probe::new(BufReader::new(Masked::open(path, at)?))
                .guess_file_type()?
                .read(),
            None => Err(e),
        },
        ok => ok,
    }
}

/// A file opened for lofty to parse, with a broken INFO list hidden if it has
/// one.
pub(crate) fn open_masked(path: &Path) -> io::Result<Masked> {
    Masked::open(path, broken_info(path).unwrap_or(u64::MAX))
}

/// Where the `LIST` chunk holding a malformed INFO list starts, if any.
pub(crate) fn broken_info(path: &Path) -> Option<u64> {
    let mut f = BufReader::new(File::open(path).ok()?);
    let mut head = [0u8; 12];
    f.read_exact(&mut head).ok()?;
    if &head[..4] != b"RIFF" || &head[8..] != b"WAVE" {
        return None;
    }
    let len = f.get_ref().metadata().ok()?.len();
    let mut at = 12u64;
    while at + 8 <= len {
        f.seek(SeekFrom::Start(at)).ok()?;
        let (id, size) = chunk_header(&mut f)?;
        if &id == b"LIST" {
            let mut kind = [0u8; 4];
            f.read_exact(&mut kind).ok()?;
            if &kind == b"INFO" && !info_is_sound(&mut f, at + 8 + u64::from(size))? {
                return Some(at);
            }
        }
        at += 8 + u64::from(size) + u64::from(size & 1);
    }
    None
}

/// Whether every key in an INFO list is one lofty accepts.
fn info_is_sound(f: &mut BufReader<File>, end: u64) -> Option<bool> {
    let mut pos = f.stream_position().ok()?;
    while pos + 8 <= end {
        let (key, size) = chunk_header(f)?;
        if !key
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            return Some(false);
        }
        pos += 8 + u64::from(size) + u64::from(size & 1);
        f.seek(SeekFrom::Start(pos)).ok()?;
    }
    Some(true)
}

fn chunk_header(f: &mut impl Read) -> Option<([u8; 4], u32)> {
    let mut h = [0u8; 8];
    f.read_exact(&mut h).ok()?;
    let id = [h[0], h[1], h[2], h[3]];
    Some((id, u32::from_le_bytes([h[4], h[5], h[6], h[7]])))
}

/// A file that reads `JUNK` in place of the four bytes at `at`.
pub(crate) struct Masked {
    file: File,
    at: u64,
    pos: u64,
}

impl Masked {
    fn open(path: &Path, at: u64) -> io::Result<Masked> {
        Ok(Masked {
            file: File::open(path)?,
            at,
            pos: 0,
        })
    }
}

impl Read for Masked {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.file.read(buf)?;
        let (start, end) = (self.pos, self.pos + n as u64);
        for (i, b) in b"JUNK".iter().enumerate() {
            let p = self.at.saturating_add(i as u64);
            if p >= start && p < end {
                buf[(p - start) as usize] = *b;
            }
        }
        self.pos = end;
        Ok(n)
    }
}

impl Seek for Masked {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = self.file.seek(to)?;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::file::TaggedFileExt;
    use lofty::tag::Accessor;

    /// A one-sample WAV with an ID3 title and an INFO list whose second key
    /// is one byte off, as the tagger that miscounts `Å` leaves it.
    pub(crate) fn broken_wav(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("broken.wav");
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes()); // PCM
        fmt.extend_from_slice(&1u16.to_le_bytes()); // mono
        fmt.extend_from_slice(&44100u32.to_le_bytes());
        fmt.extend_from_slice(&88200u32.to_le_bytes());
        fmt.extend_from_slice(&2u16.to_le_bytes());
        fmt.extend_from_slice(&16u16.to_le_bytes());

        let mut body = b"WAVE".to_vec();
        let mut chunk = |id: &[u8], data: &[u8]| {
            body.extend_from_slice(id);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        };
        chunk(b"fmt ", &fmt);
        chunk(b"data", &[0, 0]);
        // ID3v2.4 with a single TIT2 frame.
        let title = b"\x03Cycle";
        let mut frame = b"TIT2".to_vec();
        frame.extend_from_slice(&(title.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(title);
        let mut id3 = b"ID3\x04\x00\x00".to_vec();
        id3.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        id3.extend_from_slice(&frame);
        chunk(b"id3 ", &id3);
        let mut info = b"INFO".to_vec();
        info.extend_from_slice(b"IART\x0c\x00\x00\x00A\xcc\x8acception\x00");
        info.extend_from_slice(b"\x00OBS\x02\x00\x00\x00x\x00");
        chunk(b"LIST", &info);

        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        std::fs::write(&path, out).unwrap();
        path
    }

    #[test]
    fn a_wav_with_a_broken_info_list_still_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = broken_wav(dir.path());
        assert!(
            Probe::open(&path).unwrap().read().is_err(),
            "lofty alone refuses it"
        );

        let before = std::fs::read(&path).unwrap();
        let tagged = read(&path).unwrap();
        let tag = tagged.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Cycle"));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "nothing on disk changes"
        );
    }

    #[test]
    fn a_wav_with_a_broken_info_list_takes_tags_and_a_rating() {
        let dir = tempfile::tempdir().unwrap();
        let path = broken_wav(dir.path());

        let edit = crate::Edit {
            artist: crate::Change::Set("Åcception".into()),
            ..Default::default()
        };
        crate::edit::write(&path, &edit).unwrap();
        crate::rating::write_id3(&path, Some(4)).unwrap();

        let tagged = read(&path).unwrap();
        let tag = tagged.primary_tag().unwrap();
        assert_eq!(tag.artist().as_deref(), Some("Åcception"));
        assert_eq!(tag.title().as_deref(), Some("Cycle"));
        assert_eq!(crate::rating::read_id3(&path), Some(4));
    }

    #[test]
    fn a_sound_wav_is_not_masked() {
        let dir = tempfile::tempdir().unwrap();
        let path = broken_wav(dir.path());
        let mut bytes = std::fs::read(&path).unwrap();
        let at = bytes.windows(4).position(|w| w == b"\x00OBS").unwrap();
        bytes[at] = b'J';
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(broken_info(&path), None);
    }
}
