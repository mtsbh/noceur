//! Building file names from a template.
//!
//! Renaming is the one bulk operation with no undo worth the name: a folder
//! renamed wrongly is a folder you have to identify and repair by hand, and the
//! damage is done before anyone sees a single result. So the whole of this
//! module is about producing a name and nothing else — the caller previews every
//! one, and only then moves anything.

use std::collections::HashMap;

/// What a template can refer to.
///
/// Deliberately a map rather than a struct: the fields a DJ names files by are
/// whatever their tags happen to carry, and a new one should not need a change
/// here to become usable.
pub type Fields<'a> = HashMap<&'a str, String>;

/// Fills a template with a track's fields.
///
/// `%artist% - %title%` is the shape almost every DJ library uses, so that is
/// the syntax. A field with no value collapses along with the punctuation around
/// it: a track with no artist produces `Title`, not ` - Title`, which is the
/// difference between a rename that is useful on a folder of mixed files and one
/// that is only useful on a perfectly tagged album.
///
/// Unknown field names are left as written rather than emptied, so a typo shows
/// up in the preview as itself instead of quietly deleting part of every name.
pub fn fill(template: &str, fields: &Fields) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;

    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];

        let Some(end) = after.find('%') else {
            // An unclosed `%` is a literal one, not the start of a field.
            out.push('%');
            rest = after;
            continue;
        };

        let name = &after[..end];
        match fields.get(name) {
            Some(value) if !value.trim().is_empty() => out.push_str(value.trim()),
            // Known but empty: nothing is emitted, and the separators left on
            // either side are collapsed by `tidy`. Stripping the one in front
            // here instead would leave the closing half of a bracket behind.
            Some(_) => {}
            None => {
                out.push('%');
                out.push_str(name);
                out.push('%');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);

    tidy(&out)
}

/// Collapses the debris an absent field leaves behind.
///
/// Empty brackets, doubled separators and leading or trailing punctuation all
/// come from the same cause and all look like a bug in the name.
pub(crate) fn tidy(text: &str) -> String {
    let mut s = text.to_string();
    for (from, to) in [
        ("()", ""),
        ("[]", ""),
        ("{}", ""),
        ("  ", " "),
        (" - - ", " - "),
        ("- -", "-"),
    ] {
        while s.contains(from) {
            s = s.replace(from, to);
        }
    }
    s.trim().trim_matches(|c| " -–—_".contains(c)).to_string()
}

/// Characters Windows refuses in a file name, and what they become.
///
/// Substituted rather than deleted: a title containing `AC/DC` should become
/// `AC-DC`, not `ACDC`, and a `?` in a title is usually doing something. The
/// replacements are the ones every tagger has settled on.
const FORBIDDEN: &[(char, &str)] = &[
    ('<', "("),
    ('>', ")"),
    (':', " -"),
    ('"', "'"),
    ('/', "-"),
    ('\\', "-"),
    ('|', "-"),
    ('?', ""),
    ('*', ""),
];

/// Makes a name the filesystem will accept.
///
/// Windows is the strict one and the target here: it forbids nine characters,
/// forbids a trailing dot or space, and reserves a handful of device names that
/// have been unusable since DOS. A name that trips any of those fails at the
/// point of renaming, halfway through a folder.
pub fn sanitise(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match FORBIDDEN.iter().find(|(bad, _)| *bad == c) {
            Some((_, replacement)) => out.push_str(replacement),
            // Control characters have no business in a name and no useful
            // replacement; they are dropped rather than substituted.
            None if (c as u32) < 0x20 => {}
            None => out.push(c),
        }
    }

    let out = out.trim().trim_end_matches('.').trim().to_string();
    if is_reserved(&out) {
        format!("_{out}")
    } else {
        out
    }
}

/// Whether a name is one of the DOS device names Windows still reserves.
///
/// `CON`, `PRN`, `NUL` and the numbered ports, with or without an extension.
/// A file called any of them cannot be created, and the failure is baffling.
fn is_reserved(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = name.split('.').next().unwrap_or(name);
    RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r))
}

/// Longest name this will produce, in characters.
///
/// Windows allows 255 per path component. Leaving room below it keeps a rename
/// from failing on a deeply nested folder, and a name that long is unreadable
/// anyway.
pub const MAX_NAME: usize = 180;

