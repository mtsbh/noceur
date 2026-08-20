//! Reading and normalising audio file tags.

pub mod key;
pub mod meta;

pub use key::{KeyNotation, MusicalKey};
pub use meta::{artwork, read, Artwork, MetaError, TrackMeta};
