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
  size: string;
  /** `YYYY-MM-DD HH:MM` in UTC — when the file changed, not when it was released. */
  modified: string;
  state: string;
  /** True only for audio files the last probe found a picture in. */
  hasArt: boolean;
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

/** Opens a link in the default browser. Only http and https are accepted. */
export function openUrl(url: string): Promise<void> {
  return invoke<void>("open_url", { url });
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
  year: string | null;
  trackNo: string | null;
  bpm: string | null;
  key: string | null;
};

/** The editable fields, in the order the editor lays them out. */
export const TAG_FIELDS = [
  ["title", "Title"],
  ["artist", "Artist"],
  ["album", "Album"],
  ["albumArtist", "Album artist"],
  ["genre", "Genre"],
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
): Promise<WriteReport> {
  return invoke<WriteReport>("write_tags", {
    fileIds,
    edit: { ...edit, custom, rating },
  });
}

/**
 * The key a rating travels under in the quick-tag map.
 *
 * Not a tag name — ratings live in a frame of their own in every format. Matches
 * `RATING_FIELD` on the other side.
 */
export const RATING_FIELD = "noceur:rating";

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
 * Searches every source for what this file might be.
 *
 * Uses the file's tags, falling back to its name — the files that most need
 * this are the ones with no tags to search by.
 */
export function matchTrack(fileId: number): Promise<TrackMatch[]> {
  return invoke<TrackMatch[]>("match_track", { fileId });
}

/** Which optional credentials are set. Never carries the values themselves. */
export type Credentials = { discogs: boolean };

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
  /** Portion of samples pinned to full scale, as a percentage. */
  clipPercent: number;
  /** Longest unbroken run of full-scale samples. */
  clipRun: number;
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

/** One cleanup rule, as the back end describes it. */
export type CleanRule = { slug: string; label: string; hint: string };

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

/**
 * The rules this build knows about.
 *
 * Fetched rather than listed here, so what is on screen cannot drift from what
 * actually runs.
 */
export function cleanupRules(): Promise<CleanRule[]> {
  return invoke<CleanRule[]>("cleanup_rules");
}

/** Works out what the rules would do to the selection. Changes nothing. */
export function previewCleanup(
  fileIds: number[],
  rules: string[],
  name: boolean,
  tags: boolean,
): Promise<CleanPreview[]> {
  return invoke<CleanPreview[]>("preview_cleanup", {
    fileIds,
    rules,
    name,
    tags,
  });
}

/** Applies exactly what the preview showed. */
export function applyCleanup(items: CleanApply[]): Promise<WriteReport> {
  return invoke<WriteReport>("apply_cleanup", { items });
}
