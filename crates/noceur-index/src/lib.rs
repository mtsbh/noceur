//! The Noceur index: what we remember about the filesystem.

pub mod db;
pub mod schema;

pub use db::{Db, IndexError};
