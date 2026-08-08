import { createSignal, createResource, For, Show } from "solid-js";
import { listDir, parentOf, type DirListing, type RowDisplay } from "./api";

/** Where the browser opens. Adjust freely — pinned folders arrive in a later milestone. */
const HOME = "E:\\music";

export default function App() {
  // Back/forward history for this pane. Index points at the current entry.
  const [history, setHistory] = createSignal<string[]>([HOME]);
  const [cursor, setCursor] = createSignal(0);
  const [selected, setSelected] = createSignal<number | null>(null);

  const current = () => history()[cursor()];
  // The fetcher must be wrapped: Solid calls it as `fetcher(source, info)`, and
  // passing `listDir` directly would hand the info object to its `audioOnly`
  // parameter, silently filtering out every directory.
  const [listing] = createResource<DirListing, string>(current, (p) => listDir(p));

  function navigate(path: string) {
    // Truncate any forward history, exactly like a browser.
    const next = history().slice(0, cursor() + 1);
    next.push(path);
    setHistory(next);
    setCursor(next.length - 1);
    setSelected(null);
  }

  const canBack = () => cursor() > 0;
  const canForward = () => cursor() < history().length - 1;

  function back() {
    if (canBack()) {
      setCursor(cursor() - 1);
      setSelected(null);
    }
  }
  function forward() {
    if (canForward()) {
      setCursor(cursor() + 1);
      setSelected(null);
    }
  }
  async function up() {
    const parent = await parentOf(current());
    if (parent) navigate(parent);
  }

  function onRow(row: RowDisplay) {
    setSelected(row.fileId);
    if (row.kind === 0) {
      const sep = current().endsWith("\\") || current().endsWith("/") ? "" : "\\";
      navigate(`${current()}${sep}${row.name}`);
    }
  }

  return (
    <div class="app">
      <div class="crumbs">
        <button class="navbtn" onClick={back} disabled={!canBack()}>
          ←
        </button>
        <button class="navbtn" onClick={forward} disabled={!canForward()}>
          →
        </button>
        <button class="navbtn" onClick={up}>
          ↑
        </button>
        <Show when={listing()}>
          <For each={listing()!.segments}>
            {(seg, i) => (
              <>
                <Show when={i() > 0}>
                  <span class="sep">›</span>
                </Show>
                <span
                  class={`crumb ${i() === listing()!.segments.length - 1 ? "last" : ""}`}
                  onClick={() => navigate(seg.path)}
                >
                  {seg.label}
                </span>
              </>
            )}
          </For>
        </Show>
      </div>

      <div class="hdr">
        <span>Name</span>
        <span class="num">BPM</span>
        <span>Key</span>
        <span class="num">Time</span>
        <span>Artist</span>
        <span class="num">Size</span>
      </div>

      <div class="list">
        <Show
          when={!listing.loading}
          fallback={<div class="empty">Reading {current()}…</div>}
        >
          <Show
            when={listing() && listing()!.rows.length > 0}
            fallback={<div class="empty">Nothing here.</div>}
          >
            <For each={listing()!.rows}>
              {(row) => (
                <div
                  class={
                    "row" +
                    (row.kind === 0 ? " dir" : row.kind === 2 ? " other" : "") +
                    (selected() === row.fileId ? " sel" : "")
                  }
                  onClick={() => onRow(row)}
                >
                  <span class="name" title={row.name}>
                    {row.name}
                    <Show when={row.state}>
                      <span class="bad"> [{row.state}]</span>
                    </Show>
                  </span>
                  <span class="num mono">{row.bpm}</span>
                  <span class="mono">{row.key}</span>
                  <span class="num mono">{row.duration}</span>
                  <span class="dim">{row.artist}</span>
                  <span class="num mono">{row.size}</span>
                </div>
              )}
            </For>
          </Show>
        </Show>
      </div>

      <div class="status">
        <Show when={listing()}>
          <span>{listing()!.total} entries</span>
          <span class={listing()!.scanMs > 500 ? "slow" : ""}>
            scan {listing()!.scanMs}ms
          </span>
          <span class={listing()!.probeMs > 2000 ? "slow" : ""}>
            probe {listing()!.probeMs}ms
          </span>
        </Show>
      </div>
    </div>
  );
}
