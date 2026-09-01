//! Reading and normalising audio file tags.

pub mod edit;
pub mod key;
pub mod meta;

pub use edit::{Change, Edit, EditError};
pub use key::{KeyNotation, MusicalKey};
pub use meta::{artwork, read, read_fields, Artwork, Fields, MetaError, TrackMeta};
