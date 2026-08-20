import { createSignal, createResource, createEffect, on, For, Show } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { createVirtualizer } from "@tanstack/solid-virtual";
import {
  openDir,
  listRows,
  parentOf,
  thumbUrl,
  type ArtSize,
  type DirListing,
  type RowDisplay,
} from "./api";

/** Where the browser opens. Adjust freely — pinned folders arrive in a later milestone. */
const HOME = "E:\\music";

/**
 * Cover-art edge length and the row height that holds it, per size.
 *
 * `art` must match `ThumbSize::px()` in `noceur-art`: the backend renders at
 * exactly these edges, and asking for a different one in CSS would scale an
 * image that was rasterised precisely to avoid being scaled.
 *
 * Small keeps the 26px row milestone 3 was built around, so choosing the compact
 * size costs nothing against the list as it was.
 */
const ART: Record<ArtSize, { art: number; row: number }> = {
  small: { art: 22, row: 26 },
  medium: { art: 44, row: 48 },
  large: { art: 88, row: 92 },
};

const ART_SIZES: ArtSize[] = ["small", "medium", "large"];

const ART_KEY = "noceur.artSize";

function storedArtSize(): ArtSize {
  const v = localStorage.getItem(ART_KEY);
  return v === "medium" || v === "large" || v === "small" ? v : "small";
}

/**
 * Rows per window.
 *
 * Must equal `FIRST_WINDOW` in `commands.rs`: the window that arrives with a
 * listing is cached verbatim as page 0, so a mismatch would file 200 rows into a
 * 100-row page and lose half of them.
 */
const PAGE = 200;

/** Rows realised beyond the viewport, so a fast scroll shows rows rather than gaps. */
const OVERSCAN = 12;

/**
 * Row height is now owned by JavaScript rather than by `--row-h` in the
 * stylesheet.
 *
 * Milestone 3 had CSS as the source of truth and read the value back, because
 * the height was a constant only the virtualizer also needed to know. Now the
 * user changes it at runtime, so the signal has to be authoritative and the
 * stylesheet is fed from it.
 */
function metrics(size: ArtSize) {
  return ART[size];
}

