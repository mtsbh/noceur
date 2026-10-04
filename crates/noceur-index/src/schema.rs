//! Schema definition and migration runner.

/// Current schema version. Bump when adding a migration.
pub const SCHEMA_VERSION: i32 = 14;

/// Migrations applied in order. Index 0 upgrades version 0 → 1.
pub const MIGRATIONS: &[&str] = &[V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11, V12, V13, V14];

const V1: &str = r#"
CREATE TABLE volume (
    id           INTEGER PRIMARY KEY,
    kind         TEXT    NOT NULL,           -- fixed | removable | network
    fs_uuid      TEXT    NOT NULL UNIQUE,    -- stable identity across drive letters
    label        TEXT    NOT NULL DEFAULT '',
    mount_path   TEXT    NOT NULL,
    online       INTEGER NOT NULL DEFAULT 1,
    last_seen_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE dir (
    id         INTEGER PRIMARY KEY,
    volume_id  INTEGER NOT NULL REFERENCES volume(id) ON DELETE CASCADE,
    parent_id  INTEGER REFERENCES dir(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    path_lower TEXT    NOT NULL,
    mtime      INTEGER NOT NULL DEFAULT 0,
    scan_state TEXT    NOT NULL DEFAULT 'pending',
    UNIQUE (volume_id, path_lower)
);

CREATE TABLE content (
    id          INTEGER PRIMARY KEY,
    audio_hash  TEXT UNIQUE,                 -- filled by a later milestone
    quick_key   TEXT NOT NULL UNIQUE,        -- size + head/tail hash
    byte_size   INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER,
    sample_rate INTEGER,
    channels    INTEGER,
    bit_depth   INTEGER,
    codec       TEXT
);

CREATE TABLE file (
    id         INTEGER PRIMARY KEY,
    dir_id     INTEGER NOT NULL REFERENCES dir(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    ext        TEXT    NOT NULL DEFAULT '',
    size       INTEGER NOT NULL DEFAULT 0,
    mtime      INTEGER NOT NULL DEFAULT 0,
    kind       INTEGER NOT NULL,             -- EntryKind::group()
    content_id INTEGER REFERENCES content(id) ON DELETE SET NULL,
    state      TEXT    NOT NULL DEFAULT 'ok',-- ok|locked|unreadable|undecodable|missing
    UNIQUE (dir_id, name)
);

CREATE TABLE content_tag (
    content_id   INTEGER PRIMARY KEY REFERENCES content(id) ON DELETE CASCADE,
    artist       TEXT,
    title        TEXT,
    album        TEXT,
    album_artist TEXT,
    genre        TEXT,
    year         INTEGER,
    track_no     INTEGER,
    comment      TEXT,
    isrc         TEXT,
    bpm_tag      REAL,
    key_tag      TEXT,
    key_norm     INTEGER,
    has_artwork  INTEGER NOT NULL DEFAULT 0,
    read_at      INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_file_dir    ON file(dir_id, kind, name);
CREATE INDEX idx_file_content ON file(content_id);
CREATE INDEX idx_dir_parent  ON dir(parent_id);
"#;

/// Thumbnail cache.
///
/// Keyed by content rather than by file, because identity is the audio: the same
/// track copied to the party SSD and into a Rekordbox folder is one `content`
/// row, and rendering its cover three times would be three decodes of the same
/// JPEG for three rows that look identical.
///
/// `miss` records that a decode was already attempted and produced nothing, so a
/// file with corrupt or unreadable artwork is not re-decoded on every scroll past
/// it. Without it the expensive case — failure — would be the uncached one.
const V2: &str = r#"
CREATE TABLE thumb (
    content_id INTEGER NOT NULL REFERENCES content(id) ON DELETE CASCADE,
    size       TEXT    NOT NULL,             -- small | medium | large
    png        BLOB,                         -- NULL when miss = 1
    miss       INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (content_id, size)
);
"#;

/// Full-text search over the names and tags of every indexed file.
///
/// A plain FTS5 table rather than an external-content one: the text it indexes is
/// spread across `file` and `content_tag`, and external content wants a single
/// source table. `rowid` is the `file.id`, so a match joins straight back to the
/// row it came from.
///
/// `path` is indexed alongside the tags because a DJ library encodes a great deal
/// in its folder names — labels, years, the set a track was played in — and a
/// search that ignored them would miss the obvious query.
///
/// `remove_diacritics 2` makes "Rêves" findable by typing "reves", which matters
/// for a library full of names nobody can type.
const V3: &str = r#"
CREATE VIRTUAL TABLE file_fts USING fts5(
    name, artist, title, album, path,
    tokenize = "unicode61 remove_diacritics 2"
);
"#;

/// Backfills the search index with everything already scanned.
///
/// V3 created the table empty, which left every folder indexed before it
/// invisible to search until it happened to be reopened — a library that had
/// been browsed for weeks would have answered `Ctrl+F` with nothing.
///
/// Written as its own migration rather than folded into V3 because V3 has
/// already run on real databases. The `NOT IN` makes it a no-op where V3 and V4
/// are applied together on a fresh file.
const V4: &str = r#"
INSERT INTO file_fts (rowid, name, artist, title, album, path)
SELECT f.id,
       f.name,
       COALESCE(ct.artist, ''),
       COALESCE(ct.title, ''),
       COALESCE(ct.album, ''),
       d.path_lower
FROM file f
JOIN dir d ON d.id = f.dir_id
LEFT JOIN content_tag ct ON ct.content_id = f.content_id
WHERE f.id NOT IN (SELECT rowid FROM file_fts);
"#;

/// Waveform envelope cache.
///
/// Keyed by content for the same reason as [`thumb`]: an envelope costs a full
/// decode of the track, which is by far the most expensive thing this app does to
/// a file, and the same audio in three folders should pay for it once.
///
/// `data` holds the bands interleaved, bucket by bucket, at true linear scale.
/// Normalising to the loudest moment is left to whatever draws it: a master that
/// never exceeds a third of full scale should still fill the bar, but the stored
/// numbers are the ones an analysis pass would want later.
///
/// `buckets` is stored beside the blob so a later change to the resolution
/// invalidates old rows by simply missing them, rather than drawing a waveform at
/// the wrong scale. The reader checks the blob's length too, which catches a
/// change in the number of bands.
///
/// `miss` mirrors the thumbnail cache: a file the decoder cannot read is the
/// slowest case to discover, so discovering it twice would be the worst of both.
const V5: &str = r#"
CREATE TABLE envelope (
    content_id INTEGER PRIMARY KEY REFERENCES content(id) ON DELETE CASCADE,
    buckets    INTEGER NOT NULL,
    data       BLOB,                         -- NULL when miss = 1
    miss       INTEGER NOT NULL DEFAULT 0
);
"#;

/// Spectrogram cache.
///
/// Keyed by content and by shape, like the envelope beside it and for the same
/// reason: a grid stored at one size cannot be drawn at another without
/// inventing detail, so a build that changes either number must miss rather than
/// stretch what it finds.
///
/// Far larger than an envelope — sixty kilobytes against eight — so this is only
/// ever written for tracks somebody actually looked at, never during a scan.
const V6: &str = r#"
CREATE TABLE spectrogram (
    content_id INTEGER PRIMARY KEY REFERENCES content(id) ON DELETE CASCADE,
    columns    INTEGER NOT NULL,
    bands      INTEGER NOT NULL,
    data       BLOB,                         -- NULL when miss = 1
    miss       INTEGER NOT NULL DEFAULT 0
);
"#;

/// What a track is, regardless of its tags.
///
/// A side table rather than the `audio_hash` column reserved for it in V1: that
/// column is `UNIQUE`, which is precisely wrong for a value copies are supposed
/// to share. Rebuilding `content` to drop the constraint would mean dropping a
/// table four others cascade from, with foreign keys on, to change one index —
/// so the reserved column stays unused and this sits beside the thumbnail,
/// envelope and spectrogram caches, keyed the same way they are.
const V7: &str = r#"
CREATE TABLE audio_identity (
    content_id INTEGER PRIMARY KEY REFERENCES content(id) ON DELETE CASCADE,
    hash       TEXT NOT NULL
);

CREATE INDEX idx_audio_identity_hash ON audio_identity(hash);
"#;

/// Re-read the files whose codec was never recorded.
///
/// `content.codec` has been in the schema since V1 and nothing ever wrote to it,
/// so the column the list offers as a column was empty for every row in every
/// library. The probe fills it now, but a file is only re-probed when its mtime
/// moves past the time it was last read — and nothing about these files has
/// changed, only what is asked of them.
///
/// Marking them stale is what asks. It costs one tag read each, spread over the
/// folders as they are opened rather than all at once.
const V8: &str = r#"
UPDATE content_tag
SET read_at = 0
WHERE content_id IN (SELECT id FROM content WHERE codec IS NULL);
"#;

/// Star ratings, so the list can show and sort by them.
///
/// Every tag row is marked stale, because a rating was never read before and
/// nothing about the files has changed to make the scanner look again. The
/// cost is one tag read per file, spread over folders as they are opened.
const V9: &str = r#"
ALTER TABLE content_tag ADD COLUMN rating INTEGER;
UPDATE content_tag SET read_at = 0;
"#;

/// Detected tempo and key.
///
/// Keyed by content like the other caches, and carried across a tag write the
/// way the waveform is, since writing a tag changes the bytes but not a sample
/// of the audio. What was measured stays beside what the file says, so the list
/// can show where the two disagree without measuring again.
///
/// `version` and the tempo range are part of the answer: a row measured by an
/// older model, or with half/double time settled by another range, is stale and
/// measured again rather than shown. `miss` records a file the decoder would
/// not read, so it is not retried on every visit.
const V10: &str = r#"
CREATE TABLE analysis (
    content_id     INTEGER PRIMARY KEY REFERENCES content(id) ON DELETE CASCADE,
    version        INTEGER NOT NULL,
    range_min      REAL    NOT NULL,
    range_max      REAL    NOT NULL,
    bpm            REAL,
    key_norm       INTEGER,
    key_confidence REAL,
    miss           INTEGER NOT NULL DEFAULT 0
);
"#;

/// Integrated loudness and true peak, from Check quality.
///
/// Keyed by content and carried across tag writes, like `analysis`: a tag write
/// does not change a sample. `peak_floor` marks a true peak that is only a lower
/// bound, from a lossy file whose decoder clipped at full scale.
const V11: &str = r#"
CREATE TABLE loudness (
    content_id INTEGER PRIMARY KEY REFERENCES content(id) ON DELETE CASCADE,
    lufs       REAL,
    true_peak  REAL,
    peak_floor INTEGER NOT NULL DEFAULT 0
);
"#;

/// What Check quality found broken in a content: `"0:30 missing · 2 dropouts"`,
/// or empty for a sound file. No row is not checked yet.
///
/// Carried across tag writes like `loudness`: a tag write moves no sample.
const V12: &str = r#"
CREATE TABLE damage (
    content_id INTEGER PRIMARY KEY REFERENCES content(id) ON DELETE CASCADE,
    summary    TEXT NOT NULL
);
"#;

/// The record label, for the library overview's top labels.
///
/// Every tag row is marked stale, as in V9: the label was never read before, and
/// nothing about the files has changed to make the scanner look again.
const V13: &str = r#"
ALTER TABLE content_tag ADD COLUMN label TEXT;
UPDATE content_tag SET read_at = 0;
"#;

/// Named playlists, and the tracks ticked into them.
///
/// A track is held by its path, not its `file` row: a rescan deletes the row of a
/// file it no longer sees, and a playlist must not lose a track because its
/// folder was looked at while a drive was unplugged. `path_key` is the path as
/// [`crate::scan`] folds it, so the same file is in a playlist once.
const V14: &str = r#"
CREATE TABLE playlist (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE playlist_track (
    playlist_id INTEGER NOT NULL REFERENCES playlist(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    path        TEXT    NOT NULL,
    path_key    TEXT    NOT NULL,
    added_at    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (playlist_id, path_key)
);

CREATE INDEX idx_playlist_track_key ON playlist_track(path_key);
"#;
