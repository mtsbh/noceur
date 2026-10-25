//! Reading and normalising audio file tags.

pub mod edit;
pub mod key;
pub mod meta;
pub mod rating;
pub mod riff;

pub use edit::{Change, Edit, EditError};
pub use key::{KeyNotation, MusicalKey};
pub use meta::{artwork, is_placeholder, read, read_fields, Artwork, Fields, MetaError, TrackMeta};
