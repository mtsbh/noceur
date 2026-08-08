//! Musical key parsing and notation conversion.

/// Notation used to display a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyNotation {
    Camelot,
    OpenKey,
    Classical,
}

/// A musical key, stored as its position on the Camelot wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicalKey {
    /// 1..=12
    pub camelot_number: u8,
    /// `true` for the A (minor) side of the wheel.
    pub is_minor: bool,
}

/// Classical spelling for each Camelot position: (minor, major).
/// Index 0 is Camelot 1.
const CLASSICAL: [(&str, &str); 12] = [
    ("Abm", "B"),  //  1
    ("Ebm", "F#"), //  2
    ("Bbm", "Db"), //  3
    ("Fm", "Ab"),  //  4
    ("Cm", "Eb"),  //  5
    ("Gm", "Bb"),  //  6
    ("Dm", "F"),   //  7
    ("Am", "C"),   //  8
    ("Em", "G"),   //  9
    ("Bm", "D"),   // 10
    ("F#m", "A"),  // 11
    ("Dbm", "E"),  // 12
];

/// Pitch-class index (0 = C) for every accepted root spelling.
///
/// Declared with all two-character roots (with an accidental) before the
/// single-character roots that are their prefix.
const ROOTS: &[(&str, u8)] = &[
    ("b#", 0),
    ("c#", 1),
    ("db", 1),
    ("d#", 3),
    ("eb", 3),
    ("fb", 4),
    ("e#", 5),
    ("f#", 6),
    ("gb", 6),
    ("g#", 8),
    ("ab", 8),
    ("a#", 10),
    ("bb", 10),
    ("cb", 11),
    ("c", 0),
    ("d", 2),
    ("e", 4),
    ("f", 5),
    ("g", 7),
    ("a", 9),
    ("b", 11),
];

/// Camelot number for each pitch class, minor side. `Am` (pitch class 9) is 8A.
const MINOR_PC_TO_CAMELOT: [u8; 12] = [5, 12, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10];
/// Camelot number for each pitch class, major side. `C` (pitch class 0) is 8B.
const MAJOR_PC_TO_CAMELOT: [u8; 12] = [8, 3, 10, 5, 12, 7, 2, 9, 4, 11, 6, 1];

impl MusicalKey {
    /// Canonical sort value, 0..=23. Orders by wheel position, minor before major.
    pub fn canonical(&self) -> i32 {
        (self.camelot_number as i32 - 1) * 2 + if self.is_minor { 0 } else { 1 }
    }

    /// Rebuilds a key from its canonical value. Returns `None` if out of range.
    pub fn from_canonical(v: i32) -> Option<Self> {
        if !(0..24).contains(&v) {
            return None;
        }
        Some(MusicalKey {
            camelot_number: (v / 2) as u8 + 1,
            is_minor: v % 2 == 0,
        })
    }

    /// Open Key number, 1..=12. Open Key 1 is Camelot 8.
    fn open_key_number(&self) -> u8 {
        ((self.camelot_number + 4) % 12) + 1
    }

    /// Parses Camelot, Open Key or classical notation. Whitespace-tolerant and
    /// case-insensitive. Returns `None` for anything unrecognised.
    pub fn parse(input: &str) -> Option<Self> {
        let s = input.trim();
        if s.is_empty() {
            return None;
        }
        parse_wheel(s).or_else(|| parse_classical(s))
    }

    /// Renders the key in the requested notation.
    pub fn format(&self, notation: KeyNotation) -> String {
        let idx = (self.camelot_number - 1) as usize;
        match notation {
            KeyNotation::Camelot => {
                format!(
                    "{}{}",
                    self.camelot_number,
                    if self.is_minor { 'A' } else { 'B' }
                )
            }
            KeyNotation::OpenKey => {
                format!(
                    "{}{}",
                    self.open_key_number(),
                    if self.is_minor { 'm' } else { 'd' }
                )
            }
            KeyNotation::Classical => {
                let (minor, major) = CLASSICAL[idx];
                (if self.is_minor { minor } else { major }).to_string()
            }
        }
    }
}

/// Parses `8A` / `8a` (Camelot) and `1m` / `1d` (Open Key).
fn parse_wheel(s: &str) -> Option<MusicalKey> {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let (digits, suffix) = compact.split_at(compact.find(|c: char| !c.is_ascii_digit())?);

    let n: u8 = digits.parse().ok()?;
    if !(1..=12).contains(&n) {
        return None;
    }

    match suffix.to_ascii_lowercase().as_str() {
        "a" => Some(MusicalKey {
            camelot_number: n,
            is_minor: true,
        }),
        "b" => Some(MusicalKey {
            camelot_number: n,
            is_minor: false,
        }),
        // Open Key: number is offset by 7 positions from Camelot.
        "m" => Some(MusicalKey {
            camelot_number: ((n + 6) % 12) + 1,
            is_minor: true,
        }),
        "d" => Some(MusicalKey {
            camelot_number: ((n + 6) % 12) + 1,
            is_minor: false,
        }),
        _ => None,
    }
}

