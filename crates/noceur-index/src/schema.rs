//! Schema definition and migration runner.

/// Current schema version. Bump when adding a migration.
pub const SCHEMA_VERSION: i32 = 2;

/// Migrations applied in order. Index 0 upgrades version 0 → 1.
pub const MIGRATIONS: &[&str] = &[V1, V2];

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
