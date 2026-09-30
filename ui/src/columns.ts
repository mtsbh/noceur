import type { RowDisplay, SortColumn } from "./api";

/**
 * A column of the list.
 *
 * Width is in pixels rather than fractions: a column the user dragged to a width
 * should stay that width when the window resizes, which is what every file
 * manager does and what fractions cannot do.
 */
export type Column = {
  key: SortColumn;
  label: string;
  width: number;
  /** Right-aligned, tabular — numbers and durations. */
  num?: boolean;
  /** Rendered in the dimmer secondary colour. */
  dim?: boolean;
  visible: boolean;
};

/** Narrower than this and a column is a sliver with no room for its own label. */
export const MIN_COL = 44;

/**
 * The default layout.
 *
 * Order here is the order on screen until the user drags a header elsewhere.
 * Title, album and date start hidden: they are useful but not for everyone, and
 * six columns is already a full row at the small art size.
 */
export const DEFAULT_COLUMNS: Column[] = [
  { key: "name", label: "Name", width: 420, visible: true },
  { key: "bpm", label: "BPM", width: 62, num: true, visible: true },
  { key: "key", label: "Key", width: 52, visible: true },
  // Wide enough for the playing row's "1:43 / 5:50".
  { key: "duration", label: "Time", width: 96, num: true, visible: true },
  { key: "artist", label: "Artist", width: 180, dim: true, visible: true },
  { key: "title", label: "Title", width: 180, dim: true, visible: false },
  { key: "album", label: "Album", width: 180, dim: true, visible: false },
  { key: "albumArtist", label: "Album artist", width: 160, dim: true, visible: false },
  { key: "genre", label: "Genre", width: 120, dim: true, visible: false },
  { key: "year", label: "Year", width: 56, num: true, visible: false },
  { key: "trackNo", label: "Track", width: 52, num: true, visible: false },
  { key: "comment", label: "Comment", width: 200, dim: true, visible: false },
  { key: "isrc", label: "ISRC", width: 116, dim: true, visible: false },
  { key: "sampleRate", label: "Rate", width: 74, num: true, visible: false },
  { key: "bitDepth", label: "Depth", width: 62, num: true, visible: false },
  { key: "codec", label: "Codec", width: 78, dim: true, visible: false },
  { key: "rating", label: "Rating", width: 80, visible: true },
  { key: "size", label: "Size", width: 88, num: true, visible: true },
  { key: "modified", label: "Date", width: 120, num: true, visible: false },
];

const KEY = "noceur.columns";
/**
 * Set once a saved layout's Time column has been widened to the new default.
 * Layouts saved before the playing row showed elapsed time had it at 62 px,
 * too narrow for "1:43 / 5:50"; after the one widening, the user's width wins.
 */
const TIME_WIDENED = "noceur.columns.timeWidened";

/**
 * Reads the saved layout, reconciled against the columns this build knows.
 *
 * Saved entries supply order, width and visibility; anything the build has that
 * the save does not is appended with its default. That way adding a column in a
 * later version does not discard a layout the user arranged, and removing one
 * does not leave a phantom in the grid.
 */
export function loadColumns(): Column[] {
  let saved: Partial<Column>[] = [];
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) saved = parsed;
    }
  } catch {
    // A corrupt layout is not worth failing over; fall through to defaults.
  }

  const known = new Map(DEFAULT_COLUMNS.map((c) => [c.key, c]));
  const out: Column[] = [];

  for (const entry of saved) {
    const base = entry.key && known.get(entry.key as SortColumn);
    if (!base || out.some((c) => c.key === base.key)) continue;
    out.push({
      ...base,
      width: clampWidth(Number(entry.width) || base.width),
      visible: entry.visible ?? base.visible,
    });
  }

  for (const base of DEFAULT_COLUMNS) {
    if (!out.some((c) => c.key === base.key)) out.push({ ...base });
  }

  try {
    if (saved.length > 0 && !localStorage.getItem(TIME_WIDENED)) {
      const time = out.find((c) => c.key === "duration");
      const wide = known.get("duration")!.width;
      if (time && time.width < wide) {
        time.width = wide;
        saveColumns(out);
      }
    }
    localStorage.setItem(TIME_WIDENED, "1");
  } catch {
    // Storage unavailable: the column keeps its saved width.
  }

  return out;
}

export function saveColumns(columns: Column[]): void {
  localStorage.setItem(
    KEY,
    JSON.stringify(
      columns.map((c) => ({ key: c.key, width: c.width, visible: c.visible })),
    ),
  );
}

export function clampWidth(px: number): number {
  return Math.max(MIN_COL, Math.round(px));
}

/**
 * The `grid-template-columns` both the header and the rows use.
 *
 * A trailing `1fr` soaks up whatever is left, so narrow layouts do not leave the
 * last column stretched across the window and wide ones do not push a ragged
 * edge past it.
 */
export function gridTemplate(columns: Column[]): string {
  const widths = columns
    .filter((c) => c.visible)
    .map((c) => `${c.width}px`)
    .join(" ");
  return `var(--art) ${widths} 1fr`;
}

/** The text a column shows for a row. */
export function cellText(row: RowDisplay, key: SortColumn): string {
  switch (key) {
    case "name":
      return row.name;
    case "bpm":
      return row.bpm;
    case "key":
      return row.key;
    case "duration":
      return row.duration;
    case "artist":
      return row.artist;
    case "title":
      return row.title;
    case "album":
      return row.album;
    case "albumArtist":
      return row.albumArtist;
    case "genre":
      return row.genre;
    case "year":
      return row.year;
    case "trackNo":
      return row.trackNo;
    case "comment":
      return row.comment;
    case "isrc":
      return row.isrc;
    case "sampleRate":
      return row.sampleRate;
    case "bitDepth":
      return row.bitDepth;
    case "codec":
      return row.codec;
    case "rating":
      return row.rating ? `${row.rating} of 5 stars` : row.kind === 1 ? "Unrated" : "";
    case "size":
      return row.size;
    case "modified":
      return row.modified;
  }
}

/**
 * Colour for a key, taken from its position on the Camelot wheel.
 *
 * The wheel is a colour wheel: adjacent numbers are adjacent hues, and adjacent
 * numbers are exactly the keys that mix. Colouring by position turns "find a
 * compatible track" into "find a similar colour", which is the whole reason the
 * wheel is drawn in colour in the first place.
 *
 * Hue runs backwards from green at 1 through yellow, red, purple and blue,
 * matching the standard wheel. The A ring (minor) is the pastel inner ring, so
 * it renders less saturated than the B ring (major) — same as the printed one.
 *
 * `canonical` is `(camelot - 1) * 2 + (minor ? 0 : 1)`, per `MusicalKey`.
 */
export function keyColour(canonical: number | null): string | undefined {
  if (canonical === null || canonical < 0 || canonical > 23) return undefined;

  const number = Math.floor(canonical / 2) + 1;
  const minor = canonical % 2 === 0;
  const hue = (160 - (number - 1) * 30 + 360) % 360;

  // Lightness comes from the theme, so the wheel stays readable on a light one.
  return minor
    ? `hsl(${hue} 42% var(--key-l, 66%))`
    : `hsl(${hue} 64% var(--key-l, 66%))`;
}

/** Moves a column to a new position, returning a new array. */
export function reorder(columns: Column[], from: number, to: number): Column[] {
  if (from === to || from < 0 || to < 0) return columns;
  const next = [...columns];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}
