import {
  createSignal,
  createResource,
  createEffect,
  on,
  onCleanup,
  For,
  Show,
} from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { createVirtualizer } from "@tanstack/solid-virtual";
import {
  openDir,
  listRows,
  parentOf,
  thumbUrl,
  probeBatch,
  play,
  transportCmd,
  type Transport as TransportState,
  type ArtSize,
  type Order,
  type SortColumn,
  type DirListing,
  type RowDisplay,
} from "./api";
import Tree, { storedPins } from "./Tree";
import Search from "./Search";
import Transport from "./Transport";
import {
  DEFAULT_COLUMNS,
  cellText,
  clampWidth,
  gridTemplate,
  keyColour,
  loadColumns,
  reorder,
  saveColumns,
  type Column,
} from "./columns";

/**
 * Where the browser opens.
 *
 * The first pinned folder, if there is one — a pin is the user saying "this is
 * where I work", and opening anywhere else ignores it. The fallback only matters
 * before anything has been pinned.
 */
function home(): string {
  return storedPins()[0] ?? "E:\\music";
}

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

const SORT_KEY = "noceur.sort";

function storedOrder(): Order {
  try {
    const raw = localStorage.getItem(SORT_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Order;
      if (DEFAULT_COLUMNS.some((c) => c.key === parsed.sort)) return parsed;
    }
  } catch {
    // A corrupt preference is not worth failing over; fall through to the default.
  }
  return { sort: "name", desc: false };
}

const TREE_KEY = "noceur.treeWidth";
const TREE_MIN = 140;
const TREE_MAX = 700;
const TREE_DEFAULT = 250;

/** Keeps a dragged width usable: never so narrow it vanishes, never so wide the list does. */
function clampTree(px: number): number {
  return Math.min(TREE_MAX, Math.max(TREE_MIN, Math.round(px)));
}

