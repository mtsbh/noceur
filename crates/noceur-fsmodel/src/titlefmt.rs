//! foobar2000-style title formatting.
//!
//! The rename templates in [`crate::rename`] substitute `%field%` and nothing
//! else, which is enough to build `Artist - Title` and not enough for anything a
//! real library needs — splitting a multi-artist credit, trimming a date to its
//! year, cutting whatever a shop put in brackets.
//!
//! This is the language people already write those in. It is deliberately
//! foobar2000's syntax rather than one of our own: anyone who has used foobar,
//! Mp3tag or OneTagger has expressions already written, and a dialect that is
//! nearly the same would be worse than either a copy or something plainly
//! different.
//!
//! ```text
//! %artist% - %title%
//! $left(%date%,4)
//! $caps2(%artist%)
//! $replace(%artist%,',',';','&',';')
//! $ifgreater($strchr(%title%,'('),0,$left(%title%,$sub($strchr(%title%,'('),1)),%title%)
//! ```
//!
//! Three of Mp3tag's functions are here too, because the expressions people
//! bring from it lean on them: `$regexp(x,pattern,replacement[,ignorecase])`,
//! `$cutLeft(x,n)` and `$cutRight(x,n)`. `$replace` stays literal, exactly as it
//! is in foobar2000; a pattern is only ever a pattern inside `$regexp`.
//!
//! What is not here, and is not planned: variables (`$puts`/`$get`) and the
//! playback and file-info fields that only mean something inside a player.

use std::collections::HashMap;

/// What an expression can refer to.
pub type Fields<'a> = HashMap<&'a str, String>;

/// A parsed expression, ready to run against any track.
///
/// Parsed once and evaluated per file: an action runs over a selection, and
/// re-parsing the same expression for each of two hundred files would be the
/// only slow part of a job that is otherwise string handling.
#[derive(Debug, Clone, PartialEq)]
pub struct Script(Vec<Node>);

#[derive(Debug, Clone, PartialEq)]
enum Node {
    /// Text that stands for itself.
    Literal(String),
    /// `%name%` — a field's value.
    Field(String),
    /// `$name(args)` — a function call.
    Call(String, Vec<Vec<Node>>),
    /// `[ ... ]` — emitted only if something inside it was found.
    Optional(Vec<Node>),
}

/// A value, and whether it came from anywhere.
///
/// The second half is what makes `[ ]` and `$if` work: an empty string that came
/// from a real but blank tag and an empty string that came from a field the file
/// does not have are different claims, and only the second should make a
/// bracketed section disappear.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Value {
    pub text: String,
    pub found: bool,
}

impl Value {
    fn plain(text: impl Into<String>) -> Value {
        Value {
            text: text.into(),
            found: false,
        }
    }

    fn known(text: impl Into<String>) -> Value {
        Value {
            text: text.into(),
            found: true,
        }
    }
}

/// What went wrong with an expression, in terms of what was written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptError {
    #[error("a $ must be followed by a function name")]
    NamelessCall,
    #[error("$ {0} is missing its closing bracket")]
    UnclosedCall(String),
    #[error("a [ is missing its closing ]")]
    UnclosedOptional,
    #[error("no such function: ${0}")]
    UnknownFunction(String),
    #[error("${0} takes {1}, not {2}")]
    WrongArity(String, &'static str, usize),
    #[error("$regexp pattern does not parse: {0}")]
    BadPattern(String),
}

impl Script {
    /// Parses an expression.
    ///
    /// An unknown function is refused here rather than at evaluation, so a typo
    /// in a saved action is reported when it is saved rather than silently
    /// producing the wrong name for four hundred files.
    pub fn parse(source: &str) -> Result<Script, ScriptError> {
        let mut chars = source.chars().peekable();
        let nodes = parse_until(&mut chars, None)?;
        let script = Script(nodes);
        script.check()?;
        Ok(script)
    }

    /// Runs the expression against one track's fields.
    pub fn run(&self, fields: &Fields) -> String {
        eval(&self.0, fields).text
    }

    /// Every field name the expression mentions.
    ///
    /// So a caller can read only the tags an expression actually uses rather
    /// than every tag of every file in a selection.
    pub fn fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_fields(&self.0, &mut out);
        out.sort();
        out.dedup();
        out
    }

    fn check(&self) -> Result<(), ScriptError> {
        check(&self.0)
    }
}

