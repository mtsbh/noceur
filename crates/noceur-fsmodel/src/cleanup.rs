//! Cleaning up names and tag values with a set of named rules.
//!
//! The template renamer in [`crate::rename`] builds a name out of tags. This is
//! the other half of the job: the tags themselves, and the names of files nobody
//! ever tagged, arrive full of debris — `(Original Mix)` on every track of a
//! Beatport haul, the track number a ripper glued to the front, underscores from
//! a filesystem that could not take spaces, a forum's URL bracketed onto the end.
//!
//! Every rule here is deliberately narrow. A rule that is nearly right is worse
//! than no rule at all, because it runs across a whole folder before anyone sees
//! a result — so each one leaves alone anything it is unsure about, and the
//! caller previews every change regardless.

/// One cleanup rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Drop `(Original Mix)` and its relatives.
    Mix,
    /// Drop a track number from the front.
    TrackNumber,
    /// Underscores become spaces.
    Underscores,
    /// Dots become spaces, in names that use dots instead of them.
    Dots,
    /// Drop site names, bitrates and "Official Video".
    Junk,
    /// `ft` / `Feat` / `featuring` all become `feat.`.
    Feat,
    /// Collapse doubled spaces and even out the dashes.
    Spacing,
}

/// Every rule, with the text the interface shows for it.
///
/// One list rather than one per language: a label that lives in the front end
/// drifts from what the rule does the first time either of them changes.
pub const CATALOGUE: &[(&str, &str, &str)] = &[
    (
        "mix",
        "Remove (Original Mix)",
        "Also (Original), (Original Version), and the same words after a dash. Every other mix and edit is left alone.",
    ),
    (
        "track-number",
        "Remove leading track number",
        "01, 01., 01 -, (01). A single digit only goes when punctuation follows it, so 2 Bad Mice survives.",
    ),
    (
        "underscores",
        "Underscores to spaces",
        "Every _ becomes a space.",
    ),
    (
        "dots",
        "Dots to spaces",
        "Only for names that use dots instead of spaces, as scene releases do. A name that already has spaces is left alone.",
    ),
    (
        "junk",
        "Remove site names and bitrates",
        "www.something.com, [320kbps], (Official Video), (HQ) and the rest of what a download picks up.",
    ),
    (
        "feat",
        "Tidy featuring",
        "ft, Ft., FEAT and featuring all become feat.",
    ),
    (
        "spacing",
        "Tidy spacing",
        "Doubled spaces, dashes missing a space on one side, and punctuation left dangling.",
    ),
];

impl Rule {
    /// The name this rule is known by across the wire.
    pub fn slug(self) -> &'static str {
        match self {
            Rule::Mix => "mix",
            Rule::TrackNumber => "track-number",
            Rule::Underscores => "underscores",
            Rule::Dots => "dots",
            Rule::Junk => "junk",
            Rule::Feat => "feat",
            Rule::Spacing => "spacing",
        }
    }

    /// Reads a rule name, ignoring anything unrecognised.
    ///
    /// A slug this build does not know is dropped rather than refused: a saved
    /// setting naming a rule that has since gone should not stop the rest of the
    /// selection working.
    pub fn from_slug(slug: &str) -> Option<Rule> {
        [
            Rule::Mix,
            Rule::TrackNumber,
            Rule::Underscores,
            Rule::Dots,
            Rule::Junk,
            Rule::Feat,
            Rule::Spacing,
        ]
        .into_iter()
        .find(|r| r.slug() == slug)
    }
}

/// Applies the selected rules to one piece of text.
///
/// The order is fixed rather than taken from the caller, because the rules feed
/// each other: underscores have to become spaces before a track number is
/// visible in `01_Track`, and all of them leave debris only the tidying pass at
/// the end can see. Selecting nothing changes nothing at all — not even the
/// spacing.
pub fn clean(text: &str, rules: &[Rule]) -> String {
    if rules.is_empty() {
        return text.to_string();
    }
    let has = |r: Rule| rules.contains(&r);
    let mut s = text.to_string();

    if has(Rule::Underscores) {
        s = s.replace('_', " ");
    }
    // Guarded on there being no spaces already: `Artist.Name-Title-2001` is a
    // scene release and wants unpacking, while `Mr. Fingers` is a name and the
    // dot is the whole point.
    if has(Rule::Dots) && !s.contains(' ') {
        s = s.replace('.', " ");
    }
    if has(Rule::Junk) {
        s = strip_junk(&s);
    }
    if has(Rule::TrackNumber) {
        s = strip_track_number(&s);
    }
    if has(Rule::Mix) {
        s = strip_original(&s);
    }
    if has(Rule::Feat) {
        s = tidy_feat(&s);
    }

    // Always last, and always run: every rule above takes something out of the
    // middle of a name and leaves the spaces and brackets that surrounded it.
    tidy_spacing(&s)
}

