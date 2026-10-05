import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type RowDisplay = {
  fileId: number;
  kind: number;
  name: string;
  bpm: string;
  key: string;
  /** Canonical key 0..23, for colouring by Camelot position. Null when unknown. */
  keyNorm: number | null;
  duration: string;
  artist: string;
  title: string;
  album: string;
  albumArtist: string;
  genre: string;
  /** Release year from the tags, not the file's date on disk. */
  year: string;
  trackNo: string;
  comment: string;
  isrc: string;
  /** `44.1 kHz`, or empty. */
  sampleRate: string;
  /** `16 bit`, or empty for formats with no bit depth. */
  bitDepth: string;
  codec: string;
  /** Stars, 0 for unrated. */
  rating: number;
  size: string;
  /** `YYYY-MM-DD HH:MM` in UTC — when the file changed, not when it was released. */
  modified: string;
  state: string;
  /** True only for audio files the last probe found a picture in. */
  hasArt: boolean;
  /** Tempo measured from the audio, as it would be written; empty when not measured. */
  detectedBpm: string;
  /** Key measured from the audio; empty when not measured. */
  detectedKey: string;
  detectedKeyNorm: number | null;
  /** The file has a BPM tag and it is not the one measured. */
  bpmDiffers: boolean;
  /** The file has a key tag and it is not the one measured. */
  keyDiffers: boolean;
};

/** The cover-art sizes the backend will render. */
export type ArtSize = "small" | "medium" | "large";

/**
 * URL of one row's cover art.
 *
 * Cover art is fetched by the web view as an ordinary image rather than returned
 * from a command, so the bytes never pass through JSON and the browser does its
 * own caching and off-thread decoding. `convertFileSrc` spells the custom scheme
 * the way the current platform needs it — `http://thumb.localhost/…` on Windows,
 * `thumb://localhost/…` elsewhere.
 */
export function thumbUrl(fileId: number, size: ArtSize): string {
  return `${convertFileSrc(String(fileId), "thumb")}?size=${size}`;
}

/**
 * URL of the picture a file actually carries, at the size it was embedded.
 *
 * Not a thumbnail: the editor shows artwork to be judged rather than
 * recognised, and 88 pixels cannot answer "is this the right cover". Served
 * uncached, and `version` exists to defeat the web view's own cache — the URL
 * says nothing about which picture it holds, so replacing or removing the
 * artwork has to change the URL or the old image goes on being drawn.
 */
export function fullArtUrl(fileId: number, version = 0): string {
  return `${convertFileSrc(String(fileId), "thumb")}?size=full&v=${version}`;
}

/**
 * Shell icon for a folder named by path.
 *
 * The tree reads the filesystem rather than the index, so its nodes have no row
 * to point at. The `0` is a placeholder the URL shape requires; the backend
 * prefers `path` when it is present, and answers it without touching the index
 * at all.
 */
export function folderIconUrl(path: string, size: ArtSize = "small"): string {
  return `${convertFileSrc("0", "thumb")}?size=${size}&path=${encodeURIComponent(path)}`;
}

export type Segment = { label: string; path: string };

/** A contiguous slice of a listing, starting at `offset`. */
export type RowWindow = { offset: number; rows: RowDisplay[] };

export type DirListing = {
  /** Identifies the scanned directory for follow-up window requests. */
  dirId: number;
  path: string;
  segments: Segment[];
  /** Rows in the whole listing, not in `first`. */
  total: number;
  /** How many of those are audio — zero means a folder of folders. */
  audio: number;
  /** The window at offset 0, so the first screen paints without a round trip. */
  first: RowWindow;
  /** How long enumeration took, so slow network paths stay visible. */
  scanMs: number;
};

/** Columns the list can be sorted by. Must match `SortColumn::as_str` in Rust. */
export type SortColumn =
  | "name"
  | "bpm"
  | "key"
  | "duration"
  | "artist"
  | "title"
  | "album"
  | "albumArtist"
  | "genre"
  | "year"
  | "trackNo"
  | "comment"
  | "isrc"
  | "sampleRate"
  | "bitDepth"
  | "codec"
  | "rating"
  | "size"
  | "modified";

/**
 * How a listing is ordered.
 *
 * Sorting happens in SQLite, so the order is globally correct across a 40k-file
 * folder rather than correct only across the rows that happen to be loaded. The
 * same order must be sent when opening a directory and when reading windows of
 * it: a window cut in a different order than the listing it belongs to shows
 * duplicated and missing rows rather than an error.
 */
export type Order = { sort: SortColumn; desc: boolean };

/**
 * Which rows a listing includes. Mirrors `Show` in `noceur-index`.
 *
 * `tracks` keeps the folders — it hides the artwork, cue sheets and `.nfo`s a
 * download leaves in a release folder, not the thing you navigate with.
 * `audioOnly` drops those too and is not something the browser asks for.
 */
