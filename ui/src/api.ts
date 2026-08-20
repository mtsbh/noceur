import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type RowDisplay = {
  fileId: number;
  kind: number;
  name: string;
  bpm: string;
  key: string;
  duration: string;
  artist: string;
  title: string;
  album: string;
  size: string;
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
  /** The window at offset 0, so the first screen paints without a round trip. */
  first: RowWindow;
  scanMs: number;
  probeMs: number;
};

/** Scans, probes and counts a directory, returning its first window of rows. */
export function openDir(path: string, audioOnly = false): Promise<DirListing> {
  return invoke<DirListing>("open_dir", { req: { path, audioOnly } });
}

/**
 * Reads one window of an already-opened directory.
 *
 * `audioOnly` must match the value `openDir` was called with — the backend cuts
 * the window from the filtered list, so a mismatch silently returns rows from a
 * different list than the one on screen.
 */
export function listRows(
  dirId: number,
  offset: number,
  limit: number,
  audioOnly = false,
): Promise<RowWindow> {
  return invoke<RowWindow>("list_rows", {
    req: { dirId, offset, limit, audioOnly },
  });
}

export function parentOf(path: string): Promise<string | null> {
  return invoke<string | null>("parent_of", { path });
}