function storedTreeWidth(): number {
  const v = Number(localStorage.getItem(TREE_KEY));
  return Number.isFinite(v) && v > 0 ? clampTree(v) : TREE_DEFAULT;
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
  const [history, setHistory] = createSignal<string[]>([home()]);
  const [cursor, setCursor] = createSignal(0);
  const [selected, setSelected] = createSignal<number | null>(null);
  const [windowError, setWindowError] = createSignal("");
  // Files tag-read so far in this folder; 0 once the folder is fully probed.
  const [probed, setProbed] = createSignal(0);
  const [artSize, setArtSize] = createSignal<ArtSize>(storedArtSize());
  const [treeWidth, setTreeWidth] = createSignal(storedTreeWidth());
  const [order, setOrder] = createSignal<Order>(storedOrder());
  const [columns, setColumns] = createSignal<Column[]>(loadColumns());
  const [pickingColumns, setPickingColumns] = createSignal(false);

  const shownColumns = () => columns().filter((c) => c.visible);

  function commitColumns(next: Column[]) {
    setColumns(next);
    saveColumns(next);
  }

  function toggleColumn(key: SortColumn) {
    commitColumns(
      columns().map((c) => (c.key === key ? { ...c, visible: !c.visible } : c)),
    );
  }

  /**
   * Drags the right edge of a column heading.
   *
   * Same shape as the tree divider: listeners on the window so the drag survives
   * the pointer leaving a 5px strip, and one write to storage at the end rather
   * than on every move.
   */
  function startColumnDrag(e: PointerEvent, key: SortColumn) {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startWidth = columns().find((c) => c.key === key)?.width ?? 100;
    document.body.classList.add("resizing");

    const move = (ev: PointerEvent) =>
      setColumns(
        columns().map((c) =>
          c.key === key
            ? { ...c, width: clampWidth(startWidth + ev.clientX - startX) }
            : c,
        ),
      );

    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      document.body.classList.remove("resizing");
      saveColumns(columns());
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  // Which heading is being dragged, as an index into the visible columns.
  let dragFrom = -1;

  function dropColumn(toKey: SortColumn) {
    const all = columns();
    const from = all.findIndex((c) => c.key === shownColumns()[dragFrom]?.key);
    const to = all.findIndex((c) => c.key === toKey);
    if (from >= 0 && to >= 0) commitColumns(reorder(all, from, to));
    dragFrom = -1;
  }

  /**
   * Clicking a header sorts by it; clicking the one already active reverses it.
   *
   * Directories stay grouped first either way — `desc` reverses the sort column,
   * not the grouping, so reversing by size does not bury the folders at the
   * bottom of the listing.
   */
  function chooseSort(column: SortColumn) {
    const next: Order =
      order().sort === column
        ? { sort: column, desc: !order().desc }
        : { sort: column, desc: false };
    setOrder(next);
    localStorage.setItem(SORT_KEY, JSON.stringify(next));
  }

  function chooseArtSize(size: ArtSize) {
    setArtSize(size);
    localStorage.setItem(ART_KEY, size);
  }

  /**
   * Drags the divider between the tree and the list.
   *
   * Listeners go on the window rather than the handle so the drag survives the
   * pointer outrunning a 5px strip, which it will on any real drag. The width is
   * written to storage once at the end, not on every move.
   */
  function startTreeDrag(e: PointerEvent) {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = treeWidth();
    document.body.classList.add("resizing");

    const move = (ev: PointerEvent) =>
      setTreeWidth(clampTree(startWidth + ev.clientX - startX));

    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      document.body.classList.remove("resizing");
      localStorage.setItem(TREE_KEY, String(treeWidth()));
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  const [finding, setFinding] = createSignal(false);
  const [transport, setTransport] = createSignal<TransportState | null>(null);

  /** Loads a row into the player. Non-audio rows have nothing to play. */
  function playRow(row: RowDisplay) {
    if (row.kind !== 1) return;
    setSelected(row.fileId);
    play(row.fileId).then(setTransport).catch(() => {});
  }

  // Ctrl+F from anywhere. Registered on the window rather than a focusable
  // element so it works without the list having been clicked first.
  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setFinding(true);
        return;
      }

      // Space toggles playback, unless the user is typing — the search box is a
      // text field and a space there is a space, not a transport command.
      const typing =
        e.target instanceof HTMLElement &&
        (e.target.tagName === "INPUT" || e.target.isContentEditable);

      if (e.key === " " && !typing && !finding()) {
        e.preventDefault();
        transportCmd("toggle").then(setTransport).catch(() => {});
      }
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  /**
   * Where search looks: every pinned folder, plus the tree currently open.
   *
   * The current path is included whether or not it is pinned — searching only
   * pinned folders would ignore wherever the user actually is.
   */
  const searchScopes = () => {
    const pins = storedPins();
    const here = current();
    return pins.some((p) => here.toLowerCase().startsWith(p.toLowerCase()))
      ? pins
      : [...pins, here];
  };

  /** Opens a folder from a search result, selecting the file that matched. */
  function revealFromSearch(dirPath: string, fileId: number) {
    navigate(dirPath);
    setSelected(fileId);
  }

  const current = () => history()[cursor()];
  // Keyed on the path only. Changing the sort must not re-open the directory:
  // opening re-enumerates it, and the order is a property of the read, not of
  // the folder. A sort change clears the cached windows instead.
  //
  // The fetcher is wrapped rather than passed directly, because Solid calls it
  // as `fetcher(source, info)` and would hand that info object to openDir's
  // second parameter.
  const [listing] = createResource<DirListing, string>(current, (p) =>
    openDir(p, order()),
  );

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
      setProbed(0);
      if (l.first.rows.length > 0) setPages(0, l.first.rows);
      if (scrollEl) scrollEl.scrollTop = 0;
      void runProbe(l.dirId);
    }),
  );

  /**
   * Re-reads the windows currently on screen.
   *
   * Rows are replaced rather than cleared, so a refresh mid-probe updates the
   * cells that gained a BPM and leaves the rest of the row untouched — clearing
   * first would blink the whole list on every batch.
   */
  function refreshVisible(dirId: number) {
    const items = virtualizer.getVirtualItems();
    if (items.length === 0) return;

    const asked = order();
    const from = Math.floor(items[0].index / PAGE);
    const to = Math.floor(items[items.length - 1].index / PAGE);
    for (let p = from; p <= to; p++) {
      listRows(dirId, p * PAGE, PAGE, asked)
        .then((w) => {
          if (listing()?.dirId !== dirId || order() !== asked) return;
          setPages(Math.floor(w.offset / PAGE), w.rows);
        })
        .catch(() => {});
    }
  }

  // Re-sorting reorders every row, so every cached window is wrong. Deferred so
  // it does not fire on the initial render, which already has the right order.
  createEffect(
    on(
      order,
      () => {
        setPages(reconcile({}));
        inflight = new Set();
        if (scrollEl) scrollEl.scrollTop = 0;
      },
      { defer: true },
    ),
  );

  /**
   * Reads the folder's tags in the background, a batch at a time.
   *
   * The listing is already on screen by now; this is what fills in BPM, key,
   * duration and cover art behind it. Each batch is a separate call so the index
   * is released between them — a folder-long hold is what used to make opening a
   * large folder feel like the app had stopped.
   *
   * Stops as soon as the user navigates elsewhere: the tags of a folder nobody is
   * looking at are not worth the disk.
   */
  async function runProbe(dirId: number) {
    let done = 0;
    for (;;) {
      if (listing()?.dirId !== dirId) return;

      let progress;
      try {
        progress = await probeBatch(dirId);
      } catch {
        return;
      }
      if (listing()?.dirId !== dirId) return;

      if (progress.done) {
        setProbed(0);
        refreshVisible(dirId);
        return;
      }

      done += progress.probed;
      setProbed(done);
      refreshVisible(dirId);
    }
  }

  /** The row at a listing-wide index, or `undefined` while its window is in flight. */
  function rowAt(index: number): RowDisplay | undefined {
    return pages[Math.floor(index / PAGE)]?.[index % PAGE];
  }

  function request(page: number, l: DirListing) {
    if (pages[page] !== undefined || inflight.has(page)) return;
    inflight.add(page);
    const dirId = l.dirId;
    const asked = order();

    listRows(dirId, page * PAGE, PAGE, asked)
      .then((w) => {
        // A window that lands after we have navigated, or after the sort
        // changed, belongs to a listing that is no longer on screen; filing it
        // would show another folder's rows, or these rows in the wrong places.
        if (listing()?.dirId !== dirId || order() !== asked) return;
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
        "--tree-w": `${treeWidth()}px`,
        "--cols": gridTemplate(columns()),
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

      <div class="body">
        <Tree current={current()} onNavigate={navigate} />
        <div
          class="vsplit"
          onPointerDown={startTreeDrag}
          onDblClick={() => {
            setTreeWidth(TREE_DEFAULT);
            localStorage.setItem(TREE_KEY, String(TREE_DEFAULT));
          }}
          title="Drag to resize · double-click to reset"
        />

        <div class="pane">
      <div class="hdr">
        <span class="art" />
        <For each={shownColumns()}>
          {(col, i) => (
            <span
              class={`col ${col.num ? "num" : ""} ${order().sort === col.key ? "sorted" : ""}`}
              onClick={() => chooseSort(col.key)}
              title={`Sort by ${col.label.toLowerCase()} · drag to reorder`}
              draggable={true}
              onDragStart={() => (dragFrom = i())}
              onDragOver={(e) => e.preventDefault()}
              onDrop={() => dropColumn(col.key)}
            >
              {col.label}
              <Show when={order().sort === col.key}>
                <span class="arrow">{order().desc ? "▾" : "▴"}</span>
              </Show>
              {/* Sits on the boundary and swallows the click, so grabbing the
                  edge resizes rather than sorting. */}
              <span
                class="grip"
                title={`Resize ${col.label.toLowerCase()}`}
                onPointerDown={(e) => startColumnDrag(e, col.key)}
                onClick={(e) => e.stopPropagation()}
                onDragStart={(e) => e.preventDefault()}
              />
            </span>
          )}
        </For>
        <span
          class="colpick"
          title="Choose columns"
          onClick={(e) => {
            e.stopPropagation();
            setPickingColumns(!pickingColumns());
          }}
        >
          ⋮
        </span>
      </div>

      <Show when={pickingColumns()}>
        <div class="scrim bare" onClick={() => setPickingColumns(false)} />
        <div class="colmenu">
          <For each={columns()}>
            {(col) => (
              <div class="colopt" onClick={() => toggleColumn(col.key)}>
                <span class={`tick ${col.visible ? "on" : ""}`}>✓</span>
                {col.label}
              </div>
            )}
          </For>
        </div>
      </Show>

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
                        onDblClick={() => {
                          const r = row();
                          if (r) playRow(r);
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
                              <For each={shownColumns()}>
                                {(col) =>
                                  col.key === "name" ? (
                                    <span class="name" title={r().name}>
                                      {r().name}
                                      <Show when={r().state}>
                                        <span class="bad"> [{r().state}]</span>
                                      </Show>
                                    </span>
                                  ) : (
                                    <span
                                      class={`${col.num ? "num mono" : col.dim ? "dim" : "mono"}${
                                        col.key === "key" ? " keycol" : ""
                                      }`}
                                      style={
                                        col.key === "key"
                                          ? { color: keyColour(r().keyNorm) }
                                          : undefined
                                      }
                                      title={cellText(r(), col.key)}
                                    >
                                      {cellText(r(), col.key)}
                                    </span>
                                  )
                                }
                              </For>
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
        </div>
      </div>

      <Transport state={transport()} onState={setTransport} />

      <div class="status">
        <Show when={!listing.error && listing()}>
          <span>{listing()!.total} entries</span>
          <span class={listing()!.scanMs > 500 ? "slow" : ""}>
            scan {listing()!.scanMs}ms
          </span>
        </Show>
        {/* Only while tags are still being read. A folder already in the index
            opens with everything filled in and says nothing here. */}
        <Show when={probed() > 0}>
          <span class="working">reading tags… {probed()}</span>
        </Show>
        <Show when={windowError()}>
          <span class="slow">{windowError()}</span>
        </Show>
      </div>

      <Show when={finding()}>
        <Search
          scopes={searchScopes()}
          onClose={() => setFinding(false)}
          onOpen={revealFromSearch}
        />
      </Show>
    </div>
  );
}