export type Show = "everything" | "tracks" | "audioOnly";

/**
 * Scans and counts a directory, returning its first window of rows.
 *
 * Does not read tags — see `probeBatch`, which fills those in afterwards.
 */
export function openDir(path: string, order: Order, show: Show = "everything"): Promise<DirListing> {
  return invoke<DirListing>("open_dir", {
    req: { path, show, sort: order.sort, desc: order.desc },
  });
}

/**
 * Reads one window of an already-opened directory.
 *
 * `order` and `show` must match the values `openDir` was called with — the
 * backend cuts the window from the ordered, filtered list, so a mismatch
 * silently returns rows from a different list than the one on screen.
 */
export function listRows(
  dirId: number,
  offset: number,
  limit: number,
  order: Order,
  show: Show = "everything",
): Promise<RowWindow> {
  return invoke<RowWindow>("list_rows", {
    req: { dirId, offset, limit, show, sort: order.sort, desc: order.desc },
  });
}

export function parentOf(path: string): Promise<string | null> {
  return invoke<string | null>("parent_of", { path });
}

/**
 * Subdirectories of one path, for a single level of the tree.
 *
 * Reads the filesystem rather than the index, so unfolding a branch does not
 * write an index row for every folder that was merely looked at.
 */
export function subdirs(path: string): Promise<Segment[]> {
  return invoke<Segment[]>("subdirs", { path });
}

/** What one probe batch accomplished. */
export type ProbeProgress = { probed: number; done: boolean; elapsedMs: number };

/**
 * Reads tags for one batch of a directory's files.
 *
 * Called in a loop after the folder is already on screen. Each call takes and
 * releases the index lock once, so listing and cover-art requests interleave with
 * it instead of queueing behind a folder-long hold.
 */
export function probeBatch(dirId: number): Promise<ProbeProgress> {
  return invoke<ProbeProgress>("probe", { dirId });
}

/** The drives the tree is rooted at. */
export function roots(): Promise<Segment[]> {
  return invoke<Segment[]>("roots");
}

/** One search result, already formatted for display. */
export type SearchHit = {
  fileId: number;
  kind: number;
  name: string;
  dirPath: string;
  bpm: string;
  key: string;
  duration: string;
  artist: string;
};

/**
 * Searches the indexed library.
 *
 * `scopes` are the folders to look in — the pinned ones plus the tree the user
 * is currently in. Searching the current directory alone would miss the case the
 * feature exists for: finding a track when you cannot remember which folder it
 * is in.
 */
export function search(query: string, scopes: string[]): Promise<SearchHit[]> {
  return invoke<SearchHit[]>("search", { req: { query, scopes } });
}

/** Lists every folder under `roots` in the background, so search reaches them. */
export function indexFolders(roots: string[]): Promise<void> {
  return invoke<void>("index_folders", { roots });
}

/** A tag the library can be listed by. */
export type TagLink = { field: "artist" | "genre" | "label"; value: string };

/** Every track tagged with exactly this value, case aside. */
export function searchTag(link: TagLink, scopes: string[]): Promise<SearchHit[]> {
  return invoke<SearchHit[]>("search_tag", { field: link.field, value: link.value, scopes });
}

/** What the transport bar shows. */
export type Transport = {
  path: string;
  name: string;
  playing: boolean;
  positionMs: number;
  durationMs: number;
  position: string;
  duration: string;
  error: string;
  /** The row being played, so the bar can show its art. 0 when nothing is loaded. */
  fileId: number;
  /** Of the loaded track, for the spectrogram's frequency axis. 0 when unknown. */
  sampleRate: number;
  artist: string;
  title: string;
  bpm: string;
  key: string;
  keyNorm: number | null;
  /** 0..100. */
  volume: number;
};

/** Loads a row into the player and starts it. */
export function play(fileId: number): Promise<Transport> {
  return invoke<Transport>("play", { fileId });
}

/**
 * Drives the transport: `toggle`, `pause`, `resume`, `stop`, `seek`, `volume`.
 *
 * `seek` takes milliseconds; `volume` takes 0–100. One command rather than six,
 * because from the UI's side they are the same interaction — change what the
 * transport is doing, and tell me what it is doing now.
 */
export function transportCmd(
  action: "toggle" | "pause" | "resume" | "stop" | "seek" | "volume",
  value?: number,
): Promise<Transport> {
  return invoke<Transport>("transport_cmd", { action, value });
}

/** Polled while something is playing, to move the position. */
export function transportState(): Promise<Transport> {
  return invoke<Transport>("transport_state");
}

