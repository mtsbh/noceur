//! Volume registry: stable identity for drives that come and go.

use crate::db::{Db, IndexError};

/// What kind of storage a volume is. Drives watcher strategy in a later milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeKind {
    Fixed,
    Removable,
    Network,
}

impl VolumeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VolumeKind::Fixed => "fixed",
            VolumeKind::Removable => "removable",
            VolumeKind::Network => "network",
        }
    }

    // Not a `std::str::FromStr` impl on purpose: that trait's contract is
    // fallible parsing (`Result<Self, Self::Err>`), while this method is
    // infallible by design — an unrecognised string (e.g. from an index
    // written by a newer build) falls back to `Fixed` rather than erroring,
    // so the index still opens.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "removable" => VolumeKind::Removable,
            "network" => VolumeKind::Network,
            _ => VolumeKind::Fixed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub id: i64,
    pub kind: VolumeKind,
    pub fs_uuid: String,
    pub label: String,
    pub mount_path: String,
    pub online: bool,
    pub last_seen_at: i64,
}

/// Registers a volume, or updates its label and mount path if already known.
///
/// Identity is `fs_uuid`, never the mount path — a drive letter reassignment
/// must not create a second volume or orphan existing rows.
pub fn upsert(
    db: &Db,
    fs_uuid: &str,
    kind: VolumeKind,
    label: &str,
    mount_path: &str,
) -> Result<i64, IndexError> {
    db.conn().execute(
        "INSERT INTO volume (kind, fs_uuid, label, mount_path, online, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5)
         ON CONFLICT(fs_uuid) DO UPDATE SET
             label        = excluded.label,
             mount_path   = excluded.mount_path,
             kind         = excluded.kind,
             online       = 1,
             last_seen_at = excluded.last_seen_at",
        rusqlite::params![kind.as_str(), fs_uuid, label, mount_path, now()],
    )?;

    Ok(db
        .conn()
        .query_row("SELECT id FROM volume WHERE fs_uuid = ?1", [fs_uuid], |r| {
            r.get(0)
        })?)
}

pub fn get(db: &Db, id: i64) -> Result<Option<Volume>, IndexError> {
    let mut stmt = db.conn().prepare(
        "SELECT id, kind, fs_uuid, label, mount_path, online, last_seen_at
         FROM volume WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map([id], row_to_volume)?;
    Ok(rows.next().transpose()?)
}

pub fn all(db: &Db) -> Result<Vec<Volume>, IndexError> {
    let mut stmt = db.conn().prepare(
        "SELECT id, kind, fs_uuid, label, mount_path, online, last_seen_at
         FROM volume ORDER BY id",
    )?;
    let rows = stmt.query_map([], row_to_volume)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Marks a volume online or offline. Never deletes rows — offline content stays
/// browsable and searchable.
pub fn set_online(db: &Db, id: i64, online: bool) -> Result<(), IndexError> {
    db.conn().execute(
        "UPDATE volume SET online = ?1, last_seen_at = ?2 WHERE id = ?3",
        rusqlite::params![online as i32, now(), id],
    )?;
    Ok(())
}

fn row_to_volume(r: &rusqlite::Row<'_>) -> rusqlite::Result<Volume> {
    Ok(Volume {
        id: r.get(0)?,
        kind: VolumeKind::from_str(&r.get::<_, String>(1)?),
        fs_uuid: r.get(2)?,
        label: r.get(3)?,
        mount_path: r.get(4)?,
        online: r.get::<_, i32>(5)? != 0,
        last_seen_at: r.get(6)?,
    })
}

/// Seconds since the Unix epoch.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn registers_a_new_volume() {
        let db = db();
        let id = upsert(
            &db,
            "uuid-party",
            VolumeKind::Removable,
            "PARTY SSD",
            r"S:\",
        )
        .unwrap();
        let v = get(&db, id).unwrap().unwrap();
        assert_eq!(v.fs_uuid, "uuid-party");
        assert_eq!(v.mount_path, r"S:\");
        assert!(v.online);
    }

    #[test]
    fn upserting_the_same_uuid_returns_the_same_row() {
        let db = db();
        let a = upsert(&db, "uuid-x", VolumeKind::Fixed, "data", r"D:\").unwrap();
        let b = upsert(&db, "uuid-x", VolumeKind::Fixed, "data", r"D:\").unwrap();
        assert_eq!(a, b);
        assert_eq!(all(&db).unwrap().len(), 1);
    }

    #[test]
    fn a_reassigned_drive_letter_updates_the_mount_path() {
        let db = db();
        let id = upsert(
            &db,
            "uuid-party",
            VolumeKind::Removable,
            "PARTY SSD",
            r"S:\",
        )
        .unwrap();
        let again = upsert(
            &db,
            "uuid-party",
            VolumeKind::Removable,
            "PARTY SSD",
            r"E:\",
        )
        .unwrap();
        assert_eq!(id, again, "same volume despite the new letter");
        assert_eq!(get(&db, id).unwrap().unwrap().mount_path, r"E:\");
    }

    #[test]
    fn going_offline_preserves_the_row() {
        let db = db();
        let id = upsert(&db, "uuid-z", VolumeKind::Network, "sets", r"Z:\").unwrap();
        set_online(&db, id, false).unwrap();

        let v = get(&db, id).unwrap().unwrap();
        assert!(!v.online);
        assert_eq!(
            all(&db).unwrap().len(),
            1,
            "offline volumes are not deleted"
        );

        set_online(&db, id, true).unwrap();
        assert!(get(&db, id).unwrap().unwrap().online);
    }

    #[test]
    fn get_returns_none_for_an_unknown_id() {
        let db = db();
        assert!(get(&db, 9999).unwrap().is_none());
    }
}
