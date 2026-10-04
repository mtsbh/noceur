//! Sending playlists to the `rekordbox.xml` that rekordbox's Imported Library
//! reads (and Mixed In Key writes its cues into).
//!
//! The file holds every cue point the user has, so it is edited, never
//! rewritten: the new file is the old one with text spliced in at four places —
//! new tracks before `</COLLECTION>`, the collection's `Entries`, a `noceur`
//! folder under the playlists' ROOT, and ROOT's `Count`. Every other byte is
//! the old file's. The `noceur` folder is noceur's own and mirrors its
//! playlists on each send; nothing outside it is touched. [`check`] proves all
//! of that against the old file before anything is written.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

/// The folder under ROOT that holds noceur's playlists.
pub const FOLDER: &str = "noceur";

/// One playlist to send: its name, and its tracks' paths in order.
#[derive(Debug, Clone)]
pub struct Playlist {
    pub name: String,
    pub paths: Vec<String>,
}

/// What rekordbox is told about a track the file does not have yet.
#[derive(Debug, Clone, Default)]
pub struct NewTrack {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub label: String,
    pub comments: String,
    /// `AIFF File`, `MP3 File`…
    pub kind: String,
    pub size: u64,
    /// Seconds.
    pub total_time: u64,
    pub track_number: u32,
    pub year: u32,
    pub bpm: Option<f64>,
    /// kbps.
    pub bit_rate: u32,
    pub sample_rate: u32,
    /// Stars, 0..=5.
    pub stars: u8,
    /// Camelot, as Mixed In Key writes it: `8A`.
    pub tonality: String,
}

/// The merged file and what went into it.
#[derive(Debug)]
pub struct Merged {
    pub xml: String,
    /// Tracks added to the collection.
    pub added: usize,
    /// Playlists in the `noceur` folder.
    pub playlists: usize,
    /// Paths left out because `describe` could not describe them.
    pub missing: Vec<String>,
    /// What [`check`] holds the new file to.
    expect: Expect,
}

