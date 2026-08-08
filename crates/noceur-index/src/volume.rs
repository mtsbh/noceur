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

use std::path::Path;

/// Registers (or finds) the volume containing `path` and returns its id.
///
/// # Provisional
///
/// Identity is derived from the path's root — the drive letter on Windows, the
/// share root for UNC paths, `/` on Unix — **not** from a real filesystem
/// identifier. That means a removable drive reassigned from `S:` to `E:` currently
/// reads as a different volume, which is exactly what `fs_uuid` was introduced to
/// prevent.
///
/// This is deliberate and bounded: real identity (volume serial numbers on Windows,
/// UUIDs elsewhere) lands in the identity-and-freshness milestone, and only this
/// function changes when it does. Everything downstream already keys on the
/// returned id.
pub fn for_path(db: &Db, path: &Path) -> Result<i64, IndexError> {
    let (uuid, kind, label, mount) = root_of(path);
    upsert(db, &uuid, kind, &label, &mount)
}

/// Splits a path into its volume root and a provisional identity for it.
fn root_of(path: &Path) -> (String, VolumeKind, String, String) {
    let s = path.to_string_lossy();

    #[cfg(windows)]
    {
        // UNC share: \\server\share\... -> the \\server\share pair is the volume.
        if let Some(rest) = s.strip_prefix(r"\\") {
            let mut parts = rest.split(['\\', '/']).filter(|p| !p.is_empty());
            let server = parts.next().unwrap_or("");
            let share = parts.next().unwrap_or("");
            let mount = format!(r"\\{server}\{share}");
            return (
                format!("provisional:unc:{}", mount.to_lowercase()),
                VolumeKind::Network,
                share.to_string(),
                mount,
            );
        }
        // Drive letter.
        if let Some(letter) = s.chars().next().filter(|c| c.is_ascii_alphabetic()) {
            let letter = letter.to_ascii_uppercase();
            let mount = format!(r"{letter}:\");
            return (
                format!("provisional:drive:{letter}"),
                VolumeKind::Fixed,
                format!("{letter}:"),
                mount,
            );
        }
    }

    #[cfg(not(windows))]
    {
        let _ = &s;
    }

    (
        "provisional:root".to_string(),
        VolumeKind::Fixed,
        "/".to_string(),
        "/".to_string(),
    )
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

    #[test]
    fn resolves_a_volume_for_a_path() {
        let db = db();
        let p = if cfg!(windows) {
            std::path::PathBuf::from(r"C:\Music\House")
        } else {
            std::path::PathBuf::from("/music/house")
        };
        let id = for_path(&db, &p).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn two_paths_on_one_volume_resolve_to_the_same_row() {
        let db = db();
        let (a, b) = if cfg!(windows) {
            (r"C:\Music\House", r"C:\Other\Place")
        } else {
            ("/music/house", "/other/place")
        };
        let ia = for_path(&db, std::path::Path::new(a)).unwrap();
        let ib = for_path(&db, std::path::Path::new(b)).unwrap();
        assert_eq!(ia, ib);
        assert_eq!(all(&db).unwrap().len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn different_drive_letters_are_different_volumes() {
        let db = db();
        let c = for_path(&db, std::path::Path::new(r"C:\Music")).unwrap();
        let e = for_path(&db, std::path::Path::new(r"E:\music")).unwrap();
        assert_ne!(c, e);
        assert_eq!(all(&db).unwrap().len(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn a_unc_share_is_its_own_volume() {
        let db = db();
        let local = for_path(&db, std::path::Path::new(r"C:\Music")).unwrap();
        let share = for_path(&db, std::path::Path::new(r"\\nas\sets\friday")).unwrap();
        assert_ne!(local, share);

        let v = get(&db, share).unwrap().unwrap();
        assert_eq!(v.kind, VolumeKind::Network);
    }

    #[test]
    fn resolution_is_case_insensitive_on_windows_only() {
        let db = db();
        if cfg!(windows) {
            let lower = for_path(&db, std::path::Path::new(r"c:\music")).unwrap();
            let upper = for_path(&db, std::path::Path::new(r"C:\MUSIC")).unwrap();
            assert_eq!(lower, upper);
        } else {
            // On Unix the root is always "/" regardless of case further down.
            let a = for_path(&db, std::path::Path::new("/Music")).unwrap();
            let b = for_path(&db, std::path::Path::new("/music")).unwrap();
            assert_eq!(a, b);
        }
    }
}