export default function App() {
  // Back/forward history for this pane. Index points at the current entry.
  const [history, setHistory] = createSignal<string[]>([HOME]);
  const [cursor, setCursor] = createSignal(0);
  const [selected, setSelected] = createSignal<number | null>(null);
  const [windowError, setWindowError] = createSignal("");
  const [artSize, setArtSize] = createSignal<ArtSize>(storedArtSize());

  function chooseArtSize(size: ArtSize) {
    setArtSize(size);
    localStorage.setItem(ART_KEY, size);
  }

  const current = () => history()[cursor()];
  // The fetcher must be wrapped: Solid calls it as `fetcher(source, info)`, and
  // passing `openDir` directly would hand the info object to its `audioOnly`
  // parameter, silently filtering out every directory.
  const [listing] = createResource<DirListing, string>(current, (p) => openDir(p));

  /** Row count of the whole listing — what the virtualizer sizes itself against. */
  const total = () => (listing.error ? 0 : (listing()?.total ?? 0));

  // Rows the frontend actually holds, keyed by page index. A store rather than a
  // signal so an arriving window re-renders its own rows and not the entire list.
  const [pages, setPages] = createStore<Record<number, RowDisplay[]>>({});
  // Pages already asked for. Deliberately not reactive: it exists only to stop
  // duplicate requests, and nothing is rendered from it.
  let inflight = new Set<number>();

  let scrollEl: HTMLDivElement | undefined;

  const virtualizer = createVirtualizer({
    get count() {
      return total();
    },
    getScrollElement: () => scrollEl ?? null,
    estimateSize: () => metrics(artSize()).row,
    overscan: OVERSCAN,
  });

  // Changing the art size changes every row's height at once. The virtualizer
  // caches what it has measured, so it has to be told the old numbers are void —
  // without this the rows keep their previous spacing and overlap or gap.
  createEffect(
    on(
      artSize,
      () => virtualizer.measure(),
      { defer: true },
    ),
  );

  // A new listing invalidates every cached window. Drop them, seed page 0 from
  // the window that arrived with the listing, and go back to the top — otherwise
  // a folder opened at scroll offset 12,000 would start out of range.
  createEffect(
    on(listing, (l) => {
      if (!l) return;
      setPages(reconcile({}));
      inflight = new Set();
      setWindowError("");
      if (l.first.rows.length > 0) setPages(0, l.first.rows);
      if (scrollEl) scrollEl.scrollTop = 0;
    }),
  );

  /** The row at a listing-wide index, or `undefined` while its window is in flight. */
  function rowAt(index: number): RowDisplay | undefined {
    return pages[Math.floor(index / PAGE)]?.[index % PAGE];
  }

  function request(page: number, l: DirListing) {
    if (pages[page] !== undefined || inflight.has(page)) return;
    inflight.add(page);
    const dirId = l.dirId;

    listRows(dirId, page * PAGE, PAGE)
      .then((w) => {
        // A window that lands after we have navigated belongs to a listing that
        // is no longer on screen; filing it would show another folder's rows.
        if (listing()?.dirId !== dirId) return;
        setPages(Math.floor(w.offset / PAGE), w.rows);
        setWindowError("");
      })
      .catch((e) => setWindowError(String(e)))
      // Dropped from the in-flight set even on failure, so scrolling past the
      // gap again retries rather than leaving those rows blank forever.
      .finally(() => inflight.delete(page));
  }

  // Fetch whatever the visible range needs. Re-runs when the range moves and
  // when a window arrives; a page already held or already requested is skipped,
  // so settling on one screen issues no further calls.
  //
  // Declared after the reset effect above, and must stay there: Solid runs
  // effects in creation order, so on a navigation the cache is emptied before
  // this one looks at it. Reversed, it would find the previous folder's pages
  // still cached, consider the range satisfied, and render those rows.
  createEffect(() => {
    const l = listing.error ? undefined : listing();
    if (!l) return;
    const items = virtualizer.getVirtualItems();
    if (items.length === 0) return;

    const from = Math.floor(items[0].index / PAGE);
    const to = Math.floor(items[items.length - 1].index / PAGE);
    for (let p = from; p <= to; p++) request(p, l);
  });

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

  function rowClass(row: RowDisplay | undefined): string {
    if (!row) return "row pending";
    const kind = row.kind === 0 ? " dir" : row.kind === 2 ? " other" : "";
    return "row" + kind + (selected() === row.fileId ? " sel" : "");
  }

  return (
    <div
      class="app"
      style={{
        "--row-h": `${metrics(artSize()).row}px`,
        "--art": `${metrics(artSize()).art}px`,
      }}
    >
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
        <Show when={!listing.error && listing()}>
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

        <span class="grow" />
        <div class="sizes" role="group" aria-label="Cover art size">
          <For each={ART_SIZES}>
            {(size) => (
              <button
                class={`sizebtn ${artSize() === size ? "on" : ""}`}
                title={`${size[0].toUpperCase()}${size.slice(1)} cover art`}
                onClick={() => chooseArtSize(size)}
              >
                {size[0].toUpperCase()}
              </button>
            )}
          </For>
        </div>
      </div>

      <div class="hdr">
        <span class="art" />
        <span>Name</span>
        <span class="num">BPM</span>
        <span>Key</span>
        <span class="num">Time</span>
        <span>Artist</span>
        <span class="num">Size</span>
      </div>

      {/* The scroll container is the virtualizer's measuring stick, so it is
          always mounted — the loading and empty states render inside it. */}
      <div class="list" ref={(el) => (scrollEl = el)}>
        <Show
          when={!listing.error}
          fallback={<div class="empty">Could not read {current()}.</div>}
        >
          <Show when={!listing.loading} fallback={<div class="empty">Reading {current()}…</div>}>
            <Show when={total() > 0} fallback={<div class="empty">Nothing here.</div>}>
              {/* One tall spacer holds the scrollbar honest; only the rows in
                  view exist inside it. */}
              <div class="canvas" style={{ height: `${virtualizer.getTotalSize()}px` }}>
                <For each={virtualizer.getVirtualItems()}>
                  {(item) => {
                    const row = () => rowAt(item.index);
                    return (
                      <div
                        class={rowClass(row())}
                        style={{
                          height: `${item.size}px`,
                          transform: `translateY(${item.start}px)`,
                        }}
                        onClick={() => {
                          const r = row();
                          if (r) onRow(r);
                        }}
                      >
                        <Show when={row()} fallback={<span class="ph" />}>
                          {(r) => (
                            <>
                              {/* Only rows the probe found a picture in get an
                                  <img> at all; the rest render the empty cell
                                  directly rather than firing a request that can
                                  only 404. On error the image hides itself and
                                  the cell's own background shows through, which
                                  covers art that turns out not to decode. */}
                              <span class="art">
                                <Show when={r().hasArt}>
                                  <img
                                    src={thumbUrl(r().fileId, artSize())}
                                    alt=""
                                    loading="lazy"
                                    decoding="async"
                                    draggable={false}
                                    onLoad={(e) => e.currentTarget.classList.remove("blank")}
                                    onError={(e) => e.currentTarget.classList.add("blank")}
                                  />
                                </Show>
                              </span>
                              <span class="name" title={r().name}>
                                {r().name}
                                <Show when={r().state}>
                                  <span class="bad"> [{r().state}]</span>
                                </Show>
                              </span>
                              <span class="num mono">{r().bpm}</span>
                              <span class="mono">{r().key}</span>
                              <span class="num mono">{r().duration}</span>
                              <span class="dim">{r().artist}</span>
                              <span class="num mono">{r().size}</span>
                            </>
                          )}
                        </Show>
                      </div>
                    );
                  }}
                </For>
              </div>
            </Show>
          </Show>
        </Show>
      </div>

      <div class="status">
        <Show when={!listing.error && listing()}>
          <span>{listing()!.total} entries</span>
          <span class={listing()!.scanMs > 500 ? "slow" : ""}>
            scan {listing()!.scanMs}ms
          </span>
          <span class={listing()!.probeMs > 2000 ? "slow" : ""}>
            probe {listing()!.probeMs}ms
          </span>
        </Show>
        <Show when={windowError()}>
          <span class="slow">{windowError()}</span>
        </Show>
      </div>
    </div>
  );
}