#[derive(Debug, Clone)]
struct Expect {
    tracks: usize,
    folder: Vec<(String, Vec<u64>)>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rekordbox.xml cannot be read: {0}")]
    Xml(String),
    #[error("rekordbox.xml has no {0}")]
    Missing(&'static str),
    #[error("the new rekordbox.xml failed its check: {0}")]
    Check(String),
}

/// What a file starts as when there is none yet.
pub const EMPTY: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<DJ_PLAYLISTS Version=\"1.0.0\">\n\
<PRODUCT Name=\"noceur\" Version=\"\" Company=\"\" />\n\
<COLLECTION Entries=\"0\">\n\
</COLLECTION>\n\
<PLAYLISTS>\n\
<NODE Type=\"0\" Name=\"ROOT\" Count=\"0\">\n\
</NODE>\n\
</PLAYLISTS>\n\
</DJ_PLAYLISTS>\n";

/// Merges `lists` into `xml` as the `noceur` folder, adding to the collection
/// every track the file does not already have (matched by path, in any case).
///
/// `describe` is asked about each such track; `None` leaves it out of the
/// playlists and lists it in [`Merged::missing`]. `today` is `YYYY-MM-DD`, the
/// new tracks' DateAdded.
pub fn merge(
    xml: &str,
    lists: &[Playlist],
    mut describe: impl FnMut(&Path) -> Option<NewTrack>,
    today: &str,
) -> Result<Merged, Error> {
    let scan = scan(xml)?;
    let nl = if xml.contains("\r\n") { "\r\n" } else { "\n" };

    let mut ids: HashMap<String, u64> = scan
        .tracks
        .iter()
        .map(|t| (key(&location_path(&t.location)), t.id))
        .collect();
    let mut next = scan.tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let mut new_tracks = String::new();
    let mut added = 0;
    let mut missing = Vec::new();
    let mut folder = Vec::new();
    for list in lists {
        let mut keys = Vec::new();
        for path in &list.paths {
            let k = key(path);
            let id = match ids.get(&k) {
                Some(&id) => id,
                None => match describe(Path::new(path)) {
                    Some(t) => {
                        let id = next;
                        next += 1;
                        added += 1;
                        new_tracks.push_str(&track_element(id, path, &t, today));
                        new_tracks.push_str(nl);
                        ids.insert(k, id);
                        id
                    }
                    None => {
                        if !missing.contains(path) {
                            missing.push(path.clone());
                        }
                        continue;
                    }
                },
            };
            if !keys.contains(&id) {
                keys.push(id);
            }
        }
        folder.push((list.name.clone(), keys));
    }

    let tracks = scan.tracks.len() + added;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();

    // The collection: its count, and the new tracks at its end.
    match scan.collection_close {
        Some(close) => {
            edits.push((
                scan.collection_open.clone(),
                set_attr(&xml[scan.collection_open.clone()], "Entries", tracks),
            ));
            edits.push((close..close, new_tracks));
        }
        None => edits.push((
            scan.collection_open.clone(),
            format!("<COLLECTION Entries=\"{tracks}\">{nl}{new_tracks}</COLLECTION>"),
        )),
    }

    // The playlists: the folder replaced or added, and ROOT's count.
    let folder_xml = folder_element(&folder, nl);
    let theirs = scan.root_children.iter().filter(|c| !c.ours).count();
    let count = theirs + 1;
    match &scan.root {
        Some(root) => match root.close {
            Some(close) => {
                match scan.root_children.iter().find(|c| c.ours) {
                    Some(ours) => edits.push((ours.span.clone(), folder_xml)),
                    None => edits.push((close..close, format!("{folder_xml}{nl}"))),
                }
                edits.push((
                    root.open.clone(),
                    set_attr(&xml[root.open.clone()], "Count", count),
                ));
            }
            // `<NODE Name="ROOT" … />`: opened up around the folder.
            None => edits.push((
                root.open.clone(),
                format!(
                    "<NODE Type=\"0\" Name=\"ROOT\" Count=\"{count}\">{nl}{folder_xml}{nl}</NODE>"
                ),
            )),
        },
        None => {
            let at = scan
                .document_close
                .ok_or(Error::Missing("</DJ_PLAYLISTS>"))?;
            if scan.playlists {
                return Err(Error::Missing("ROOT playlist folder"));
            }
            edits.push((
                at..at,
                format!(
                    "<PLAYLISTS>{nl}<NODE Type=\"0\" Name=\"ROOT\" Count=\"1\">{nl}{folder_xml}{nl}</NODE>{nl}</PLAYLISTS>{nl}"
                ),
            ));
        }
    }

    edits.sort_by_key(|(r, _)| std::cmp::Reverse((r.start, r.end)));
    let mut out = xml.to_string();
    for (range, text) in edits {
        out.replace_range(range, &text);
    }

    Ok(Merged {
        xml: out,
        added,
        playlists: folder.len(),
        missing,
        expect: Expect { tracks, folder },
    })
}

/// Holds `merged` to the promise that nothing in `old` was changed or lost:
/// every track is still there byte for byte (with its cues and grid), every
/// playlist and folder outside `noceur` too, and the collection and the
/// `noceur` folder hold exactly what was meant to go in.
pub fn check(old: &str, merged: &Merged) -> Result<(), Error> {
    let fail = |why: String| Err(Error::Check(why));
    let before = scan(old)?;
    let after = scan(&merged.xml).map_err(|e| Error::Check(e.to_string()))?;
    let new = &merged.xml;

    if old[..before.collection_open.start] != new[..after.collection_open.start] {
        return fail("the header changed".into());
    }
    if after.tracks.len() != merged.expect.tracks {
        return fail(format!(
            "{} tracks, {} expected",
            after.tracks.len(),
            merged.expect.tracks
        ));
    }
    if after.entries != Some(after.tracks.len()) {
        return fail("the collection's Entries is not its track count".into());
    }
    let now: HashMap<u64, &str> = after
        .tracks
        .iter()
        .map(|t| (t.id, &new[t.span.clone()]))
        .collect();
    if now.len() != after.tracks.len() {
        return fail("two tracks share an ID".into());
    }
    for t in &before.tracks {
        if now.get(&t.id) != Some(&&old[t.span.clone()]) {
            return fail(format!("track {} changed", t.id));
        }
    }

    let theirs = |s: &Scan, text: &str| -> Vec<String> {
        s.root_children
            .iter()
            .filter(|c| !c.ours)
            .map(|c| text[c.span.clone()].to_string())
            .collect()
    };
    if theirs(&before, old) != theirs(&after, new) {
        return fail("a playlist outside the noceur folder changed".into());
    }
    let root = after.root.as_ref().ok_or(Error::Check("no ROOT".into()))?;
    if root.count != Some(after.root_children.len()) {
        return fail("ROOT's Count is not its number of children".into());
    }
    if after.folder != merged.expect.folder {
        return fail("the noceur folder is not what was sent".into());
    }
    for (_, keys) in &after.folder {
        if let Some(k) = keys.iter().find(|k| !now.contains_key(k)) {
            return fail(format!(
                "a playlist names track {k}, which is not in the collection"
            ));
        }
    }
    Ok(())
}

/* ── Reading ───────────────────────────────────────────────────────────── */

#[derive(Debug, Default)]
struct Scan {
    /// The `<COLLECTION …>` tag (the whole element when it is empty).
    collection_open: Range<usize>,
    /// Where `</COLLECTION>` starts; `None` for `<COLLECTION … />`.
    collection_close: Option<usize>,
    entries: Option<usize>,
    tracks: Vec<Track>,
    playlists: bool,
    root: Option<Root>,
    root_children: Vec<Child>,
    /// The playlists in the `noceur` folder, with their track keys.
    folder: Vec<(String, Vec<u64>)>,
    document_close: Option<usize>,
}

#[derive(Debug)]
struct Track {
    id: u64,
    location: String,
    span: Range<usize>,
}

#[derive(Debug)]
struct Root {
    open: Range<usize>,
    /// Where ROOT's `</NODE>` starts; `None` when it is self-closing.
    close: Option<usize>,
    count: Option<usize>,
}

#[derive(Debug)]
struct Child {
    span: Range<usize>,
    /// The `noceur` folder (the first, should there be two).
    ours: bool,
}

/// An element still open while scanning.
struct Open {
    name: String,
    start: usize,
    end_of_tag: usize,
    attrs: HashMap<String, String>,
}

fn scan(xml: &str) -> Result<Scan, Error> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Open> = Vec::new();
    let mut s = Scan::default();
    let mut seen_collection = false;
    let mut in_ours = false;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader
            .read_event()
            .map_err(|e| Error::Xml(format!("at byte {}: {e}", reader.error_position())))?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(e) => {
                let open = opened(&e, start, end)?;
                let parents: Vec<&str> = stack.iter().map(|o| o.name.as_str()).collect();
                if open.name == "NODE" && parents == ["DJ_PLAYLISTS", "PLAYLISTS", "NODE"] {
                    in_ours = !s.root_children.iter().any(|c| c.ours) && is_folder(&open);
                }
                // A playlist in the folder, before its entries arrive.
                if open.name == "NODE"
                    && in_ours
                    && parents == ["DJ_PLAYLISTS", "PLAYLISTS", "NODE", "NODE"]
                {
                    s.folder.push((name_of(&open), Vec::new()));
                }
                stack.push(open);
            }
            Event::Empty(e) => {
                let open = opened(&e, start, end)?;
                element(
                    &mut s,
                    &stack,
                    &open,
                    end,
                    None,
                    &mut in_ours,
                    &mut seen_collection,
                );
            }
            Event::End(_) => {
                let open = stack.pop().ok_or(Error::Xml("unbalanced end tag".into()))?;
                element(
                    &mut s,
                    &stack,
                    &open,
                    end,
                    Some(start),
                    &mut in_ours,
                    &mut seen_collection,
                );
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(Error::Xml("the file ends inside an element".into()));
    }
    if !seen_collection {
        return Err(Error::Missing("COLLECTION"));
    }
    Ok(s)
}