/** Opens Explorer with the row selected. */
export function reveal(fileId: number): Promise<void> {
  return invoke<void>("reveal", { fileId });
}

/** The full path of a row, for copying. */
export function pathOf(fileId: number): Promise<string> {
  return invoke<string>("path_of", { fileId });
}

/**
 * Starts a native drag of the given rows.
 *
 * The payload is a real `CF_HDROP` offered from the window's own handle, so
 * Rekordbox, Explorer and anything else that accepts files sees a file drop
 * rather than a web page's idea of one. Returns as soon as the drag begins; the
 * drop outcome arrives on the `drag-finished` event.
 */
export function startDrag(fileIds: number[]): Promise<void> {
  return invoke<void>("start_drag", { fileIds });
}

/** A native drag of paths the index has no rows for: folders in the tree. */
export function startDragPaths(paths: string[]): Promise<void> {
  return invoke<void>("start_drag_paths", { paths });
}

/** Copy, move or delete rows through the shell's own file operation. */
export function fileOp(
  action: "copy" | "move" | "delete",
  fileIds: number[],
  destination?: string,
): Promise<void> {
  return invoke<void>("file_op", { action, fileIds, destination });
}

/** The same, for paths the index has never seen — files dropped in from Explorer. */
export function fileOpPaths(
  action: "copy" | "move" | "delete",
  paths: string[],
  destination?: string,
): Promise<void> {
  return invoke<void>("file_op_paths", { action, paths, destination });
}

/** Copy or move? Decided the way Explorer decides it: move within a volume, copy across. */
export function dropEffect(sources: string[], destination: string): Promise<string> {
  return invoke<string>("drop_effect", { sources, destination });
}

/**
 * Shows the shell's own context menu for the given rows.
 *
 * The same menu Explorer shows, with the same shell extensions in it, because it
 * is the shell building it. Returns once the menu is up; what the user picks
 * happens inside the shell.
 */
export function shellMenu(fileIds: number[]): Promise<void> {
  return invoke<void>("shell_menu", { fileIds });
}

/** Files on the Windows clipboard, shared with Explorer. */
export type Clip = { paths: string[]; cut: boolean };

export function clipboardPut(paths: string[], cut: boolean): Promise<void> {
  return invoke("clipboard_put", { paths, cut });
}

/** The files on the clipboard, or null when it holds something else. */
export function clipboardFiles(): Promise<Clip | null> {
  return invoke("clipboard_files");
}

/** Empties the clipboard if it still holds these files - after pasting a cut. */
export function clipboardRelease(paths: string[]): Promise<void> {
  return invoke("clipboard_release", { paths });
}

/** Puts the last rename job back. `restored` is 0 when there was nothing to undo. */
export function undoRename(): Promise<{ restored: number; failed: number }> {
  return invoke("undo_rename");
}

/** Whether Telegram Desktop is installed, so there is somewhere to send to. */
export function telegramAvailable(): Promise<boolean> {
  return invoke<boolean>("telegram_available");
}

/**
 * Hands files to Telegram Desktop, which comes to the front with its chat
 * picker. Nothing is sent until the user picks a chat there.
 */
export function sendToTelegram(fileIds: number[]): Promise<void> {
  return invoke<void>("send_to_telegram", { fileIds });
}

export type Playlist = { id: number; name: string; tracks: number; durationMs: number };

/** One track of a playlist. `row` is null when the index has no file at `path`. */
export type PlaylistTrack = { path: string; row: RowDisplay | null };

export function playlists(): Promise<Playlist[]> {
  return invoke<Playlist[]>("playlists");
}

export function createPlaylist(name: string): Promise<number> {
  return invoke<number>("playlist_create", { name });
}

export function renamePlaylist(id: number, name: string): Promise<void> {
  return invoke<void>("playlist_rename", { id, name });
}

export function deletePlaylist(id: number): Promise<void> {
  return invoke<void>("playlist_delete", { id });
}

export function playlistTracks(id: number): Promise<PlaylistTrack[]> {
  return invoke<PlaylistTrack[]>("playlist_tracks", { id });
}

/** Adds the files, or takes them out when every one is already in. */
export function togglePlaylist(
  id: number,
  fileIds: number[],
): Promise<{ added: number; removed: number }> {
  return invoke("playlist_toggle", { id, fileIds });
}

export function removeFromPlaylist(id: number, paths: string[]): Promise<number> {
  return invoke<number>("playlist_remove", { id, paths });
}

/** Moves `paths` so the first lands at `to`, counted without them. */
export function reorderPlaylist(id: number, paths: string[], to: number): Promise<void> {
  return invoke<void>("playlist_reorder", { id, paths, to });
}

/** Playlists that hold every one of the files. */
export function playlistsHolding(fileIds: number[]): Promise<number[]> {
  return invoke<number[]>("playlists_holding", { fileIds });
}

