//! Sort ordering for directory listings.

use crate::entry::FsEntry;
use std::cmp::Ordering;

/// Column the user is sorting by. Tag-derived columns are added by
/// `noceur-index`, which sorts in SQL rather than here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Sorts entries the way Explorer and Finder do: group first, column second.
///
/// The group ordering (directories, audio, other) is never reversed — flipping
/// to descending reverses order *within* each group only.
pub fn sort(entries: &mut [FsEntry], key: SortKey, dir: SortDir) {
    entries.sort_by(|a, b| {
        a.kind
            .group()
            .cmp(&b.kind.group())
            .then_with(|| apply_dir(compare_by(a, b, key), dir))
    });
}

fn apply_dir(ord: Ordering, dir: SortDir) -> Ordering {
    match dir {
        SortDir::Asc => ord,
        SortDir::Desc => ord.reverse(),
    }
}

fn compare_by(a: &FsEntry, b: &FsEntry, key: SortKey) -> Ordering {
    match key {
        SortKey::Name => natural_cmp(&a.name, &b.name),
        SortKey::Size => a
            .size
            .cmp(&b.size)
            .then_with(|| natural_cmp(&a.name, &b.name)),
        SortKey::Modified => a
            .mtime
            .cmp(&b.mtime)
            .then_with(|| natural_cmp(&a.name, &b.name)),
    }
}

/// Case-insensitive comparison that orders embedded digit runs numerically,
/// so `track2` precedes `track10`.
///
/// Digit runs compare by numeric value, so `"7"` and `"007"` are equal at
/// this stage; when that happens we fall back to a raw string comparison so
/// that two such names still get a stable, deterministic order instead of
/// shuffling between refreshes (`read_dir`'s enumeration order is not
/// guaranteed stable).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    natural_cmp_inner(a, b).then_with(|| a.cmp(b))
}

fn natural_cmp_inner(a: &str, b: &str) -> Ordering {
    let mut x = a.chars().peekable();
    let mut y = b.chars().peekable();

    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let na = take_number(&mut x);
                    let nb = take_number(&mut y);
                    match na.cmp(&nb) {
                        Ordering::Equal => continue,
                        other => return other,
                    }
                } else {
                    x.next();
                    y.next();
                    let la = ca.to_ascii_lowercase();
                    let lb = cb.to_ascii_lowercase();
                    match la.cmp(&lb) {
                        Ordering::Equal => continue,
                        other => return other,
                    }
                }
            }
        }
    }
}

/// Consumes a run of digits, saturating on absurdly long runs.
fn take_number(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u128 {
    let mut n: u128 = 0;
    while let Some(c) = it.peek().copied() {
        if !c.is_ascii_digit() {
            break;
        }
        it.next();
        n = n
            .saturating_mul(10)
            .saturating_add((c as u8 - b'0') as u128);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::EntryKind;
    use std::path::PathBuf;

    fn e(name: &str, kind: EntryKind, size: u64, mtime: i64) -> FsEntry {
        FsEntry {
            path: PathBuf::from(name),
            name: name.to_string(),
            ext: String::new(),
            size,
            mtime,
            kind,
            hidden: false,
        }
    }

    #[test]
    fn groups_before_columns() {
        let mut v = vec![
            e("zzz.wav", EntryKind::Audio, 1, 1),
            e("aaa.txt", EntryKind::Other, 1, 1),
            e("mmm", EntryKind::Directory, 0, 1),
        ];
        sort(&mut v, SortKey::Name, SortDir::Asc);
        assert_eq!(
            v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["mmm", "zzz.wav", "aaa.txt"]
        );
    }

    #[test]
    fn grouping_survives_descending_order() {
        let mut v = vec![
            e("b.wav", EntryKind::Audio, 1, 1),
            e("a", EntryKind::Directory, 0, 1),
            e("c.txt", EntryKind::Other, 1, 1),
        ];
        sort(&mut v, SortKey::Name, SortDir::Desc);
        assert_eq!(
            v[0].kind,
            EntryKind::Directory,
            "dirs stay first when reversed"
        );
        assert_eq!(v[2].kind, EntryKind::Other);
    }

    #[test]
    fn name_sort_is_case_insensitive() {
        let mut v = vec![
            e("Banana.wav", EntryKind::Audio, 1, 1),
            e("apple.wav", EntryKind::Audio, 1, 1),
        ];
        sort(&mut v, SortKey::Name, SortDir::Asc);
        assert_eq!(v[0].name, "apple.wav");
    }

    #[test]
    fn name_sort_is_natural_for_numbers() {
        let mut v = vec![
            e("track10.wav", EntryKind::Audio, 1, 1),
            e("track2.wav", EntryKind::Audio, 1, 1),
            e("track1.wav", EntryKind::Audio, 1, 1),
        ];
        sort(&mut v, SortKey::Name, SortDir::Asc);
        assert_eq!(
            v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["track1.wav", "track2.wav", "track10.wav"]
        );
    }

    #[test]
    fn sorts_by_size_and_mtime() {
        let mut v = vec![
            e("a.wav", EntryKind::Audio, 300, 10),
            e("b.wav", EntryKind::Audio, 100, 30),
            e("c.wav", EntryKind::Audio, 200, 20),
        ];
        sort(&mut v, SortKey::Size, SortDir::Asc);
        assert_eq!(v[0].name, "b.wav");

        sort(&mut v, SortKey::Modified, SortDir::Desc);
        assert_eq!(v[0].name, "b.wav");
    }

    #[test]
    fn ties_break_on_name_so_order_is_stable() {
        let mut v = vec![
            e("b.wav", EntryKind::Audio, 100, 1),
            e("a.wav", EntryKind::Audio, 100, 1),
        ];
        sort(&mut v, SortKey::Size, SortDir::Asc);
        assert_eq!(v[0].name, "a.wav");
    }

    #[test]
    fn natural_cmp_breaks_ties_between_equal_numeric_values() {
        // "007" and "7" compare numerically equal, but must still get a
        // deterministic, non-equal relative order via the raw-string tiebreak.
        assert_ne!(natural_cmp("007", "7"), Ordering::Equal);
        assert_eq!(natural_cmp("007", "7"), Ordering::Less);
        assert_eq!(natural_cmp("7", "007"), Ordering::Greater);
    }

    #[test]
    fn natural_cmp_still_orders_naturally_and_case_insensitively() {
        assert_eq!(natural_cmp("track2", "track10"), Ordering::Less);
        assert_eq!(natural_cmp("apple.wav", "Banana.wav"), Ordering::Less);
    }
}