fn opened(e: &BytesStart, start: usize, end: usize) -> Result<Open, Error> {
    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let mut attrs = HashMap::new();
    for a in e.attributes() {
        let a = a.map_err(|e| Error::Xml(e.to_string()))?;
        let value = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| Error::Xml(e.to_string()))?;
        attrs.insert(
            String::from_utf8_lossy(a.key.as_ref()).into_owned(),
            value.into_owned(),
        );
    }
    Ok(Open {
        name,
        start,
        end_of_tag: end,
        attrs,
    })
}

/// Records an element once it is complete. `close` is where its end tag
/// starts, `None` when it was self-closing.
fn element(
    s: &mut Scan,
    parents: &[Open],
    e: &Open,
    end: usize,
    close: Option<usize>,
    in_ours: &mut bool,
    seen_collection: &mut bool,
) {
    let path: Vec<&str> = parents.iter().map(|o| o.name.as_str()).collect();
    let span = e.start..end;
    match (path.as_slice(), e.name.as_str()) {
        (["DJ_PLAYLISTS"], "COLLECTION") => {
            *seen_collection = true;
            s.collection_open = match close {
                Some(_) => e.start..e.end_of_tag,
                None => span,
            };
            s.collection_close = close;
            s.entries = e.attrs.get("Entries").and_then(|v| v.parse().ok());
        }
        (["DJ_PLAYLISTS", "COLLECTION"], "TRACK") => {
            if let Some(id) = e.attrs.get("TrackID").and_then(|v| v.parse().ok()) {
                s.tracks.push(Track {
                    id,
                    location: e.attrs.get("Location").cloned().unwrap_or_default(),
                    span,
                });
            }
        }
        ([], "DJ_PLAYLISTS") => s.document_close = close,
        (["DJ_PLAYLISTS"], "PLAYLISTS") => s.playlists = true,
        (["DJ_PLAYLISTS", "PLAYLISTS"], "NODE") if s.root.is_none() => {
            s.root = Some(Root {
                open: match close {
                    Some(_) => e.start..e.end_of_tag,
                    None => span,
                },
                close,
                count: e.attrs.get("Count").and_then(|v| v.parse().ok()),
            });
        }
        (["DJ_PLAYLISTS", "PLAYLISTS", "NODE"], "NODE") => {
            // A self-closing folder never set `in_ours` on the way in.
            let ours = match close {
                Some(_) => *in_ours,
                None => !s.root_children.iter().any(|c| c.ours) && is_folder(e),
            };
            s.root_children.push(Child { span, ours });
            *in_ours = false;
        }
        // An empty playlist in the folder; one with entries was pushed on its
        // way in.
        (["DJ_PLAYLISTS", "PLAYLISTS", "NODE", "NODE"], "NODE") if *in_ours && close.is_none() => {
            s.folder.push((name_of(e), Vec::new()));
        }
        (["DJ_PLAYLISTS", "PLAYLISTS", "NODE", "NODE", "NODE"], "TRACK") if *in_ours => {
            if let (Some(k), Some(list)) = (
                e.attrs.get("Key").and_then(|v| v.parse().ok()),
                s.folder.last_mut(),
            ) {
                list.1.push(k);
            }
        }
        _ => {}
    }
}

