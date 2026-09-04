//! Deciding whether a search result is the track in front of you.
//!
//! Every source answers a query with things that are roughly what was asked
//! for. Turning "roughly" into a ranking is the whole difficulty of an
//! autotagger: a library is full of `Artist - Title (Someone Else Remix)`,
//! `(Original Mix)`, `[FREE DOWNLOAD]` and `(Unreleased)`, and a naive string
//! compare either rejects the right answer or accepts the wrong one.
//!
//! Nothing here reaches the network, so all of it is testable — which matters,
//! because this is the part that decides what gets written into someone's files.

/// How similar two strings are, 0.0 to 1.0.
///
/// Token-based rather than character-based. Edit distance says
/// `Rrose - Wedge` and `Rrose - Hedge` are nearly identical, which is exactly
/// wrong; and it says `Skee Mask - Rev8617` and `Rev8617 - Skee Mask` are
/// distant, which is also wrong. Comparing sets of words handles reordering,
/// punctuation and the wildly different spacing conventions of a dozen
/// databases without pretending two different words are one.
pub fn similarity(a: &str, b: &str) -> f32 {
    let left = tokens(a);
    let right = tokens(b);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }

    let shared = left.iter().filter(|t| right.contains(*t)).count();
    // Divided by the larger side, so a title that is a prefix of a much longer
    // one does not score as a perfect match: `Track` against
    // `Track (Extended Club Mix)` is a real difference worth seeing.
    let denominator = left.len().max(right.len());
    shared as f32 / denominator as f32
}

/// Splits text into comparable words.
///
/// Lower-cased, punctuation dropped, and each word kept whole. Numbers are kept
/// because they carry meaning in this catalogue — `Selectors 006` is not
/// `Selectors 005`, and `Rev8617` is a title.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            word.extend(c.to_lowercase());
        } else if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// What was parsed out of a file with nothing useful in its tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Guess {
    pub artist: String,
    pub title: String,
}

/// Reads an artist and title out of a file name.
///
/// A DJ library is mostly named `Artist - Title`, and the files that most need
/// tagging are exactly the ones with no tags to search by — promos, rips, things
/// pulled off a USB stick. The leading track number that rippers add is dropped,
/// and so is the extension.
///
/// Returns everything as the title when there is no separator, because a title
/// alone still searches usefully and a wrong guess at the artist does not.
pub fn from_file_name(name: &str) -> Guess {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);

    // `Artist_-_Title` is as common as the spaced form and parses as neither an
    // artist nor a title without this — the whole name lands in the title and
    // the search goes out with the artist blank, which is the difference between
    // a confident match and none. Only for names with no spaces at all, so an
    // underscore inside an otherwise ordinary name is left where it is.
    let opened;
    let stem = if !stem.contains(' ') && stem.contains('_') {
        opened = stem.replace('_', " ");
        opened.as_str()
    } else {
        stem
    };

    let stem = strip_leading_track_number(stem);

    // ` - ` with spaces, which is the convention; a bare hyphen is left alone
    // because it turns up inside titles far more often than between fields.
    match stem.split_once(" - ") {
        Some((artist, title)) => Guess {
            artist: artist.trim().to_string(),
            title: title.trim().to_string(),
        },
        None => Guess {
            artist: String::new(),
            title: stem.trim().to_string(),
        },
    }
}

/// Drops `01 `, `01. `, `01 - ` and `A1 ` from the front of a name.
///
/// Rippers and vinyl conventions both put a position there. It is not part of
/// the artist, and leaving it in poisons every search that uses the name.
fn strip_leading_track_number(stem: &str) -> &str {
    let trimmed = stem.trim_start();
    let digits: usize = trimmed
        .chars()
        // A vinyl side letter, as in `A1` or `B2`.
        .skip(usize::from(
            trimmed.starts_with(|c: char| c.is_ascii_alphabetic())
                && trimmed.len() > 1
                && trimmed[1..].starts_with(|c: char| c.is_ascii_digit()),
        ))
        .take_while(|c| c.is_ascii_digit())
        .count();
    if digits == 0 {
        return stem;
    }

    let after = &trimmed[digits + usize::from(trimmed.starts_with(|c: char| c.is_alphabetic()))..];
    let after = after.trim_start_matches(['.', ')', '-', ' ']);
    // Only if something is left: a file genuinely called `01` keeps its name.
    if after.is_empty() {
        stem
    } else {
        after
    }
}