/// Parses `Am`, `A minor`, `C`, `Cmaj`, `F#m`, `A-moll` and similar.
fn parse_classical(s: &str) -> Option<MusicalKey> {
    let lower = s.to_ascii_lowercase();
    let cleaned: String = lower
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_' | '.'))
        .collect();

    // ROOTS is declared with two-character roots (with an accidental) before
    // their one-character prefix, so e.g. `c#` is tried before `c`. This
    // ordering isn't actually load-bearing for correctness: no accepted mode
    // suffix (`""`, `maj`, `major`, `dur`, `m`, `min`, `minor`, `moll`)
    // begins with `#` or `b`, so if a short root like `c` wrongly matches the
    // start of `c#m`, the remainder (`#m`) matches no suffix and the loop
    // just continues on to the correct longer root. Adding a suffix that
    // starts with an accidental would break that safety net.
    for (root, pc) in ROOTS {
        let Some(rest) = cleaned.strip_prefix(root) else {
            continue;
        };
        let is_minor = match rest {
            "" | "maj" | "major" | "dur" => false,
            "m" | "min" | "minor" | "moll" => true,
            _ => continue,
        };
        let table = if is_minor {
            MINOR_PC_TO_CAMELOT
        } else {
            MAJOR_PC_TO_CAMELOT
        };
        return Some(MusicalKey {
            camelot_number: table[*pc as usize],
            is_minor,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_camelot() {
        let k = MusicalKey::parse("8A").unwrap();
        assert_eq!(k.camelot_number, 8);
        assert!(k.is_minor);
        assert_eq!(k.canonical(), 14);

        let k = MusicalKey::parse("12B").unwrap();
        assert_eq!(k.camelot_number, 12);
        assert!(!k.is_minor);
    }

    #[test]
    fn camelot_parsing_is_case_and_space_insensitive() {
        assert_eq!(
            MusicalKey::parse(" 8a ").unwrap(),
            MusicalKey::parse("8A").unwrap()
        );
    }

    #[test]
    fn parses_open_key() {
        // Open Key 1m is A minor, which is Camelot 8A.
        let k = MusicalKey::parse("1m").unwrap();
        assert_eq!(k.camelot_number, 8);
        assert!(k.is_minor);

        // Open Key 1d is C major, which is Camelot 8B.
        let k = MusicalKey::parse("1d").unwrap();
        assert_eq!(k.camelot_number, 8);
        assert!(!k.is_minor);
    }

    #[test]
    fn parses_classical_minor_and_major() {
        let am = MusicalKey::parse("Am").unwrap();
        assert_eq!(am.camelot_number, 8);
        assert!(am.is_minor);

        let c = MusicalKey::parse("C").unwrap();
        assert_eq!(c.camelot_number, 8);
        assert!(!c.is_minor);
    }

    #[test]
    fn accepts_classical_spelling_variants() {
        for s in ["Am", "A min", "A minor", "amin", "A-moll"] {
            assert_eq!(
                MusicalKey::parse(s).map(|k| k.canonical()),
                Some(MusicalKey::parse("Am").unwrap().canonical()),
                "{s} should parse as A minor"
            );
        }
        for s in ["C", "C maj", "C major", "Cmaj"] {
            assert_eq!(
                MusicalKey::parse(s).map(|k| k.canonical()),
                Some(MusicalKey::parse("C").unwrap().canonical()),
                "{s} should parse as C major"
            );
        }
    }

    #[test]
    fn treats_enharmonics_as_equal() {
        assert_eq!(
            MusicalKey::parse("F#m").unwrap().canonical(),
            MusicalKey::parse("Gbm").unwrap().canonical()
        );
        assert_eq!(
            MusicalKey::parse("Db").unwrap().canonical(),
            MusicalKey::parse("C#").unwrap().canonical()
        );
    }

    #[test]
    fn rejects_nonsense() {
        for s in ["", "  ", "13A", "0A", "H", "banana", "8C", "9x"] {
            assert!(MusicalKey::parse(s).is_none(), "{s} should not parse");
        }
    }

    #[test]
    fn formats_in_every_notation() {
        let k = MusicalKey::parse("8A").unwrap();
        assert_eq!(k.format(KeyNotation::Camelot), "8A");
        assert_eq!(k.format(KeyNotation::OpenKey), "1m");
        assert_eq!(k.format(KeyNotation::Classical), "Am");

        let k = MusicalKey::parse("C").unwrap();
        assert_eq!(k.format(KeyNotation::Camelot), "8B");
        assert_eq!(k.format(KeyNotation::OpenKey), "1d");
        assert_eq!(k.format(KeyNotation::Classical), "C");
    }

    #[test]
    fn canonical_round_trips_through_every_notation() {
        for n in 1..=12u8 {
            for minor in [true, false] {
                let src = MusicalKey {
                    camelot_number: n,
                    is_minor: minor,
                };
                for notation in [
                    KeyNotation::Camelot,
                    KeyNotation::OpenKey,
                    KeyNotation::Classical,
                ] {
                    let text = src.format(notation);
                    let back = MusicalKey::parse(&text)
                        .unwrap_or_else(|| panic!("failed to reparse {text} ({notation:?})"));
                    assert_eq!(back, src, "round trip failed for {text}");
                }
            }
        }
    }

    #[test]
    fn canonical_values_are_unique_and_bounded() {
        let mut seen = std::collections::HashSet::new();
        for n in 1..=12u8 {
            for minor in [true, false] {
                let c = MusicalKey {
                    camelot_number: n,
                    is_minor: minor,
                }
                .canonical();
                assert!((0..24).contains(&c));
                assert!(seen.insert(c), "duplicate canonical value {c}");
            }
        }
        assert_eq!(seen.len(), 24);
    }
}
