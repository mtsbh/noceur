//! Filesystem model: enumeration, classification and ordering.

pub mod kind;
pub mod path;

pub use kind::{is_audio_extension, EntryKind, AUDIO_EXTENSIONS};