/// Collapses whitespace, evens out the dashes and clears the debris.
fn tidy_spacing(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");

    // A dash with a space on one side only wanted one on both. A dash with no
    // space on either side is inside a word — `Hi-Fi`, `Lo-Fi`, `X-Coast` — and
    // is left exactly as it is.
    let spaced = collapsed.replace(" -", " - ").replace("- ", " - ");
    let respaced = spaced.split_whitespace().collect::<Vec<_>>().join(" ");

    crate::rename::tidy(&respaced)
}

/// The top-level bracketed groups in a string, as byte ranges with their inner text.
///
/// Nesting is counted, so `(Remix (2019))` comes back as one group. An
/// unbalanced bracket ends the search rather than guessing where it should have
/// closed: half a name is not worth removing.
fn groups(text: &str) -> Vec<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let open = bytes[i];
        let close = match open {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            _ => {
                i += 1;
                continue;
            }
        };

        let mut depth = 1usize;
        let mut j = i + 1;
        while j < bytes.len() {
            if bytes[j] == open {
                depth += 1;
            } else if bytes[j] == close {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            j += 1;
        }
        if depth != 0 {
            break;
        }

        out.push((i, j + 1, &text[i + 1..j]));
        i = j + 1;
    }
    out
}

/// Removes the bracketed groups a predicate accepts.
fn remove_groups(text: &str, unwanted: impl Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end, inner) in groups(text) {
        if unwanted(inner) {
            out.push_str(&text[last..start]);
            last = end;
        }
    }
    out.push_str(&text[last..]);
    out
}

/// Whether a bracketed group says "this is the plain version".
fn is_original(inner: &str) -> bool {
    matches!(
        inner.trim().to_lowercase().as_str(),
        "original mix" | "original" | "original version" | "original edit" | "originalmix"
    )
}

/// Drops `(Original Mix)` however it was written.
///
/// Only the words that mean "not a remix". `Extended Mix`, `Radio Edit`, `Dub`
/// and every remixer's name say which cut of a track this is, and losing them
/// turns two different files into the same name.
fn strip_original(text: &str) -> String {
    let out = remove_groups(text, is_original);

    // The unbracketed form: `Title - Original Mix`.
    if let Some(cut) = out.rfind('-') {
        if is_original(&out[cut + 1..]) {
            return out[..cut].to_string();
        }
    }
    out
}

/// Drops a track number from the front of a name.
fn strip_track_number(text: &str) -> String {
    let start = text.trim_start();

    // `(01) Title` and `[01] Title`.
    if start.starts_with(['(', '[', '{']) {
        if let Some((_, end, inner)) = groups(start).first() {
            let numeric =
                !inner.is_empty() && inner.len() <= 3 && inner.chars().all(|c| c.is_ascii_digit());
            let rest = start[*end..].trim_start();
            if numeric && !rest.is_empty() {
                return rest.to_string();
            }
        }
    }

    let digits = start.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return text.to_string();
    }
    let after = &start[digits..];

    // With punctuation it is unambiguous: nothing is titled `1.` or `03 -`.
    let punctuated = after
        .trim_start()
        .starts_with(['.', '-', '_', ')', ']', ':']);
    // Without it, only the padded two-digit form. `2 Bad Mice` and `4 Hero` are
    // artists, and a rule that renamed them would do more harm than the one it
    // fixes. `99 Luftballons` is the price of taking `01 Track`, which is what
    // the preview is for.
    let bare = digits == 2 && after.starts_with(' ');

    if !punctuated && !bare {
        return text.to_string();
    }

    let rest = after
        .trim_start()
        .trim_start_matches(['.', '-', '_', ')', ']', ':'])
        .trim_start();

    if rest.is_empty() {
        text.to_string()
    } else {
        rest.to_string()
    }
}

/// Site names, bitrates and the words a video rip carries.
const JUNK: &[&str] = &[
    "flac",
    "wav",
    "mp3",
    "aiff",
    "320",
    "256",
    "192",
    "128",
    "v0",
    "v2",
    "hq",
    "hd",
    "4k",
    "hq audio",
    "hd audio",
    "free",
    "free download",
    "free dl",
    "freedownload",
    "download",
    "official video",
    "official audio",
    "official music video",
    "official",
    "music video",
    "lyric video",
    "lyrics",
    "audio",
    "visualizer",
    "visualiser",
    "full album",
];