/// Text that says nothing about which recording this is.
///
/// These appear in file names constantly and in databases almost never, so
/// leaving them in makes every candidate look worse than it is. `Remix` is
/// deliberately absent: a remix really is a different recording.
const NOISE: &[&str] = &[
    "original mix",
    "original",
    "free download",
    "free dl",
    "out now",
    "premiere",
    "hq",
    "hd",
    "320",
    "www",
    "mp3",
    "master",
    "preview",
];

/// Removes bracketed noise and stray decoration from a title.
///
/// Only the phrases above, and only when they are the whole of a bracketed
/// group: `(Original Mix)` goes, `(Marcel Dettmann Remix)` stays, and so does a
/// title that happens to contain the word "premiere".
pub fn clean_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut depth = 0usize;
    let mut group = String::new();

    for c in title.chars() {
        match c {
            '(' | '[' => {
                if depth == 0 {
                    group.clear();
                } else {
                    group.push(c);
                }
                depth += 1;
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let inner = group.trim().to_ascii_lowercase();
                    if !NOISE.contains(&inner.as_str()) {
                        out.push('(');
                        out.push_str(group.trim());
                        out.push(')');
                    }
                } else {
                    group.push(c);
                }
            }
            _ if depth > 0 => group.push(c),
            _ => out.push(c),
        }
    }
    // An unclosed bracket means the group was never decided on; keep the text.
    if depth > 0 {
        out.push_str(&group);
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How well a candidate matches what was searched for, 0.0 to 1.0.
///
/// Title carries more weight than artist. Artist credits differ wildly between
/// databases — features, `&` against `and`, remixers listed or not — while a
/// title that does not match usually means a different recording. Duration is a
/// tiebreak rather than a gate: it is the one field that cannot be spelled
/// differently, but plenty of sources report a radio edit's length for an
/// extended mix.
pub fn score(
    want_artist: &str,
    want_title: &str,
    got_artist: &str,
    got_title: &str,
    want_ms: Option<u64>,
    got_ms: Option<u64>,
) -> f32 {
    let title = similarity(&clean_title(want_title), &clean_title(got_title));
    let artist = similarity(want_artist, got_artist);

    // A file with no artist to compare against should not be penalised for it;
    // the title then carries the whole judgement.
    let base = if want_artist.trim().is_empty() {
        title
    } else {
        title * 0.7 + artist * 0.3
    };

    match (want_ms, got_ms) {
        (Some(a), Some(b)) if a > 0 && b > 0 => {
            let diff = a.abs_diff(b);
            // Within three seconds is the same recording as far as anyone can
            // tell; beyond thirty it is a different edit and the bonus is gone.
            let closeness = if diff <= 3_000 {
                1.0
            } else if diff >= 30_000 {
                0.0
            } else {
                1.0 - (diff - 3_000) as f32 / 27_000.0
            };
            (base * 0.85 + closeness * 0.15).clamp(0.0, 1.0)
        }
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_matches_completely() {
        assert_eq!(similarity("Skee Mask", "Skee Mask"), 1.0);
        assert_eq!(similarity("SKEE  mask!", "skee mask"), 1.0);
    }

    /// The case edit distance gets wrong: one different word is a different
    /// track, however few letters separate them.
    #[test]
    fn one_different_word_is_a_real_difference() {
        assert!(similarity("Rrose Wedge", "Rrose Hedge") < 0.6);
    }

    #[test]
    fn word_order_does_not_matter() {
        assert_eq!(similarity("Skee Mask Rev8617", "Rev8617 Skee Mask"), 1.0);
    }

    /// A shorter title being contained in a longer one is a difference worth
    /// seeing — an extended club mix is not the original.
    #[test]
    fn extra_words_lower_the_score() {
        let s = similarity("Track", "Track Extended Club Mix");
        assert!(s > 0.0 && s < 0.5, "got {s}");
    }

    #[test]
    fn nothing_matches_nothing() {
        assert_eq!(similarity("", "anything"), 0.0);
        assert_eq!(similarity("anything", ""), 0.0);
    }

    #[test]
    fn a_conventional_file_name_splits_into_artist_and_title() {
        assert_eq!(
            from_file_name("Afriqua - Cheep Spouse.wav"),
            Guess {
                artist: "Afriqua".into(),
                title: "Cheep Spouse".into()
            }
        );
    }

    /// The files that most need tagging are the ones with no tags, so the name
    /// has to be read carefully. A leading position number is not the artist.
    #[test]
    fn leading_track_numbers_are_dropped() {
        for name in [
            "01 Skee Mask - Rev8617.flac",
            "01. Skee Mask - Rev8617.flac",
            "01 - Skee Mask - Rev8617.flac",
            "A1 Skee Mask - Rev8617.flac",
        ] {
            assert_eq!(
                from_file_name(name),
                Guess {
                    artist: "Skee Mask".into(),
                    title: "Rev8617".into()
                },
                "failed on {name}"
            );
        }
    }

    /// A wrong guess at the artist is worse than no guess: it drags every real
    /// match down the ranking.
    #[test]
    fn a_name_with_no_separator_is_all_title() {
        assert_eq!(
            from_file_name("mystery track.mp3"),
            Guess {
                artist: String::new(),
                title: "mystery track".into()
            }
        );
    }

    #[test]
    fn a_file_called_only_a_number_keeps_its_name() {
        assert_eq!(from_file_name("01.wav").title, "01");
    }

    /// The underscored form is as common as the spaced one, and without this it
    /// searches with a blank artist and the whole name as the title — which is
    /// the difference between a confident match and none.
    #[test]
    fn an_underscored_name_parses_like_a_spaced_one() {
        assert_eq!(
            from_file_name("Afriqua_-_Cheep_Spouse.wav"),
            Guess {
                artist: "Afriqua".into(),
                title: "Cheep Spouse".into()
            }
        );
        assert_eq!(
            from_file_name("01_Afriqua_-_Cheep_Spouse.wav"),
            Guess {
                artist: "Afriqua".into(),
                title: "Cheep Spouse".into()
            }
        );
    }

    /// An underscore inside a name that is otherwise spaced normally is part of
    /// the name, not a separator someone chose.
    #[test]
    fn an_underscore_in_an_ordinary_name_stays() {
        assert_eq!(
            from_file_name("Some Artist - Track_01.wav"),
            Guess {
                artist: "Some Artist".into(),
                title: "Track_01".into()
            }
        );
    }

    /// `(Original Mix)` is in half a DJ library's file names and in almost no
    /// database, so leaving it in makes every real match look worse.
    #[test]
    fn decoration_that_says_nothing_is_removed() {
        assert_eq!(clean_title("Czill (Original Mix)"), "Czill");
        assert_eq!(clean_title("Czill [FREE DOWNLOAD]"), "Czill");
        assert_eq!(clean_title("Czill (PREMIERE)"), "Czill");
    }

    /// The other half of the rule, and the one that matters more: a remix is a
    /// different recording and must not be folded into the original.
    #[test]
    fn a_remix_credit_is_not_decoration() {
        assert_eq!(
            clean_title("Czill (Marcel Dettmann Remix)"),
            "Czill (Marcel Dettmann Remix)"
        );
        assert_eq!(clean_title("Premiere Nights"), "Premiere Nights");
    }

    #[test]
    fn the_right_track_outscores_a_near_miss() {
        let right = score(
            "Afriqua",
            "Cheep Spouse",
            "Afriqua",
            "Cheep Spouse",
            None,
            None,
        );
        let wrong = score("Afriqua", "Cheep Spouse", "Afriqua", "Herod", None, None);
        assert!(right > 0.95, "got {right}");
        assert!(right > wrong * 2.0, "right {right} vs wrong {wrong}");
    }

    /// Title is weighted above artist because artist credits differ between
    /// databases far more often than titles do.
    #[test]
    fn a_matching_title_survives_a_differently_credited_artist() {
        let s = score(
            "Afriqua",
            "Cheep Spouse",
            "Afriqua & Friends",
            "Cheep Spouse",
            None,
            None,
        );
        assert!(s > 0.75, "got {s}");
    }

    #[test]
    fn a_matching_duration_breaks_a_tie_upwards() {
        let close = score("A", "T", "A", "T", Some(300_000), Some(301_000));
        let far = score("A", "T", "A", "T", Some(300_000), Some(400_000));
        assert!(close > far, "close {close} vs far {far}");
    }

    /// Duration is a tiebreak, not a gate: plenty of sources report a radio
    /// edit's length for an extended mix, and refusing on that would throw away
    /// correct matches.
    #[test]
    fn a_wrong_duration_does_not_reject_an_otherwise_perfect_match() {
        let s = score("A", "T", "A", "T", Some(120_000), Some(600_000));
        assert!(s > 0.8, "got {s}");
    }
}