/** Opens files in a program, all of them in one launch. */
export function openWith(program: string, fileIds: number[]): Promise<void> {
  return invoke<void>("open_with", { program, fileIds });
}

/** Opens a link in the default browser. Only http and https are accepted. */
export function openUrl(url: string): Promise<void> {
  return invoke<void>("open_url", { url });
}

/** What the meters read: the newest frames sent to the output. */
export type ScopeFrames = {
  /** Sample rate the frames play at. */
  rate: number;
  left: Float32Array;
  right: Float32Array;
};

/**
 * The newest `frames` stereo frames sent to the output, taken before the volume.
 *
 * Comes back as raw bytes - see `scope_frames` in `commands.rs` for the layout.
 */
export async function scopeFrames(frames: number): Promise<ScopeFrames> {
  const buf = await invoke<ArrayBuffer>("scope_frames", { frames });
  const view = new DataView(buf);
  const rate = view.byteLength >= 4 ? view.getUint32(0, true) : 44100;
  const n = Math.max(0, (view.byteLength - 4) >> 2);
  const left = new Float32Array(n);
  const right = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    left[i] = view.getInt16(4 + i * 4, true) / 32767;
    right[i] = view.getInt16(6 + i * 4, true) / 32767;
  }
  return { rate, left, right };
}

/**
 * The amplitude envelope of one file: interleaved [rms, peak] per bucket.
 *
 * Empty when the row is not audio or will not decode; the transport falls back
 * to a plain progress bar. The first call for a track pays for a full decode, so
 * this is deliberately fired without blocking anything else the UI is doing.
 */
export async function envelope(fileId: number): Promise<Uint8Array> {
  const data = await invoke<number[]>("envelope", { fileId });
  return Uint8Array.from(data);
}

/**
 * The tags of a selection, merged into one form.
 *
 * A field is `null` when the selected files disagree about it, and `""` when
 * they agree it is empty. The editor must keep those apart: a blank box that
 * meant "various" would write that blank to every file if it were submitted.
 */
export type TagForm = {
  count: number;
  /** The file name for one file, else how many there are. */
  subject: string;
  artist: string | null;
  title: string | null;
  album: string | null;
  albumArtist: string | null;
  genre: string | null;
  comment: string | null;
  isrc: string | null;
  label: string | null;
  year: string | null;
  trackNo: string | null;
  bpm: string | null;
  key: string | null;
  /** What the file is, for one file only. */
  facts: FileFacts | null;
};

/** What a file is, as text. Read-only: none of it is a tag. Empty when unknown. */
export type FileFacts = {
  format: string;
  bitrate: string;
  length: string;
  sampleRate: string;
  bitDepth: string;
  channels: string;
  size: string;
};

/** The editable fields, in the order the editor lays them out. */
export const TAG_FIELDS = [
  ["title", "Title"],
  ["artist", "Artist"],
  ["album", "Album"],
  ["albumArtist", "Album artist"],
  ["genre", "Genre"],
  ["label", "Label"],
  ["year", "Year"],
  ["trackNo", "Track"],
  ["bpm", "BPM"],
  ["key", "Key"],
  ["isrc", "ISRC"],
  ["comment", "Comment"],
] as const;

export type TagField = (typeof TAG_FIELDS)[number][0];

/** One file a write could not touch, and why. */
export type WriteFailure = { name: string; reason: string };

/** What a write did, per file. */
export type WriteReport = {
  written: number;
  failures: WriteFailure[];
};

export function readTags(fileIds: number[]): Promise<TagForm> {
  return invoke<TagForm>("read_tags", { fileIds });
}

/**
 * Writes tags to a selection.
 *
 * Only fields present in `edit` are touched; a field set to `""` clears the tag.
 * Absent means leave every file's own value alone, which is why the editor sends
 * only the boxes the user actually typed in.
 */
export function writeTags(
  fileIds: number[],
  edit: Partial<Record<TagField, string>>,
  custom: Record<string, string> = {},
  rating?: string,
  hashtags?: { set: string[]; clear: string[] },
): Promise<WriteReport> {
  return invoke<WriteReport>("write_tags", {
    fileIds,
    edit: { ...edit, custom, rating, hashtags },
  });
}

/**
 * The quick-tag field for values kept as `#tags` at the end of the comment -
 * the one free text field every DJ app reads from every format. Matches
 * `HASHTAG_FIELD` on the other side.
 */
export const HASHTAG_FIELD = "noceur:hashtags";

/**
 * The key a rating travels under in the quick-tag map.
 *
 * Not a tag name — ratings live in a frame of their own in every format. Matches
 * `RATING_FIELD` on the other side.
 */