fn is_junk(inner: &str) -> bool {
    let t = inner.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }
    if t.contains("www.") || t.contains("http") {
        return true;
    }
    for tld in [".com", ".net", ".org", ".ru", ".io", ".co.uk"] {
        if t.contains(tld) {
            return true;
        }
    }
    // `320kbps`, `320 kbps`, and the same with any other number in front.
    if let Some(rest) = t.strip_suffix("kbps") {
        if rest.trim().chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    JUNK.contains(&t.as_str())
}

/// Removes junk, bracketed or bare.
fn strip_junk(text: &str) -> String {
    let without_groups = remove_groups(text, is_junk);

    // A bare URL, which is how most of them are actually glued on:
    // `Artist - Title -www.somesite.com`.
    without_groups
        .split_whitespace()
        .filter(|token| {
            let t = token.to_lowercase();
            !(t.contains("www.") || t.starts_with("http"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Settles on one spelling of "featuring".
///
/// The word turns up four ways across one folder, which means four different
/// names for the same pairing of artists whenever anything sorts or groups by it.
fn tidy_feat(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for (i, token) in text.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        // The bracket stays where it is: `(feat. X)` keeps its brackets, and
        // only the word inside them is rewritten.
        let lead = token.len() - token.trim_start_matches(['(', '[', '{']).len();
        let core = token[lead..].trim_end_matches('.');

        if matches!(core.to_lowercase().as_str(), "ft" | "feat" | "featuring") {
            out.push_str(&token[..lead]);
            out.push_str("feat.");
        } else {
            out.push_str(token);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, slugs: &[&str]) -> String {
        let rules: Vec<Rule> = slugs.iter().filter_map(|s| Rule::from_slug(s)).collect();
        clean(text, &rules)
    }

    #[test]
    fn original_mix_goes_however_it_was_written() {
        let r = ["mix"];
        assert_eq!(run("Rev8617 (Original Mix)", &r), "Rev8617");
        assert_eq!(run("Rev8617 [Original Mix]", &r), "Rev8617");
        assert_eq!(run("Rev8617 (original mix)", &r), "Rev8617");
        assert_eq!(run("Rev8617 (Original)", &r), "Rev8617");
        assert_eq!(run("Rev8617 - Original Mix", &r), "Rev8617");
        assert_eq!(
            run("Artist - Rev8617 (Original Mix)", &r),
            "Artist - Rev8617"
        );
    }

    /// The rule has to know the difference between "this is not a remix" and
    /// which cut of a track this is. Getting it wrong collapses two files onto
    /// one name.
    #[test]
    fn every_other_mix_survives() {
        let r = ["mix"];
        for name in [
            "Rev8617 (Extended Mix)",
            "Rev8617 (Radio Edit)",
            "Rev8617 (Dub Mix)",
            "Rev8617 (Skee Mask Remix)",
            "Rev8617 (Original Sin Remix)",
            "Rev8617 (Club Mix)",
        ] {
            assert_eq!(run(name, &r), name, "{name}");
        }
    }

    #[test]
    fn track_numbers_come_off_the_front() {
        let r = ["track-number"];
        assert_eq!(run("01 Rev8617", &r), "Rev8617");
        assert_eq!(run("01. Rev8617", &r), "Rev8617");
        assert_eq!(run("01 - Rev8617", &r), "Rev8617");
        assert_eq!(run("1. Rev8617", &r), "Rev8617");
        assert_eq!(run("(01) Rev8617", &r), "Rev8617");
        assert_eq!(run("[03] Rev8617", &r), "Rev8617");
        assert_eq!(run("003 - Rev8617", &r), "Rev8617");
    }

    /// A number at the front of a name is not always a track number, and the
    /// ones that are not belong to well known artists.
    #[test]
    fn a_number_that_is_part_of_the_name_stays() {
        let r = ["track-number"];
        assert_eq!(run("2 Bad Mice - Bombscare", &r), "2 Bad Mice - Bombscare");
        assert_eq!(run("4 Hero - Mr Kirk", &r), "4 Hero - Mr Kirk");
        assert_eq!(run("808 State - Pacific", &r), "808 State - Pacific");
    }

    #[test]
    fn a_name_that_is_only_a_number_is_left_alone() {
        assert_eq!(run("01", &["track-number"]), "01");
    }

    #[test]
    fn underscores_become_spaces() {
        assert_eq!(
            run("Skee_Mask_-_Rev8617", &["underscores"]),
            "Skee Mask - Rev8617"
        );
    }

    /// Underscores first, so the track number is recognisable afterwards.
    #[test]
    fn the_rules_feed_each_other() {
        assert_eq!(
            run(
                "01_Skee_Mask_-_Rev8617_(Original_Mix)",
                &["underscores", "track-number", "mix"]
            ),
            "Skee Mask - Rev8617"
        );
    }

    #[test]
    fn dots_only_open_up_a_name_that_has_no_spaces() {
        assert_eq!(
            run("Skee.Mask.Rev8617.2016", &["dots"]),
            "Skee Mask Rev8617 2016"
        );
        assert_eq!(
            run("Mr. Fingers - Can You Feel It", &["dots"]),
            "Mr. Fingers - Can You Feel It"
        );
    }

    #[test]
    fn downloads_lose_what_they_picked_up() {
        let r = ["junk"];
        assert_eq!(run("Rev8617 [320kbps]", &r), "Rev8617");
        assert_eq!(run("Rev8617 (320 kbps)", &r), "Rev8617");
        assert_eq!(run("Rev8617 [www.somesite.com]", &r), "Rev8617");
        assert_eq!(run("Rev8617 -www.somesite.com", &r), "Rev8617");
        assert_eq!(run("Rev8617 (Official Video)", &r), "Rev8617");
        assert_eq!(run("Rev8617 (Official Audio) [HQ]", &r), "Rev8617");
        assert_eq!(run("Rev8617 [FLAC]", &r), "Rev8617");
    }

    /// The junk list is whole groups, not substrings: a title that contains one
    /// of those words is a title.
    #[test]
    fn junk_words_inside_a_real_title_stay() {
        let r = ["junk"];
        assert_eq!(run("Free Your Mind", &r), "Free Your Mind");
        assert_eq!(run("Rev8617 (Audio Remix)", &r), "Rev8617 (Audio Remix)");
    }

    #[test]
    fn featuring_settles_on_one_spelling() {
        let r = ["feat"];
        assert_eq!(run("Artist ft. Other", &r), "Artist feat. Other");
        assert_eq!(run("Artist Ft Other", &r), "Artist feat. Other");
        assert_eq!(run("Artist FEATURING Other", &r), "Artist feat. Other");
        assert_eq!(run("Artist (feat Other)", &r), "Artist (feat. Other)");
        assert_eq!(run("Artist feat. Other", &r), "Artist feat. Other");
    }

    #[test]
    fn words_that_merely_start_the_same_are_not_touched() {
        assert_eq!(
            run("Feature Presentation", &["feat"]),
            "Feature Presentation"
        );
    }

    #[test]
    fn spacing_is_evened_out() {
        let r = ["spacing"];
        assert_eq!(run("Skee  Mask   -  Rev8617", &r), "Skee Mask - Rev8617");
        assert_eq!(run("Skee Mask -Rev8617", &r), "Skee Mask - Rev8617");
        assert_eq!(run("Skee Mask- Rev8617", &r), "Skee Mask - Rev8617");
        assert_eq!(run("  Rev8617  ", &r), "Rev8617");
    }

    /// A dash inside a word is part of the word.
    #[test]
    fn hyphenated_words_keep_their_hyphen() {
        assert_eq!(
            run("Lo-Fi Hi-Fi X-Coast", &["spacing"]),
            "Lo-Fi Hi-Fi X-Coast"
        );
    }

    /// Taking something out of the middle of a name leaves a hole, and the hole
    /// is the part people notice.
    #[test]
    fn nothing_is_left_dangling_where_a_rule_cut() {
        assert_eq!(run("Rev8617 () - ", &["spacing"]), "Rev8617");
        assert_eq!(
            run("01 - Rev8617 (Original Mix)", &["track-number", "mix"]),
            "Rev8617"
        );
    }

    /// Selecting no rules is not a request to reformat everything.
    #[test]
    fn no_rules_changes_nothing() {
        assert_eq!(run("  01_Weird   name  ", &[]), "  01_Weird   name  ");
    }

    #[test]
    fn every_catalogue_entry_is_a_real_rule() {
        for (slug, label, hint) in CATALOGUE {
            let rule = Rule::from_slug(slug).unwrap_or_else(|| panic!("no rule for {slug}"));
            assert_eq!(rule.slug(), *slug);
            assert!(!label.is_empty() && !hint.is_empty(), "{slug}");
        }
    }

    #[test]
    fn an_unknown_rule_name_is_ignored_rather_than_fatal() {
        assert!(Rule::from_slug("no-such-rule").is_none());
        assert_eq!(
            run("01 Rev8617", &["no-such-rule", "track-number"]),
            "Rev8617"
        );
    }
}
