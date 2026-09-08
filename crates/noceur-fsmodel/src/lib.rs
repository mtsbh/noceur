//! Filesystem model: enumeration, classification and ordering.

pub mod cleanup;
pub mod entry;
pub mod enumerate;
pub mod kind;
pub mod order;
pub mod path;
pub mod rename;
pub mod titlefmt;

pub use entry::FsEntry;
pub use enumerate::{read_dir, DirListing, EnumerateError, SkippedEntry};
pub use kind::{extension_of, is_audio_extension, EntryKind, AUDIO_EXTENSIONS};
pub use order::{natural_cmp, sort, SortDir, SortKey};
pub use path::{extended, simplify};