fn is_folder(e: &Open) -> bool {
    e.attrs.get("Name").map(String::as_str) == Some(FOLDER)
        && e.attrs.get("Type").map(String::as_str) == Some("0")
}

fn name_of(e: &Open) -> String {
    e.attrs.get("Name").cloned().unwrap_or_default()
}

/* ── Writing ───────────────────────────────────────────────────────────── */

fn track_element(id: u64, path: &str, t: &NewTrack, today: &str) -> String {
    let bpm = t
        .bpm
        .map(|b| format!("{b:.2}"))
        .unwrap_or_else(|| "0.00".into());
    let rating = u32::from(t.stars.min(5)) * 51;
    format!(
        "<TRACK TrackID=\"{id}\" Name=\"{}\" Artist=\"{}\" Composer=\"\" Album=\"{}\" Grouping=\"\" \
         Genre=\"{}\" Kind=\"{}\" Size=\"{}\" TotalTime=\"{}\" DiscNumber=\"0\" TrackNumber=\"{}\" \
         Year=\"{}\" AverageBpm=\"{bpm}\" DateAdded=\"{}\" BitRate=\"{}\" SampleRate=\"{}\" \
         Comments=\"{}\" PlayCount=\"0\" Rating=\"{rating}\" Location=\"{}\" Remixer=\"\" \
         Tonality=\"{}\" Label=\"{}\" Mix=\"\" />",
        attr(&t.name),
        attr(&t.artist),
        attr(&t.album),
        attr(&t.genre),
        attr(&t.kind),
        t.size,
        t.total_time,
        t.track_number,
        t.year,
        attr(today),
        t.bit_rate,
        t.sample_rate,
        attr(&t.comments),
        attr(&location(path)),
        attr(&t.tonality),
        attr(&t.label),
    )
}

