//! Entry classification: directory, audio file, or other.

use std::path::Path;

/// Extensions treated as audio, lowercase, without the leading dot.
///
/// `mp4` is included because DJ libraries contain ALAC and AAC audio in `.mp4`
/// containers. Video in `.mp4` is rare enough in a music tree that mis-classifying
/// it costs a wasted probe, not a wrong answer.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "aiff", "aif", "aifc", "flac", "mp3", "m4a", "mp4", "ogg", "oga",
    "opus", "wv", "ape",
];

/// How an entry is grouped in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    Directory,
    Audio,
    Other,
}

impl EntryKind {
    /// Classifies an entry from its name and whether it is a directory.
    pub fn classify(path: &Path, is_dir: bool) -> Self {
        if is_dir {
            return EntryKind::Directory;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) if is_audio_extension(ext) => EntryKind::Audio,
            _ => EntryKind::Other,
        }
    }

    /// Sort group. Lower sorts first, and this always outranks the user's
    /// chosen sort column so directories stay on top.
    pub fn group(self) -> u8 {
        match self {
            EntryKind::Directory => 0,
            EntryKind::Audio => 1,
            EntryKind::Other => 2,
        }
    }

    pub fn is_audio(self) -> bool {
        matches!(self, EntryKind::Audio)
    }
}

/// Case-insensitive audio extension check.
pub fn is_audio_extension(ext: &str) -> bool {
    AUDIO_EXTENSIONS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directories_classify_as_directory() {
        assert_eq!(EntryKind::classify(Path::new("Unsorted"), true), EntryKind::Directory);
        // Extension is irrelevant for directories.
        assert_eq!(EntryKind::classify(Path::new("weird.wav"), true), EntryKind::Directory);
    }

    #[test]
    fn known_audio_extensions_classify_as_audio() {
        for name in [
            "a.wav", "a.aiff", "a.aif", "a.aifc", "a.flac", "a.mp3",
            "a.m4a", "a.mp4", "a.ogg", "a.oga", "a.opus", "a.wv", "a.ape",
        ] {
            assert_eq!(
                EntryKind::classify(Path::new(name), false),
                EntryKind::Audio,
                "{name} should be audio"
            );
        }
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert_eq!(EntryKind::classify(Path::new("A.WAV"), false), EntryKind::Audio);
        assert_eq!(EntryKind::classify(Path::new("A.FlAc"), false), EntryKind::Audio);
    }

    #[test]
    fn other_files_classify_as_other() {
        for name in ["cover.jpg", "tracklist.txt", "notes", "a.wave", "a.mp"] {
            assert_eq!(
                EntryKind::classify(Path::new(name), false),
                EntryKind::Other,
                "{name} should be other"
            );
        }
    }

    #[test]
    fn group_order_is_directories_then_audio_then_other() {
        assert!(EntryKind::Directory.group() < EntryKind::Audio.group());
        assert!(EntryKind::Audio.group() < EntryKind::Other.group());
    }
}