export const RATING_FIELD = "noceur:rating";

/**
 * A rating changed somewhere, so every place showing it can agree.
 *
 * The list and the quick-tag panel both show and set stars on the same track.
 * Each kept its own copy, so the panel went on holding the rating it read when
 * the track started - and wrote it back over a star cleared in the list the
 * moment the track changed.
 */
export type Rated = { ids: number[]; stars: number; from: "list" | "quick" };

const RATED_EVENT = "noceur:rated";

export function announceRating(rated: Rated) {
  window.dispatchEvent(new CustomEvent<Rated>(RATED_EVENT, { detail: rated }));
}

/** Listens for [`announceRating`]; returns the function that stops listening. */
export function onRating(handler: (rated: Rated) => void): () => void {
  const listener = (e: Event) => handler((e as CustomEvent<Rated>).detail);
  window.addEventListener(RATED_EVENT, listener);
  return () => window.removeEventListener(RATED_EVENT, listener);
}

/** One sleeve a source found. */
export type CoverCandidate = {
  /** Which service found it. */
  source: string;
  title: string;
  artist: string;
  /** Year, country, label — whatever that source knows, pre-joined. */
  detail: string;
  /** Small rendering, loaded straight into an `<img>` by the picker. */
  thumbnail: string;
  /** What gets downloaded and embedded when this one is chosen. */
  full: string;
  size: number | null;
};

/**
 * Searches every source for artwork for this track.
 *
 * Their catalogues barely overlap, so a record missing from one is often in
 * another. A source that fails is simply absent from the results.
 */
export function findCovers(fileId: number): Promise<CoverCandidate[]> {
  return invoke<CoverCandidate[]>("find_covers", { fileId });
}

/** Downloads a chosen cover and embeds it in every selected file. */
export function applyCover(fileIds: number[], url: string): Promise<WriteReport> {
  return invoke<WriteReport>("apply_cover", { fileIds, url });
}

/**
 * Strips the artwork out of every selected file.
 *
 * Every picture, not only the front cover: a file showing the wrong sleeve
 * usually carries two, and removing one makes the other appear.
 */
export function removeCover(fileIds: number[]): Promise<WriteReport> {
  return invoke<WriteReport>("remove_cover", { fileIds });
}

/** One candidate match for a track, from any source. */
export type TrackMatch = {
  source: string;
  title: string;
  artists: string[];
  album: string | null;
  albumArtist: string | null;
  label: string | null;
  catalogNumber: string | null;
  releaseDate: string | null;
  genres: string[];
  bpm: number | null;
  key: string | null;
  isrc: string | null;
  durationMs: number | null;
  trackNo: number | null;
  url: string | null;
  art: string | null;
  /** How well it matched what was searched for, 0..1. */
  score: number;
};

/**
 * What the matches were searched with, when it was not the file's tags: the
 * AcoustID fingerprint ("sound"), Shazam ("shazam") or a guess from the file
 * name ("name").
 */
export type Query = { by: "sound" | "shazam" | "name"; artist: string; title: string };

export type Matched = { matches: TrackMatch[]; query: Query | null };

/**
 * Searches every source for what this file might be.
 *
 * Tries the file's tags, then its fingerprint (AcoustID, then Shazam when on),
 * then its name, until one finds something — the files that most need this are the ones with no tags.
 */
export function matchTrack(fileId: number): Promise<Matched> {
  return invoke<Matched>("match_track", { fileId });
}

/**
 * Which optional credentials are set, and whether Shazam is on. Never carries
 * the values themselves.
 */
export type Credentials = { discogs: boolean; tidal: boolean; shazam: boolean };

export function credentials(): Promise<Credentials> {
  return invoke<Credentials>("credentials");
}

/**
 * Stores or clears the Discogs personal access token.
 *
 * An empty string clears it. The token is never read back out — the UI can ask
 * whether one is set, not what it is.
 */
export function setDiscogsToken(token: string): Promise<Credentials> {
  return invoke<Credentials>("set_discogs_token", { token });
}

/** Turns Shazam lookups in Find metadata on or off. */
export function setShazam(on: boolean): Promise<Credentials> {
  return invoke<Credentials>("set_shazam", { on });
}

/** Stores the Tidal app's client ID and secret. Both empty clears them. */
export function setTidal(clientId: string, secret: string): Promise<Credentials> {
  return invoke<Credentials>("set_tidal", { clientId, secret });
}

/** Reads the quick-tag fields currently set on one track. */
export function quickFields(
  fileId: number,
  names: string[],
): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("quick_fields", { fileId, names });
}

/** One file's proposed rename. */
export type RenamePreview = {
  fileId: number;
  from: string;
  to: string;
  /** Why it will be skipped, or empty when it will go ahead. */
  problem: string;
  unchanged: boolean;
};