/// Builds the final file name: template, then the rules, then the extension.
///
/// The extension is carried over rather than templated. A rename that changed
/// `.flac` to `.mp3` would produce a file every program refuses to open, and
/// nothing about renaming should be able to do that.
pub fn build(template: &str, fields: &Fields, extension: &str) -> String {
    let mut stem = sanitise(&fill(template, fields));

    if stem.chars().count() > MAX_NAME {
        // Cut on a character boundary, then tidy again: the cut can land
        // mid-word and leave the same trailing punctuation an empty field does.
        stem = stem.chars().take(MAX_NAME).collect();
        stem = tidy(&stem);
    }
    if stem.is_empty() {
        return String::new();
    }

    if extension.is_empty() {
        stem
    } else {
        format!("{stem}.{extension}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&'static str, &str)]) -> Fields<'static> {
        pairs.iter().map(|(k, v)| (*k, (*v).to_string())).collect()
    }

    #[test]
    fn the_usual_template_produces_the_usual_name() {
        let f = fields(&[("artist", "Skee Mask"), ("title", "Rev8617")]);
        assert_eq!(
            build("%artist% - %title%", &f, "flac"),
            "Skee Mask - Rev8617.flac"
        );
    }

    /// The case that decides whether this is usable on a real folder: most
    /// libraries have files missing one field or another, and a dangling dash on
    /// every one of them is worse than not renaming at all.
    #[test]
    fn an_empty_field_takes_its_separator_with_it() {
        let f = fields(&[("artist", ""), ("title", "Rev8617")]);
        assert_eq!(build("%artist% - %title%", &f, "flac"), "Rev8617.flac");

        let f = fields(&[("artist", "Skee Mask"), ("title", "")]);
        assert_eq!(build("%artist% - %title%", &f, "flac"), "Skee Mask.flac");
    }

    #[test]
    fn empty_brackets_do_not_survive() {
        let f = fields(&[("title", "Rev8617"), ("label", "")]);
        assert_eq!(build("%title% (%label%)", &f, "wav"), "Rev8617.wav");
    }

    /// A typo should be visible, not silently destructive. Emptying an unknown
    /// field would quietly delete part of every name in the folder.
    #[test]
    fn an_unknown_field_is_left_as_written() {
        let f = fields(&[("title", "Rev8617")]);
        assert_eq!(
            build("%title% %nonsense%", &f, "mp3"),
            "Rev8617 %nonsense%.mp3"
        );
    }

    #[test]
    fn a_lone_percent_is_a_literal_one() {
        let f = fields(&[("title", "100%")]);
        assert_eq!(fill("%title% complete", &f), "100% complete");
    }

    /// Windows refuses nine characters outright. Substituting rather than
    /// deleting keeps `AC/DC` readable.
    #[test]
    fn forbidden_characters_become_something_readable() {
        assert_eq!(sanitise("AC/DC"), "AC-DC");
        assert_eq!(sanitise(r"Front 242\Headhunter"), "Front 242-Headhunter");
        assert_eq!(sanitise("Untitled: Part 2"), "Untitled - Part 2");
        assert_eq!(sanitise(r#"He said "no""#), "He said 'no'");
        assert_eq!(sanitise("What?"), "What");
        assert_eq!(sanitise("A<B>C|D*E"), "A(B)C-DE");
    }

    /// A trailing dot or space is accepted by the API and then silently stripped
    /// by the filesystem, which makes the file impossible to find by name.
    #[test]
    fn trailing_dots_and_spaces_are_removed() {
        assert_eq!(sanitise("Track name. "), "Track name");
        assert_eq!(sanitise("Track name   "), "Track name");
    }

    /// These have been unusable since DOS, and the error when you try is
    /// nothing like "that name is reserved".
    #[test]
    fn reserved_device_names_are_made_usable() {
        assert_eq!(sanitise("CON"), "_CON");
        assert_eq!(sanitise("nul"), "_nul");
        assert_eq!(sanitise("COM1.mp3"), "_COM1.mp3");
        assert_eq!(sanitise("CONCERT"), "CONCERT", "only the exact names");
    }

    /// A path component over 255 characters cannot be created at all.
    #[test]
    fn an_overlong_name_is_cut_to_something_the_filesystem_accepts() {
        let f = fields(&[("title", &"a".repeat(400))]);
        let name = build("%title%", &f, "flac");
        assert!(name.chars().count() <= MAX_NAME + 5, "got {}", name.len());
        assert!(name.ends_with(".flac"));
    }

    /// Changing the extension would produce a file nothing can open, so the
    /// template never touches it.
    #[test]
    fn the_extension_is_carried_over_untouched() {
        let f = fields(&[("title", "Rev8617")]);
        assert_eq!(build("%title%", &f, "flac"), "Rev8617.flac");
        assert_eq!(build("%title%", &f, ""), "Rev8617");
    }

    /// A template that fills to nothing must produce nothing, so the caller can
    /// refuse rather than create a file called `.flac`.
    #[test]
    fn a_template_that_empties_produces_no_name() {
        let f = fields(&[("artist", ""), ("title", "")]);
        assert_eq!(build("%artist% - %title%", &f, "flac"), "");
    }

    #[test]
    fn values_are_trimmed_and_spacing_collapsed() {
        let f = fields(&[("artist", "  Skee Mask  "), ("title", "Rev8617")]);
        assert_eq!(fill("%artist%  -  %title%", &f), "Skee Mask - Rev8617");
    }
}