fn folder_element(lists: &[(String, Vec<u64>)], nl: &str) -> String {
    let mut out = format!(
        "<NODE Name=\"{FOLDER}\" Type=\"0\" Count=\"{}\">{nl}",
        lists.len()
    );
    for (name, keys) in lists {
        let name = attr(name);
        if keys.is_empty() {
            out.push_str(&format!(
                "<NODE Name=\"{name}\" Type=\"1\" KeyType=\"0\" Entries=\"0\" />{nl}"
            ));
            continue;
        }
        out.push_str(&format!(
            "<NODE Name=\"{name}\" Type=\"1\" KeyType=\"0\" Entries=\"{}\">{nl}",
            keys.len()
        ));
        for k in keys {
            out.push_str(&format!("<TRACK Key=\"{k}\" />{nl}"));
        }
        out.push_str(&format!("</NODE>{nl}"));
    }
    out.push_str("</NODE>");
    out
}

/// The tag with `name="…"` set to `value`, everything else kept as written.
fn set_attr(tag: &str, name: &str, value: usize) -> String {
    let needle = format!(" {name}=\"");
    match tag.find(&needle) {
        Some(at) => {
            let from = at + needle.len();
            let to = from + tag[from..].find('"').unwrap_or(0);
            format!("{}{value}{}", &tag[..from], &tag[to..])
        }
        None => {
            let at = tag.trim_end_matches(['>', '/', ' ']).len();
            format!("{} {name}=\"{value}\"{}", &tag[..at], &tag[at..])
        }
    }
}

/// An attribute value, escaped. Control characters XML 1.0 cannot hold are
/// dropped.
fn attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// `X:\ssd\a b.aiff` → `file://localhost/X:/ssd/a%20b.aiff`, escaped the way
/// rekordbox escapes its own: space, `%`, `&`, `'` and everything outside
/// ASCII, the rest as it is.
pub fn location(path: &str) -> String {
    let mut out = String::from("file://localhost/");
    for b in path.replace('\\', "/").trim_start_matches('/').bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => out.push(b as char),
            b'/' | b':' | b'-' | b'.' | b'_' | b'~' | b'(' | b')' | b'!' | b'#' | b'$' | b','
            | b';' | b'=' | b'@' | b'+' | b'[' | b']' => out.push(b as char),
            b => out.push_str(&format!("%{b:02x}")),
        }
    }
    out
}

/// The path a `Location` points at.
pub fn location_path(location: &str) -> String {
    let rest = location
        .strip_prefix("file://localhost/")
        .or_else(|| location.strip_prefix("file:///"))
        .unwrap_or(location);
    let bytes = rest.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let path = String::from_utf8_lossy(&out).into_owned();
    // A Windows path keeps its drive; anything else gets its root back.
    if path.as_bytes().get(1) == Some(&b':') {
        path.replace('/', "\\")
    } else {
        format!("/{path}")
    }
}

/// Paths compared the way Windows compares them.
fn key(path: &str) -> String {
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

#[cfg(test)]
mod tests;