/** Fields a rename template can refer to. */
export const RENAME_FIELDS: [string, string][] = [
  ["artist", "Track artist"],
  ["title", "Track title"],
  ["album", "Album"],
  ["albumartist", "Album artist"],
  ["genre", "Genre"],
  ["year", "Year"],
  ["track", "Track number"],
  ["track2", "Track number, two digits"],
  ["bpm", "BPM"],
  ["key", "Musical key"],
  ["label", "Label, from the comment"],
  ["filename", "Current name, without extension"],
];

/** Works out what the selection would be called. Changes nothing. */
export function previewRename(
  fileIds: number[],
  template: string,
): Promise<RenamePreview[]> {
  return invoke<RenamePreview[]>("preview_rename", { fileIds, template });
}

/** Applies exactly the names the preview showed. */
export function applyRename(renames: [number, string][]): Promise<WriteReport> {
  return invoke<WriteReport>("apply_rename", { renames });
}

/**
 * Whether tag writes put the file's modified date back afterwards.
 *
 * Stored beside the app rather than in the browser, because the writes that
 * obey it happen in Rust.
 */
export function preserveDates(): Promise<boolean> {
  return invoke<boolean>("preserve_dates");
}

export function setPreserveDates(keep: boolean): Promise<boolean> {
  return invoke<boolean>("set_preserve_dates", { keep });
}

/**
 * The spectrogram of a track: `columns * 128` bytes, column by column.
 *
 * Empty when there is nothing to draw. Measuring one is a full decode, so this
 * is cached against the file's content and only ever asked for on demand.
 */
export function spectrogram(fileId: number): Promise<Uint8Array> {
  return invoke<number[]>("spectrogram", { fileId }).then(
    (bytes) => new Uint8Array(bytes),
  );
}

/** What one file turned out to be. */
export type QualityRow = {
  fileId: number;
  name: string;
  /** genuine · upscaled · transcoded · unsure · unreadable */
  verdict: string;
  claimedKbps: number | null;
  impliedKbps: number | null;
  cutoffKhz: number;
  sampleRate: number;
  lossless: boolean;
  /** Portion of samples in a flat top, as a percentage. */
  clipPercent: number;
  /** Longest unbroken run of flat-topped samples. */
  clipRun: number;
  /** Where the flat tops sit, dBFS: 0 at full scale, below when turned down after clipping. */
  clipDb: number;
  /** Integrated loudness, LUFS. */
  lufs: number | null;
  /** True peak, dBTP. */
  truePeak: number | null;
  /** The true peak is at least this: a lossy decode clipped at full scale. */
  peakFloor: boolean;
  /** What is broken, as opposed to lossy: `0:30 missing · 2 dropouts`. Empty when sound. */
  damage: string;
  reason: string;
};

/**
 * Measures what a batch of files really is.
 *
 * A batch rather than a selection: analysis decodes each track and costs about
 * half a second, so the caller walks its selection a few at a time and shows
 * progress. One command for a whole folder would be a command that appears to
 * hang.
 */
export function analyseQuality(fileIds: number[]): Promise<QualityRow[]> {
  return invoke<QualityRow[]>("analyse_quality", { fileIds });
}

/** Writes ReplayGain track gain (to -18 LUFS) and peak from what was measured. */
export function writeReplayGain(fileIds: number[]): Promise<WriteReport> {
  return invoke<WriteReport>("write_replay_gain", { fileIds });
}

/** What one file measured at, beside what it is tagged with. Keys are Camelot. */
export type AnalysisRow = {
  fileId: number;
  name: string;
  taggedBpm: number | null;
  /** Rounded as it would be written: whole, or to one decimal. */
  bpm: number | null;
  taggedKey: string | null;
  key: string | null;
  /** The model's probability for `key`, 0..1. */
  keyConfidence: number | null;
  error: string | null;
};

/**
 * Measures tempo (within `min`..`max` BPM) and key for a batch of files, one
 * decode each.
 *
 * About a second a track, so the caller walks its selection a few at a time.
 * Writes nothing: writing is `writeTags`, per file, for the ones the user ticks.
 */
export function analyseTracks(fileIds: number[], min: number, max: number): Promise<AnalysisRow[]> {
  return invoke<AnalysisRow[]>("analyse_tracks", { fileIds, min, max });
}

/** What background analysis does with a measurement. */
export type AnalysisMode = "fill" | "show" | "overwrite";

export type AnalysisSettings = {
  enabled: boolean;
  mode: AnalysisMode;
  /** The tempo range that settles half and double time. */
  min: number;
  max: number;
};

export function analysisSettings(): Promise<AnalysisSettings> {
  return invoke<AnalysisSettings>("analysis_settings");
}