fn collect_fields(nodes: &[Node], out: &mut Vec<String>) {
    for node in nodes {
        match node {
            Node::Field(name) => out.push(name.clone()),
            Node::Call(_, args) => {
                for arg in args {
                    collect_fields(arg, out);
                }
            }
            Node::Optional(inner) => collect_fields(inner, out),
            Node::Literal(_) => {}
        }
    }
}

fn check(nodes: &[Node]) -> Result<(), ScriptError> {
    for node in nodes {
        match node {
            Node::Call(name, args) => {
                let arity =
                    arity_of(name).ok_or_else(|| ScriptError::UnknownFunction(name.clone()))?;
                if !arity.accepts(args.len()) {
                    return Err(ScriptError::WrongArity(
                        name.clone(),
                        arity.describe(),
                        args.len(),
                    ));
                }
                // A pattern written out in full is compiled now, so a typo in it
                // is reported when the action is saved. One built from fields
                // can only be checked when it runs.
                if name == "regexp" {
                    if let [Node::Literal(pattern)] = args[1].as_slice() {
                        pattern_of(pattern, false)
                            .map_err(|e| ScriptError::BadPattern(e.to_string()))?;
                    }
                }
                for arg in args {
                    check(arg)?;
                }
            }
            Node::Optional(inner) => check(inner)?,
            _ => {}
        }
    }
    Ok(())
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// Reads nodes until one of `stop` is seen at this nesting level.
///
/// The stop character is consumed by the caller, not here, so a function's
/// argument parser can tell `,` from `)`.
fn parse_until(chars: &mut Chars, stop: Option<&[char]>) -> Result<Vec<Node>, ScriptError> {
    let mut nodes = Vec::new();
    let mut text = String::new();

    /// Pushes accumulated literal text, if any.
    fn flush(text: &mut String, nodes: &mut Vec<Node>) {
        if !text.is_empty() {
            nodes.push(Node::Literal(std::mem::take(text)));
        }
    }

    while let Some(&c) = chars.peek() {
        if let Some(stop) = stop {
            if stop.contains(&c) {
                break;
            }
        }
        chars.next();

        match c {
            // Single quotes make their contents literal, which is how a `%` or a
            // bracket gets into the output. `''` is one quote.
            '\'' => {
                let mut quoted = String::new();
                let mut closed = false;
                while let Some(q) = chars.next() {
                    if q == '\'' {
                        if chars.peek() == Some(&'\'') {
                            chars.next();
                            quoted.push('\'');
                            continue;
                        }
                        closed = true;
                        break;
                    }
                    quoted.push(q);
                }
                // An unterminated quote runs to the end rather than failing:
                // it is a half-typed expression, and the preview shows it.
                let _ = closed;
                text.push_str(&quoted);
            }

            '%' => {
                let mut name = String::new();
                for n in chars.by_ref() {
                    if n == '%' {
                        break;
                    }
                    name.push(n);
                }
                flush(&mut text, &mut nodes);
                nodes.push(Node::Field(name.trim().to_lowercase()));
            }

            '$' => {
                let mut name = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphanumeric() || n == '_' {
                        name.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if name.is_empty() {
                    return Err(ScriptError::NamelessCall);
                }
                flush(&mut text, &mut nodes);

                let mut args = Vec::new();
                if chars.peek() == Some(&'(') {
                    chars.next();
                    loop {
                        let arg = parse_until(chars, Some(&[',', ')']))?;
                        args.push(arg);
                        match chars.next() {
                            Some(',') => continue,
                            Some(')') => break,
                            _ => return Err(ScriptError::UnclosedCall(name)),
                        }
                    }
                }
                nodes.push(Node::Call(name.to_lowercase(), args));
            }

            // Inside a function's arguments a bracket that opens is text, and so
            // is the one that closes it: `$regexp(%title%,^(A|The)\s,)` is how
            // Mp3tag expressions are written, unquoted. Without counting them the
            // first `)` would end the call halfway through the pattern.
            '(' if stop.is_some_and(|s| s.contains(&')')) => {
                text.push('(');
                for node in parse_until(chars, Some(&[')']))? {
                    match node {
                        Node::Literal(inner) => text.push_str(&inner),
                        other => {
                            flush(&mut text, &mut nodes);
                            nodes.push(other);
                        }
                    }
                }
                if chars.next() != Some(')') {
                    return Err(ScriptError::UnclosedCall("(".into()));
                }
                text.push(')');
            }

            '[' => {
                let inner = parse_until(chars, Some(&[']']))?;
                if chars.next() != Some(']') {
                    return Err(ScriptError::UnclosedOptional);
                }
                flush(&mut text, &mut nodes);
                nodes.push(Node::Optional(inner));
            }

            _ => text.push(c),
        }
    }

    flush(&mut text, &mut nodes);
    Ok(nodes)
}

fn eval(nodes: &[Node], fields: &Fields) -> Value {
    let mut text = String::new();
    let mut found = false;

    for node in nodes {
        let value = eval_one(node, fields);
        found |= value.found;
        text.push_str(&value.text);
    }
    Value { text, found }
}

fn eval_one(node: &Node, fields: &Fields) -> Value {
    match node {
        Node::Literal(text) => Value::plain(text.clone()),
        Node::Field(name) => match fields.get(name.as_str()) {
            Some(v) if !v.is_empty() => Value::known(v.clone()),
            // Present but empty, and absent entirely, both read as not found:
            // for the purpose of `[ ]` a blank tag has nothing to contribute.
            _ => Value::default(),
        },
        Node::Optional(inner) => {
            let value = eval(inner, fields);
            if value.found {
                value
            } else {
                Value::default()
            }
        }
        Node::Call(name, args) => call(name, args, fields),
    }
}

/// How many arguments a function takes.
#[derive(Debug, Clone, Copy)]
enum Arity {
    Exactly(usize),
    Between(usize, usize),
    AtLeast(usize),
    /// A count that must be odd and at least this — `$replace` and its pairs.
    OddFrom(usize),
}

impl Arity {
    fn accepts(self, n: usize) -> bool {
        match self {
            Arity::Exactly(k) => n == k,
            Arity::Between(a, b) => n >= a && n <= b,
            Arity::AtLeast(k) => n >= k,
            Arity::OddFrom(k) => n >= k && n % 2 == 1,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Arity::Exactly(1) => "one argument",
            Arity::Exactly(2) => "two arguments",
            Arity::Exactly(3) => "three arguments",
            Arity::Exactly(4) => "four arguments",
            Arity::Between(2, 3) => "two or three arguments",
            Arity::Between(3, 4) => "three or four arguments",
            Arity::AtLeast(1) => "at least one argument",
            Arity::AtLeast(2) => "at least two arguments",
            Arity::OddFrom(3) => "a value and then pairs of arguments",
            _ => "a different number of arguments",
        }
    }
}

fn arity_of(name: &str) -> Option<Arity> {
    Some(match name {
        "upper" | "lower" | "caps" | "caps2" | "trim" | "len" | "num" | "abs" | "not" => {
            Arity::Exactly(1)
        }
        "left" | "right" | "strchr" | "strrchr" | "repeat" | "cutleft" | "cutright" => {
            Arity::Exactly(2)
        }
        "regexp" => Arity::Between(3, 4),
        "substr" | "insert" => Arity::Exactly(3),
        "replace" => Arity::OddFrom(3),
        "if" => Arity::Between(2, 3),
        "if2" | "if3" => Arity::AtLeast(2),
        "ifequal" | "ifgreater" | "iflonger" => Arity::Exactly(4),
        "add" | "sub" | "mul" | "div" | "min" | "max" | "and" | "or" => Arity::AtLeast(1),
        "meta" | "strstr" | "eql" | "neql" => Arity::Exactly(2),
        "pad" | "padcut" => Arity::Exactly(2),
        _ => return None,
    })
}

fn call(name: &str, args: &[Vec<Node>], fields: &Fields) -> Value {
    // Most functions want every argument evaluated. The conditionals do not —
    // evaluating the branch not taken is how `$if` on a missing field ends up
    // reporting that the field was found.
    let lazy = matches!(
        name,
        "if" | "if2" | "if3" | "ifequal" | "ifgreater" | "iflonger" | "and" | "or"
    );
    let done: Vec<Value> = if lazy {
        Vec::new()
    } else {
        args.iter().map(|a| eval(a, fields)).collect()
    };
    let text = |i: usize| done.get(i).map(|v| v.text.clone()).unwrap_or_default();
    let number = |i: usize| parse_number(&text(i));

    match name {
        "upper" => Value::known(text(0).to_uppercase()),
        "lower" => Value::known(text(0).to_lowercase()),
        "caps" => Value::known(caps(&text(0), true)),
        "caps2" => Value::known(caps(&text(0), false)),
        "trim" => Value::known(text(0).trim().to_string()),
        "len" => Value::known(text(0).chars().count().to_string()),
        "num" => Value::known(number(0).to_string()),
        "abs" => Value::known(number(0).abs().to_string()),

        "left" => Value::known(slice(&text(0), 0, number(1))),
        "right" => {
            let s = text(0);
            let total = s.chars().count() as i64;
            let n = number(1).clamp(0, total);
            Value::known(slice(&s, total - n, n))
        }
        // 1-based and inclusive of both ends, which is foobar2000's rule rather
        // than the one a programmer would pick. It has to be: the whole reason
        // for this syntax is that expressions people already have keep working,
        // and an off-by-one here would silently shorten every name.
        "substr" => {
            let s = text(0);
            let total = s.chars().count() as i64;
            let from = number(1).max(1) - 1;
            let to = number(2).min(total);
            Value::known(slice(&s, from, to - from))
        }

        // 1-based, and 0 for "not there" — the foobar2000 convention, which is
        // what makes `$ifgreater($strchr(...),0,...)` the idiom it is.
        "strchr" => Value::known(index_of(&text(0), &text(1), false).to_string()),
        "strrchr" => Value::known(index_of(&text(0), &text(1), true).to_string()),
        "strstr" => Value::known(index_of(&text(0), &text(1), false).to_string()),

        // Literal, and in pairs, exactly as foobar2000 does it. No patterns.
        "replace" => {
            let mut s = text(0);
            let mut i = 1;
            while i + 1 < done.len() {
                let from = text(i);
                if !from.is_empty() {
                    s = s.replace(&from, &text(i + 1));
                }
                i += 2;
            }
            Value::known(s)
        }
        "insert" => {
            let s = text(0);
            let at = number(2).max(0);
            let head = slice(&s, 0, at);
            let tail_len = s.chars().count() as i64 - at;
            Value::known(format!(
                "{head}{}{}",
                text(1),
                slice(&s, at, tail_len.max(0))
            ))
        }
        // Mp3tag's: everything but the first or last n characters.
        "cutleft" => {
            let s = text(0);
            let total = s.chars().count() as i64;
            let n = number(1).clamp(0, total);
            Value::known(slice(&s, n, total - n))
        }
        "cutright" => {
            let s = text(0);
            let total = s.chars().count() as i64;
            let n = number(1).clamp(0, total);
            Value::known(slice(&s, 0, total - n))
        }

        // Mp3tag's, with `$1` for a captured group. A pattern that will not
        // compile leaves the value as it was: a field that only sometimes builds
        // a bad pattern must not empty a tag.
        "regexp" => {
            let s = text(0);
            let ignore_case = number(3) != 0;
            match pattern_of(&text(1), ignore_case) {
                Ok(re) => Value::known(re.replace_all(&s, text(2).as_str()).into_owned()),
                Err(_) => Value::known(s),
            }
        }

        "repeat" => Value::known(text(0).repeat(number(1).clamp(0, 4096) as usize)),
        "pad" => {
            let s = text(0);
            let width = number(1).max(0) as usize;
            let short = width.saturating_sub(s.chars().count());
            Value::known(format!("{s}{}", " ".repeat(short)))
        }
        "padcut" => {
            let s = text(0);
            let width = number(1).max(0) as usize;
            let cut: String = s.chars().take(width).collect();
            let short = width.saturating_sub(cut.chars().count());
            Value::known(format!("{cut}{}", " ".repeat(short)))
        }

        "add" | "sub" | "mul" | "div" | "min" | "max" => {
            let mut numbers = (0..done.len()).map(number);
            let first = numbers.next().unwrap_or(0);
            let total = numbers.fold(first, |acc, n| match name {
                "add" => acc + n,
                "sub" => acc - n,
                "mul" => acc * n,
                "div" => {
                    if n == 0 {
                        acc
                    } else {
                        acc / n
                    }
                }
                "min" => acc.min(n),
                _ => acc.max(n),
            });
            Value::known(total.to_string())
        }

        // Mp3tag's text comparisons, as conditions for `$if`.
        "eql" | "neql" => Value {
            text: String::new(),
            found: (text(0) == text(1)) == (name == "eql"),
        },

        "not" => {
            let v = eval(&args[0], fields);
            Value {
                text: String::new(),
                found: !v.found,
            }
        }
        "and" => {
            let all = args.iter().all(|a| eval(a, fields).found);
            Value {
                text: String::new(),
                found: all,
            }
        }
        "or" => {
            let any = args.iter().any(|a| eval(a, fields).found);
            Value {
                text: String::new(),
                found: any,
            }
        }

        "if" => {
            if eval(&args[0], fields).found {
                eval(&args[1], fields)
            } else {
                args.get(2).map(|a| eval(a, fields)).unwrap_or_default()
            }
        }
        // The first argument that has anything in it.
        "if2" | "if3" => {
            for arg in &args[..args.len() - 1] {
                let v = eval(arg, fields);
                if v.found {
                    return v;
                }
            }
            eval(&args[args.len() - 1], fields)
        }
        "ifequal" | "ifgreater" | "iflonger" => {
            let a = eval(&args[0], fields);
            let b = eval(&args[1], fields);
            let yes = match name {
                "ifequal" => parse_number(&a.text) == parse_number(&b.text),
                "ifgreater" => parse_number(&a.text) > parse_number(&b.text),
                _ => a.text.chars().count() > b.text.chars().count(),
            };
            eval(&args[if yes { 2 } else { 3 }], fields)
        }

        "meta" => match fields.get(text(0).to_lowercase().as_str()) {
            Some(v) if !v.is_empty() => Value::known(v.clone()),
            _ => Value::default(),
        },

        // Unreachable: `check` refuses unknown names at parse time.
        _ => Value::default(),
    }
}

/// Compiles a `$regexp` pattern, bounded so a pathological one cannot eat memory.
fn pattern_of(pattern: &str, ignore_case: bool) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(&perl_classes(pattern))
        .case_insensitive(ignore_case)
        .size_limit(1 << 20)
        .build()
}

/// Escapes a `[` inside a character class.
///
/// Mp3tag's engine reads `[[{(]` as "one of `[`, `{`, `(`"; this one reads the
/// inner `[` as the start of a nested class and refuses the pattern. `[:alpha:]`
/// is left alone, since both engines agree on what that means.
fn perl_classes(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '[' if in_class && chars.peek() != Some(&':') => out.push_str(r"\["),
            '[' => {
                in_class = true;
                out.push(c);
                // A `]` straight after the opening one, or after `^`, is a
                // member rather than the end.
                if chars.peek() == Some(&'^') {
                    out.push(chars.next().unwrap_or('^'));
                }
                if chars.peek() == Some(&']') {
                    chars.next();
                    out.push_str(r"\]");
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Characters `n` from `from`, both counted in characters rather than bytes.
fn slice(text: &str, from: i64, len: i64) -> String {
    if len <= 0 {
        return String::new();
    }
    text.chars()
        .skip(from.max(0) as usize)
        .take(len as usize)
        .collect()
}

/// 1-based position of `needle`, or 0 when it is not there.
fn index_of(haystack: &str, needle: &str, last: bool) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let at = if last {
        haystack.rfind(needle)
    } else {
        haystack.find(needle)
    };
    // Converted to a character position: a byte offset would be wrong the moment
    // a title has an accent in it, and these numbers are handed straight to
    // `$left` and `$substr`.
    at.map(|byte| haystack[..byte].chars().count() + 1)
        .unwrap_or(0)
}

/// Reads a leading integer, as foobar2000 does: anything unparseable is zero.
fn parse_number(text: &str) -> i64 {
    let trimmed = text.trim();
    let mut end = 0;
    for (i, c) in trimmed.char_indices() {
        if c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')) {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    trimmed[..end].parse().unwrap_or(0)
}

/// Capitalises words. `all_lower` lowercases the rest of each word, as `$caps`
/// does; `$caps2` leaves it, which is what keeps `DJ` and `EBM` intact.
fn caps(text: &str, all_lower: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut starting = true;

    for c in text.chars() {
        if starting && c.is_alphanumeric() {
            out.extend(c.to_uppercase());
            starting = false;
        } else if c.is_alphanumeric() {
            if all_lower {
                out.extend(c.to_lowercase());
            } else {
                out.push(c);
            }
        } else {
            out.push(c);
            starting = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&'static str, &str)]) -> Fields<'static> {
        pairs.iter().map(|(k, v)| (*k, (*v).to_string())).collect()
    }

    fn run(source: &str, f: &Fields) -> String {
        Script::parse(source).expect("parses").run(f)
    }

    #[test]
    fn a_field_is_substituted() {
        let f = fields(&[("artist", "Skee Mask"), ("title", "Rev8617")]);
        assert_eq!(run("%artist% - %title%", &f), "Skee Mask - Rev8617");
    }

    #[test]
    fn field_names_are_case_insensitive() {
        let f = fields(&[("album artist", "Ilian Tape")]);
        assert_eq!(run("%ALBUM ARTIST%", &f), "Ilian Tape");
    }

    #[test]
    fn a_missing_field_produces_nothing() {
        let f = fields(&[("title", "Rev8617")]);
        assert_eq!(run("%artist%%title%", &f), "Rev8617");
    }

    /// The whole point of the bracket form: punctuation that belongs to a field
    /// disappears with it.
    #[test]
    fn a_bracketed_section_needs_something_in_it() {
        let f = fields(&[("title", "Rev8617"), ("label", "Ilian Tape")]);
        assert_eq!(run("%title%[ '['%label%']']", &f), "Rev8617 [Ilian Tape]");

        let f = fields(&[("title", "Rev8617")]);
        assert_eq!(run("%title%[ '['%label%']']", &f), "Rev8617");
    }

    #[test]
    fn quotes_make_their_contents_literal() {
        let f = fields(&[("title", "Rev8617")]);
        assert_eq!(run("'%title%'", &f), "%title%");
        assert_eq!(run("%title%' 100%'", &f), "Rev8617 100%");
        assert_eq!(run("'it''s'", &f), "it's");
    }

    // ── The expressions this was built for ────────────────────────────────

    #[test]
    fn separators_between_artists_can_be_unified() {
        let f = fields(&[("artist", "Aphex Twin, Squarepusher & Autechre and µ-Ziq")]);
        assert_eq!(
            run("$replace(%artist%,', ',';','&',';',' and ',';')", &f),
            "Aphex Twin;Squarepusher ; Autechre;µ-Ziq"
        );
    }

    #[test]
    fn a_date_can_be_cut_to_its_year() {
        let f = fields(&[("date", "2016-04-22")]);
        assert_eq!(run("$left(%date%,4)", &f), "2016");
    }

    #[test]
    fn artist_case_can_be_fixed_without_flattening_acronyms() {
        let f = fields(&[("artist", "skee mask")]);
        assert_eq!(run("$caps2(%artist%)", &f), "Skee Mask");

        // $caps2 leaves the rest of a word alone, so initialisms survive.
        let f = fields(&[("artist", "DJ Stingray")]);
        assert_eq!(run("$caps2(%artist%)", &f), "DJ Stingray");
        // $caps lowercases the rest, which is the difference between them.
        assert_eq!(run("$caps(%artist%)", &f), "Dj Stingray");
    }

    #[test]
    fn the_artist_can_be_merged_into_the_title() {
        let f = fields(&[("artist", "Skee Mask"), ("title", "Rev8617")]);
        assert_eq!(run("%artist% - %title%", &f), "Skee Mask - Rev8617");
    }

    /// The bracket-stripping idiom, verbatim as a foobar2000 user would write it.
    #[test]
    fn whatever_is_in_brackets_can_be_cut() {
        let expr = "$ifgreater($strchr(%title%,'('),0,\
                    $trim($left(%title%,$sub($strchr(%title%,'('),1))),%title%)";

        let f = fields(&[("title", "Rev8617 (Original Mix)")]);
        assert_eq!(run(expr, &f), "Rev8617");

        // A title with no bracket is returned untouched rather than emptied.
        let f = fields(&[("title", "Rev8617")]);
        assert_eq!(run(expr, &f), "Rev8617");
    }

    /// The Telegram argument, which rebuilds a path from its own parts:
    /// everything up to and including the last separator, then the file name.
    /// The trailing separator is the point rather than a bug in the cut.
    #[test]
    fn a_path_can_be_cut_at_its_last_separator() {
        let f = fields(&[
            ("_path", "E:\\music\\downloads\\track.flac"),
            ("filename_ext", "track.flac"),
        ]);
        assert_eq!(
            run("$substr(%_path%,0,$strrchr(%_path%,'\\'))", &f),
            "E:\\music\\downloads\\"
        );
        assert_eq!(
            run(
                "$substr(%_path%,0,$strrchr(%_path%,'\\'))%filename_ext%",
                &f
            ),
            "E:\\music\\downloads\\track.flac"
        );
    }

    // ── The pieces those are built from ───────────────────────────────────

    #[test]
    fn strchr_is_one_based_and_zero_when_absent() {
        let f = fields(&[("title", "Rev (Mix)")]);
        assert_eq!(run("$strchr(%title%,'(')", &f), "5");
        assert_eq!(run("$strchr(%title%,'|')", &f), "0");
    }

    /// A byte offset would be wrong the moment a title has an accent in it, and
    /// these numbers go straight into `$left`.
    #[test]
    fn positions_are_counted_in_characters_not_bytes() {
        let f = fields(&[("title", "Café (Mix)")]);
        assert_eq!(run("$strchr(%title%,'(')", &f), "6");
        assert_eq!(run("$left(%title%,4)", &f), "Café");
    }

    #[test]
    fn strrchr_finds_the_last_one() {
        let f = fields(&[("x", "a/b/c")]);
        assert_eq!(run("$strrchr(%x%,'/')", &f), "4");
    }

    #[test]
    fn arithmetic_is_integer_and_left_to_right() {
        let f = fields(&[]);
        assert_eq!(run("$add(2,3,4)", &f), "9");
        assert_eq!(run("$sub(10,3)", &f), "7");
        assert_eq!(run("$mul(3,4)", &f), "12");
        assert_eq!(run("$div(7,2)", &f), "3");
        assert_eq!(run("$max(1,9,4)", &f), "9");
        assert_eq!(run("$min(1,9,4)", &f), "1");
    }

    /// Dividing by zero must not take the whole rename down.
    #[test]
    fn dividing_by_zero_leaves_the_value_alone() {
        assert_eq!(run("$div(7,0)", &fields(&[])), "7");
    }

    #[test]
    fn text_that_is_not_a_number_counts_as_zero() {
        let f = fields(&[("bpm", "about 128")]);
        assert_eq!(run("$add(%bpm%,1)", &f), "1");
        let f = fields(&[("bpm", "128 bpm")]);
        assert_eq!(run("$add(%bpm%,1)", &f), "129");
    }

    #[test]
    fn substr_and_left_and_right_agree_about_where_they_cut() {
        let f = fields(&[("x", "abcdef")]);
        assert_eq!(run("$left(%x%,3)", &f), "abc");
        assert_eq!(run("$right(%x%,2)", &f), "ef");
        // 1-based and inclusive of both ends, as foobar2000 has it.
        assert_eq!(run("$substr(%x%,2,4)", &f), "bcd");
        assert_eq!(run("$substr(%x%,1,3)", &f), "abc");
        // A start below 1 means the beginning, which is what makes the
        // widespread `$substr(%path%,0,...)` idiom work.
        assert_eq!(run("$substr(%x%,0,3)", &f), "abc");
    }

    #[test]
    fn cutting_past_the_end_is_not_an_error() {
        let f = fields(&[("x", "ab")]);
        assert_eq!(run("$left(%x%,99)", &f), "ab");
        assert_eq!(run("$right(%x%,99)", &f), "ab");
        assert_eq!(run("$left(%x%,-1)", &f), "");
        assert_eq!(run("$substr(%x%,5,9)", &f), "");
    }

    #[test]
    fn if_branches_on_whether_a_field_was_found() {
        let f = fields(&[("artist", "Skee Mask")]);
        assert_eq!(run("$if(%artist%,yes,no)", &f), "yes");
        assert_eq!(run("$if(%label%,yes,no)", &f), "no");
        assert_eq!(run("$if(%label%,yes)", &f), "");
    }

    #[test]
    fn if2_takes_the_first_field_that_has_anything() {
        let f = fields(&[("title", "Rev8617")]);
        assert_eq!(run("$if2(%album%,%title%)", &f), "Rev8617");
    }

    #[test]
    fn replace_is_literal_rather_than_a_pattern() {
        let f = fields(&[("x", "a.b")]);
        // A regular expression would match everything; this must match nothing.
        assert_eq!(run("$replace(%x%,'.','-')", &f), "a-b");
        assert_eq!(run("$replace(%x%,'(,|&)',';')", &f), "a.b");
    }

    // ── Mp3tag's functions ────────────────────────────────────────────────

    #[test]
    fn cut_left_and_right_drop_characters_from_either_end() {
        let f = fields(&[("_filename", "01 Skee Mask - Rev8617 xyz")]);
        assert_eq!(
            run("$cutLeft(%_filename%,3)", &f),
            "Skee Mask - Rev8617 xyz"
        );
        assert_eq!(
            run("$cutRight(%_filename%,4)", &f),
            "01 Skee Mask - Rev8617"
        );
        assert_eq!(run("$cutLeft(%_filename%,99)", &f), "");
        assert_eq!(
            run("$cutRight(%_filename%,-2)", &f),
            "01 Skee Mask - Rev8617 xyz"
        );
    }

    #[test]
    fn regexp_replaces_with_captured_groups() {
        let f = fields(&[("title", "Rev8617 (Original Mix)")]);
        assert_eq!(run(r"$regexp(%title%,'\s*\(.*\)$',)", &f), "Rev8617");
        let f = fields(&[("_filename", "01 - Skee Mask - Rev8617")]);
        assert_eq!(
            run(r"$regexp(%_filename%,'^\s*\d+\s*-\s*',)", &f),
            "Skee Mask - Rev8617"
        );
        let f = fields(&[("artist", "Mask, Skee")]);
        assert_eq!(
            run("$regexp(%artist%,'(.+), (.+)','$2 $1')", &f),
            "Skee Mask"
        );
    }

    #[test]
    fn regexp_can_ignore_case() {
        let f = fields(&[("title", "Rev8617 (ORIGINAL MIX)")]);
        assert_eq!(
            run(r"$regexp(%title%,' \(original mix\)',,1)", &f),
            "Rev8617"
        );
        assert_eq!(
            run(r"$regexp(%title%,' \(original mix\)',)", &f),
            "Rev8617 (ORIGINAL MIX)"
        );
    }

    /// Mp3tag expressions leave brackets in a pattern unquoted. Counted, they
    /// stay part of the argument instead of ending the call.
    #[test]
    fn an_mp3tag_expression_runs_as_written() {
        let expr = r"$if($eql($regexp(%title%,^(A|An|The|El|Los|La|Las|Le|Les|De)\s,),%title%),,$regexp(%title%,'^(.+?)\s(.+?)(\s[[{(].)$','$2, $1$3'))";
        assert_eq!(Script::parse(expr).err(), None);
        // An article, so the inner replace changes it and the else branch runs.
        let f = fields(&[("title", "The Beatles (Remastered)")]);
        assert_eq!(run(expr, &f), "The Beatles (Remastered)");
        // No article: unchanged, equal, and the empty first branch is taken.
        let f = fields(&[("title", "Rev8617")]);
        assert_eq!(run(expr, &f), "");
    }

    #[test]
    fn eql_compares_text() {
        let f = fields(&[("a", "x"), ("b", "x"), ("c", "y")]);
        assert_eq!(run("$if($eql(%a%,%b%),same,diff)", &f), "same");
        assert_eq!(run("$if($neql(%a%,%c%),diff,same)", &f), "diff");
    }

    #[test]
    fn a_broken_pattern_is_refused_when_saved() {
        assert!(matches!(
            Script::parse("$regexp(%title%,'(unclosed',)"),
            Err(ScriptError::BadPattern(_))
        ));
    }

    // ── What is refused, and when ─────────────────────────────────────────

    /// Caught when the action is saved rather than after it has renamed a folder.
    #[test]
    fn an_unknown_function_is_refused_at_parse_time() {
        assert_eq!(
            Script::parse("$regex(%title%,'a','b')"),
            Err(ScriptError::UnknownFunction("regex".into()))
        );
    }

    #[test]
    fn the_wrong_number_of_arguments_is_refused() {
        assert!(matches!(
            Script::parse("$left(%title%)"),
            Err(ScriptError::WrongArity(..))
        ));
        // $replace takes a value and then pairs, so an even count is wrong.
        assert!(matches!(
            Script::parse("$replace(%x%,'a')"),
            Err(ScriptError::WrongArity(..))
        ));
    }

    #[test]
    fn an_unclosed_call_is_refused() {
        assert!(Script::parse("$left(%title%,4").is_err());
    }

    #[test]
    fn a_lone_dollar_is_refused() {
        assert_eq!(Script::parse("100$"), Err(ScriptError::NamelessCall));
    }

    #[test]
    fn nested_calls_are_reported_too() {
        assert_eq!(
            Script::parse("$left(%t%,$nope(1))"),
            Err(ScriptError::UnknownFunction("nope".into()))
        );
    }

    /// So a caller can read only the tags an expression uses rather than all of
    /// them for every file in a selection.
    #[test]
    fn an_expression_can_say_which_fields_it_needs() {
        let s = Script::parse("$if(%artist%,%artist% - %title%,%title%)").unwrap();
        assert_eq!(s.fields(), vec!["artist", "title"]);
    }

    #[test]
    fn an_empty_expression_produces_nothing() {
        assert_eq!(run("", &fields(&[])), "");
    }
}
