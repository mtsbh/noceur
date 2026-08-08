import { invoke } from "@tauri-apps/api/core";

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
};

export type Segment = { label: string; path: string };

export type DirListing = {
  path: string;
  segments: Segment[];
  rows: RowDisplay[];
  total: number;
  scanMs: number;
  probeMs: number;
};

export function listDir(path: string, audioOnly = false): Promise<DirListing> {
  return invoke<DirListing>("list_dir", { req: { path, audioOnly } });
}

export function parentOf(path: string): Promise<string | null> {
  return invoke<string | null>("parent_of", { path });
}