export function setAnalysisSettings(settings: AnalysisSettings): Promise<AnalysisSettings> {
  return invoke<AnalysisSettings>("set_analysis_settings", { settings });
}

/** Files waiting for background analysis, including the ones being measured. */
export function analysisStatus(): Promise<{ pending: number }> {
  return invoke<{ pending: number }>("analysis_status");
}

/** One file in a set of copies. */
export type DuplicateCopy = {
  fileId: number;
  name: string;
  folder: string;
  size: string;
  modified: string;
};

/** A set of files holding identical audio. */
export type DuplicateGroup = {
  hash: string;
  files: DuplicateCopy[];
  /** What keeping one of them would give back. */
  wasted: string;
};

/**
 * Measures what a batch of files is, and remembers it.
 *
 * A full decode each, so the caller walks its selection a few at a time. Files
 * already measured are skipped — their bytes cannot have changed without
 * becoming different content.
 */
export function identify(fileIds: number[]): Promise<number> {
  return invoke<number>("identify", { fileIds });
}

/**
 * Every set of measured files that hold the same audio.
 *
 * Files nobody has measured are absent rather than reported as unique: not
 * knowing and knowing there is no copy are different answers.
 */
export function duplicates(scopes: string[]): Promise<DuplicateGroup[]> {
  return invoke<DuplicateGroup[]>("duplicates", { scopes });
}

/** One value and how many tracks carry it. */
export type Tally = { value: string; count: number };

/** A folder and what is wrong in it. */
export type ProblemFolder = {
  path: string;
  tracks: number;
  damaged: number;
  unreadable: number;
};

/** Statistics over the indexed library. */
export type Overview = {
  tracks: number;
  folders: number;
  seconds: number;
  bytes: number;
  /** `[bpm, tracks]`, ascending. */
  bpm: [number, number][];
  /** Tracks per key, indexed by the canonical key value (0–23). */
  keys: number[];
  formats: Tally[];
  /** `"44100/16"`, or `"44100/"` where the depth is unknown. */
  sampleRates: Tally[];
  /** `[whole LUFS, tracks]`, ascending. */
  lufs: [number, number][];
  checked: number;
  damaged: number;
  unreadable: number;
  noArtist: number;
  noTitle: number;
  noGenre: number;
  noBpm: number;
  noKey: number;
  noArt: number;
  labels: Tally[];
  genres: Tally[];
  artists: Tally[];
  problemFolders: ProblemFolder[];
};

/** The library at a glance, or one folder and everything under it. */
export function overview(scope: string | null): Promise<Overview> {
  return invoke<Overview>("overview", { scope });
}

/** What a user-defined action does. */
export type ActionKind = "tag" | "rename" | "open" | "url" | "copy" | "move";

/** One user-defined button. */
export type Action = {
  id: string;
  label: string;
  kind: ActionKind;
  /** Tag field, program path, or destination folder, depending on the kind. */
  target: string;
  /** The title-formatting expression, run once per file. */
  script: string;
  /**
   * Which recipe built the expression, or `custom` for one written by hand.
   *
   * Stored so reopening the editor shows the recipe rather than only its
   * output — an expression cannot be read backwards into the choice that made
   * it, and asking the user to pick again every time would defeat the point.
   */
  recipe: string;
  /** The recipe's own inputs, in the order it asks for them. */
  slots: string[];
  /** Name from `ACTION_ICONS`, or empty. A button with an icon is also in the toolbar. */
  icon?: string;
};

/** What an action would do to one file. */
export type ActionPreview = {
  fileId: number;
  name: string;
  result: string;
  problem: string;
};

/** Whether a kind changes files, and so is worth previewing first. */
export function actionWrites(kind: ActionKind): boolean {
  return kind !== "open" && kind !== "url";
}

/** The actions the user has defined. Stored beside the app, never in the repo. */
export function actions(): Promise<Action[]> {
  return invoke<Action[]>("actions");
}

/**
 * Replaces the whole list.
 *
 * Refused as a whole if any expression will not parse, so a typo is reported
 * while it is being written rather than after it has renamed a folder.
 */
export function setActions(list: Action[]): Promise<Action[]> {
  return invoke<Action[]>("set_actions", { actions: list });
}

/** Works out what an action would do. Changes nothing. */
export function previewAction(
  action: Action,
  fileIds: number[],
): Promise<ActionPreview[]> {
  return invoke<ActionPreview[]>("preview_action", { action, fileIds });
}

/** Runs an action over a selection. */
export function runAction(
  action: Action,
  fileIds: number[],
): Promise<WriteReport> {
  return invoke<WriteReport>("run_action", { action, fileIds });
}

/** One tag field a cleanup would change. */
export type CleanField = {
  field: string;
  label: string;
  from: string;
  to: string;
};

/** One file's proposed cleanup. */
export type CleanPreview = {
  fileId: number;
  from: string;
  to: string;
  fields: CleanField[];
  problem: string;
  unchanged: boolean;
};

/** One file's cleanup, as it goes back to be applied. */
export type CleanApply = {
  fileId: number;
  /** Empty to leave the file name alone. */
  name: string;
  fields: [string, string][];
};

/** Applies exactly what the preview showed. */
export function applyCleanup(items: CleanApply[]): Promise<WriteReport> {
  return invoke<WriteReport>("apply_cleanup", { items });
}

// ── Tag tools ──────────────────────────────────────────────────────────────

/** A built-in one-click tool. Names match `tools::Tool` in the back end. */
export type ToolId =
  | "clean-name"
  | "album-artist"
  | "artist-into-title"
  | "artist-out-of-title"
  | "remove-original"
  | "trailing-brackets"
  | "year-only"
  | "clean-tags"
  | "tidy-feat"
  | "cut-start"
  | "cut-end";

/**
 * Works out what the tools, run in order, would do. Changes nothing; the result is applied with
 * `applyCleanup`, exactly as a cleanup is.
 */
export function previewTool(
  tools: ToolId[],
  count: number,
  fileIds: number[],
): Promise<CleanPreview[]> {
  return invoke<CleanPreview[]>("preview_tool", { tools, count, fileIds });
}

// ── Converter ──────────────────────────────────────────────────────────────

export type ConvertFormat = "wav" | "aiff" | "flac" | "alac";
export type ConvertDepth = "source" | "16" | "24";

export type ConvertOptions = {
  format: ConvertFormat;
  depth: ConvertDepth;
  /** Empty for beside each source. */
  folder: string;
  replace: boolean;
  verify: boolean;
};

/** Which ffmpeg the converter uses. `path` is empty when none was found. */
export type FfmpegInfo = { path: string; version: string; configured: string };

export type ConvertPlanned = {
  fileId: number;
  name: string;
  from: string;
  to: string;
  output: string;
  /** Whether the output will hold exactly the source's samples. */
  exact: boolean;
  /** Why it will be skipped; empty when it will not. */
  problem: string;
};

export type Converted = {
  fileId: number;
  output: string;
  ok: boolean;
  /** True when the audio was compared with the source and matched. */
  verified: boolean | null;
  message: string;
};

export function ffmpegInfo(): Promise<FfmpegInfo> {
  return invoke<FfmpegInfo>("ffmpeg_info");
}

export function setFfmpegPath(path: string): Promise<FfmpegInfo> {
  return invoke<FfmpegInfo>("set_ffmpeg_path", { path });
}

/** Where Send to rekordbox writes. `set` is the Settings path, empty to use `found`. */
export type RekordboxTarget = { path: string | null; set: string; found: string | null };

export type RekordboxSent = { playlists: number; added: number; missing: number; path: string };

export function rekordboxTarget(): Promise<RekordboxTarget> {
  return invoke<RekordboxTarget>("rekordbox_target");
}

export function setRekordboxXml(path: string): Promise<RekordboxTarget> {
  return invoke<RekordboxTarget>("set_rekordbox_xml", { path });
}

/** Writes every playlist into rekordbox.xml's `noceur` folder. */
export function sendToRekordbox(): Promise<RekordboxSent> {
  return invoke<RekordboxSent>("send_to_rekordbox");
}

export function convertPlan(fileIds: number[], options: ConvertOptions): Promise<ConvertPlanned[]> {
  return invoke<ConvertPlanned[]>("convert_plan", { fileIds, options });
}

export function convertFile(fileId: number, options: ConvertOptions): Promise<Converted> {
  return invoke<Converted>("convert_file", { fileId, options });
}

/**
 * Makes a folder in `parent` and returns its path. An empty name makes
 * "New folder", numbered past any that exist.
 */
export function createFolder(parent: string, name: string): Promise<string> {
  return invoke<string>("create_folder", { parent, name });
}

// ── Tags from file name ────────────────────────────────────────────────────

export type FromNameRow = {
  fileId: number;
  /** What the pattern was laid over: the name, plus a folder per `\`. */
  name: string;
  /** `[label, value]` for each field that will be written. */
  fields: [string, string][];
  /** Why nothing will be written; empty when something will. */
  problem: string;
};

export function previewFromName(fileIds: number[], pattern: string): Promise<FromNameRow[]> {
  return invoke<FromNameRow[]>("preview_from_name", { fileIds, pattern });
}

export function applyFromName(fileIds: number[], pattern: string): Promise<WriteReport> {
  return invoke<WriteReport>("apply_from_name", { fileIds, pattern });
}
