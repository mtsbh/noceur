import {
  createSignal,
  createResource,
  createEffect,
  createMemo,
  on,
  onCleanup,
  onMount,
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
  startDrag,
  writeTags,
  applyRename,
  announceRating,
  onRating,
  type TagField,
  type ArtSize,
  type Order,
  type SortColumn,
  type DirListing,
  type RowDisplay,
  // Aliased: `Show` is Solid's conditional component in this file.
  type Show as RowFilter,
} from "./api";
import { currentPath, rememberOrder, type Tab } from "./layout";
import { cellText, gridTemplate, keyColour, type Column } from "./columns";
import Stars from "./Stars";

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
 * One icon tile, in CSS pixels. Must match `.tile` in the theme.
 *
 * The width divides the pane into columns and the height is what the
 * virtualizer is told a grid row costs, so both are arithmetic here before they
 * are ever a rule — a tile that is 88px in CSS and 96px here would drift a row
 * out of place for every screenful scrolled.
 */
const TILE_W = 88;
const TILE_H = 84;

/** Case-insensitive, and blind to a trailing separator: Windows paths are both. */
const norm = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();
const sameFolder = (a?: string, b?: string) =>
  a !== undefined && b !== undefined && norm(a) === norm(b);

/**
 * What a pane lets the rest of the app do to it.
 *
 * The pane owns its listing, and everything outside it that needs to touch one —
 * the transport stepping to the next track, a dialog that has just renamed
 * files, the toolbar counting what is selected — reaches it through this. Every
 * field is an accessor rather than a value, so a caller reading one inside a
 * memo tracks it.
 */
export type PaneHandle = {
  tabId: number;
  /** The folder this pane is showing. */
  path: () => string;
  /** Re-reads the listing. Rows are replaced in place rather than cleared. */
  refetch: () => void;
  /** Every selected file id. */
  selection: () => number[];
  /** The selected ids that are audio files — what the dialogs act on. */
  audioSelection: () => number[];
  /** A loaded row by id, for things that need its tags — a web search. */
  rowOf: (fileId: number) => RowDisplay | undefined;
  /**
   * The audio row `by` steps from `fileId`, in this pane's order, or null when
   * there is none or its window is not loaded.
   */
  step: (fileId: number, by: number) => RowDisplay | null;
  /**
   * Opens a folder in this pane, optionally selecting a file once it arrives.
   *
   * The tree and the search results navigate through here rather than by
   * writing the tab record directly. Writing it directly changes the path and
   * leaves the pane's selection alone — and a selection is a set of file ids
   * from the folder you just left, which would leave Delete pointed at files in
   * a folder nobody is looking at any more.
   */
  navigate: (path: string, select?: number) => void;
  /** Steps this pane's own history. Each pane has its own, as a browser tab does. */
  back: () => void;
  forward: () => void;
};

/**
 * One browsing pane: a path, the rows in it, and the chrome around them.
 *
 * Mounted per *visible* tab rather than per tab — switching tabs unmounts this
 * and mounts another. Everything that has to survive that lives in the `Tab`
 * record and comes back through `props.tab`; everything here is what the pane is
 * currently showing, which is re-read on arrival anyway. Keeping every tab
 * mounted would mean a virtualizer measuring a container of zero height for
 * every tab not in front of you, and a folder's worth of rows held for each.
 */
export default function Pane(props: {
  tab: Tab;
  /** Whether this is the pane the tree, toolbar and keyboard are talking to. */
  focused: boolean;
  /** Whether the window is split, which decides if the pane needs a focus ring. */
  split: boolean;
  onFocus: () => void;
  /** Writes back to the tab record: navigation, sort, selection, scroll. */
  onTab: (patch: Partial<Tab>) => void;
  onReady: (handle: PaneHandle) => void;
  onGone: (tabId: number) => void;

  /** Columns, art size and the header drags are global — every pane shares them. */
  columns: Column[];
  artSize: ArtSize;
  /** Which rows the listing includes. A setting, so every pane gets the same one. */
  show: RowFilter;
  /** Whether a folder holding no audio is drawn as icons rather than as a list. */
  folderGrid: boolean;
  rowHeight: number;
  onColumnMenu: (x: number, y: number) => void;
  onColumnResize: (e: PointerEvent, key: SortColumn) => void;
  onColumnMove: (e: PointerEvent, key: SortColumn) => void;
  columnDragging: SortColumn | null;
  columnOver: SortColumn | null;

  /** Opens the row menu. The selection travels with it, so the menu acts on what was selected. */
  onMenu: (e: MouseEvent, row: RowDisplay, ids: number[]) => void;
  /** Opens the menu for the folder itself, from a right-click between rows. */
  onFolderMenu: (e: MouseEvent, path: string) => void;
  /** Opens the tag editor on one row, from a click on its cover art. */
  onArt: (fileId: number) => void;
  /** Loads a row into the player. */
  onPlay: (row: RowDisplay, tabId: number) => void;
  /**
   * The loaded track and how far into it the player is, so its row can show
   * `1:43 / 5:50` in the Time column. Null when nothing is playing or paused.
   */
  nowPlaying: { fileId: number; position: string } | null;
  /** Key of the drop target a file drag is over, so this pane can light up. */
  dropKey: string | null;
}) {
  const current = () => currentPath(props.tab);
  const order = () => props.tab.order;

  const [selected, setSelected] = createSignal<number | null>(props.tab.anchor);
  /**
   * Every selected row, by file id.
   *
   * Separate from `selected`, which is the anchor the keyboard and the range
   * shortcut work from. A set rather than a list because the only questions
   * asked of it are "is this row in it" and "what is in it".
   */
  const [selection, setSelection] = createSignal<Set<number>>(new Set(props.tab.selection));
  const [windowError, setWindowError] = createSignal("");
  /** Files tag-read so far in this folder; 0 once the folder is fully probed. */
  const [probed, setProbed] = createSignal(0);

  /** The scroll box's own width, which is what decides how many tiles fit. */
  const [paneWidth, setPaneWidth] = createSignal(0);

  let scrollEl: HTMLDivElement | undefined;
  let headEl: HTMLDivElement | undefined;
  let trail: HTMLSpanElement | undefined;

  // Keyed on the path only. Changing the sort must not re-open the directory:
  // opening re-enumerates it, and the order is a property of the read, not of
  // the folder. A sort change clears the cached windows instead.
  //
  // The fetcher is wrapped rather than passed directly, because Solid calls it
  // as `fetcher(source, info)` and would hand that info object to openDir's
  // second parameter.
  const [listing, { refetch: refetchListing }] = createResource<DirListing, string>(
    current,
    (p) => openDir(p, order(), props.show),
  );

  // The path sits beside the buttons that navigate it, so a path too long for a
  // narrow pane overflows to the right — off the end, taking the folder you are
  // actually in with it. Scrolling the box to its end keeps that folder on
  // screen and drops the front of the path instead, which is the half nobody
  // reads. `overflow: hidden` still scrolls under script; only the user cannot.
  createEffect(
    on(
      () => [current(), listing()] as const,
      () => {
        if (trail) trail.scrollLeft = trail.scrollWidth;
      },
    ),
  );

  /**
   * Whether to show "Reading…" instead of the rows.
   *
   * Only while a *different* folder is loading. A re-read of the folder already
   * on screen - after a drag, a tag write, a rating - keeps the rows up. Swapping
   * them for the placeholder collapsed the scroll box, the browser clamped the
   * offset to zero, and every refresh threw the list back to the top.
   */
  const reading = () =>
    listing.loading && (listing.state !== "refreshing" || !sameFolder(listing()?.path, current()));

  /** Row count of the whole listing — what the virtualizer sizes itself against. */
  const total = () => (listing.error ? 0 : (listing()?.total ?? 0));

  /**
   * Whether this folder is drawn as icons rather than as a list.
   *
   * A folder holding no audio at all is a set of places, not a table of values:
   * every column except the name is empty for a directory, so the list spends a
   * whole screen's width saying nothing. One track in it and the columns start
   * meaning something again, so the list comes back.
   *
   * `audio` is counted by the backend rather than inferred from the rows on
   * screen — the listing is directories-first, so a first window that is all
   * folders proves nothing about the thousand rows behind it.
   */
  const grid = () =>
    props.folderGrid && !listing.error && (listing()?.audio ?? 1) === 0 && total() > 0;

  /** Tiles across the pane. At least one, however narrow it has been dragged. */
  const perRow = () => Math.max(1, Math.floor(Math.max(paneWidth(), TILE_W) / TILE_W));

  /** The listing indices a grid row holds, left to right. */
  function slotsIn(gridRow: number): number[] {
    const first = gridRow * perRow();
    const last = Math.min(first + perRow(), total());
    const out: number[] = [];
    for (let i = first; i < last; i++) out.push(i);
    return out;
  }

  /**
   * The listing indices a virtual item covers.
   *
   * One row is one item in the list, and `perRow` of them in the grid — which is
   * the whole difference between the two modes as far as fetching is concerned.
   */
  const firstOf = (index: number) => (grid() ? index * perRow() : index);
  const lastOf = (index: number) =>
    grid() ? Math.min(total() - 1, index * perRow() + perRow() - 1) : index;

  // Rows the frontend actually holds, keyed by page index. A store rather than a
  // signal so an arriving window re-renders its own rows and not the entire list.
  const [pages, setPages] = createStore<Record<number, RowDisplay[]>>({});
  // Pages already asked for. Deliberately not reactive: it exists only to stop
  // duplicate requests, and nothing is rendered from it.
  let inflight = new Set<number>();

  const virtualizer = createVirtualizer({
    // In the grid the virtualizer counts *rows of tiles*, not tiles: it measures
    // one dimension, and a grid scrolls in that one dimension too.
    get count() {
      return grid() ? Math.ceil(total() / perRow()) : total();
    },
    getScrollElement: () => scrollEl ?? null,
    estimateSize: () => (grid() ? TILE_H : props.rowHeight),
    overscan: OVERSCAN,
  });

  // Changing the art size changes every row's height at once. The virtualizer
  // caches what it has measured, so it has to be told the old numbers are void —
  // without this the rows keep their previous spacing and overlap or gap.
  createEffect(
    on(
      () => [props.rowHeight, grid(), perRow()] as const,
      () => virtualizer.measure(),
      { defer: true },
    ),
  );

  // How many tiles fit is a function of the pane's width, which changes with the
  // tree divider, the split divider and the window. Observed rather than read on
  // render: none of those three go through this component.
  onMount(() => {
    if (!scrollEl) return;
    const box = new ResizeObserver((entries) => setPaneWidth(entries[0].contentRect.width));
    box.observe(scrollEl);
    onCleanup(() => box.disconnect());
  });

  /** The row at a listing-wide index, or `undefined` while its window is in flight. */
  function rowAt(index: number): RowDisplay | undefined {
    return pages[Math.floor(index / PAGE)]?.[index % PAGE];
  }

  /**
   * Sets the stars of a row - or of the whole selection, when the row is part
   * of it, which is how rating a batch works everywhere else in the list.
   *
   * Shown at once and written behind, because a click on a star that took a
   * round trip to light up would feel broken. A write that fails puts the old
   * values back. Pressing the stars a track already has clears them.
   */
  function rate(row: RowDisplay, stars: number) {
    const ids = selection().has(row.fileId)
      ? audioSelection()
      : row.kind === 1
        ? [row.fileId]
        : [];
    if (ids.length === 0) return;
    const next = stars === row.rating ? 0 : stars;

    const before = showRating(ids, next);
    writeTags(ids, {}, {}, String(next))
      .then((report) => {
        if (report.failures.length > 0) throw new Error(report.failures[0].reason);
        announceRating({ ids, stars: next, from: "list" });
      })
      .catch(() => {
        for (const [page, rows] of Object.entries(pages)) {
          rows.forEach((r, i) => {
            const old = before.get(r.fileId);
            if (old !== undefined) setPages(Number(page), i, "rating", old);
          });
        }
      });
  }

  /** Lights the stars of loaded rows; returns what they showed before. */
  function showRating(ids: number[], stars: number): Map<number, number> {
    const before = new Map<number, number>();
    for (const [page, rows] of Object.entries(pages)) {
      rows.forEach((r, i) => {
        if (ids.includes(r.fileId)) {
          before.set(r.fileId, r.rating);
          setPages(Number(page), i, "rating", stars);
        }
      });
    }
    return before;
  }

  // Stars set in the quick-tag panel are written when the track changes, but
  // shown here straight away, so the list never disagrees with the panel.
  onCleanup(
    onRating((r) => {
      if (r.from === "quick") showRating(r.ids, r.stars);
    }),
  );

  /** Position of a row in the listing, or -1 if its window is not loaded. */
  function indexOf(fileId: number): number {
    for (const [page, rows] of Object.entries(pages)) {
      const at = rows.findIndex((r) => r.fileId === fileId);
      if (at >= 0) return Number(page) * PAGE + at;
    }
    return -1;
  }

  /**
   * The selected rows that are audio files.
   *
   * Both menus act on tracks, and a selection can hold folders — dragging a box
   * across a listing takes whatever it crosses. Rows whose window has since been
   * dropped are not in here, which is the same limit the shift-select range has
   * and for the same reason: the id is known, what it points at is not.
   */
  const audioSelection = createMemo(() => {
    const ids = selection();
    if (ids.size === 0) return [];
    const out: number[] = [];
    for (const rows of Object.values(pages)) {
      for (const r of rows) {
        if (r.kind === 1 && ids.has(r.fileId)) out.push(r.fileId);
      }
    }
    return out;
  });

  // The tab keeps the selection, so switching away and back does not lose it.
  // Written on change rather than only on the way out: a tab can also be closed
  // by the other pane, and a pane that has already unmounted cannot save
  // anything.
  createEffect(
    on(
      [selection, selected],
      ([ids, anchor]) => props.onTab({ selection: [...ids], anchor }),
      { defer: true },
    ),
  );

  // Scroll is written on the way out rather than on every scroll event, which
  // fires a hundred times a drag and would put a store write behind each one.
  onCleanup(() => {
    if (scrollEl) props.onTab({ scroll: scrollEl.scrollTop });
    props.onGone(props.tab.id);
  });

  onMount(() => {
    props.onReady({
      tabId: props.tab.id,
      path: current,
      refetch: () => refetchListing(),
      rowOf: (fileId) => {
        for (const rows of Object.values(pages)) {
          const hit = rows.find((r) => r.fileId === fileId);
          if (hit) return hit;
        }
        return undefined;
      },
      selection: () => [...selection()],
      audioSelection,
      step: (fileId, by) => {
        const at = indexOf(fileId);
        if (at < 0) return null;
        const count = total();
        for (let i = at + by; i >= 0 && i < count; i += by) {
          const row = rowAt(i);
          // A gap means the window is not loaded, and guessing what is in it
          // would play a file nobody pointed at.
          if (!row) return null;
          if (row.kind === 1) return row;
        }
        return null;
      },
      back,
      forward,
      navigate: (path, select) => {
        pendingSelect = select ?? null;
        if (norm(path) === norm(current())) {
          // Already here, so no listing will arrive to apply the selection.
          applyPending();
          return;
        }
        navigate(path);
      },
    });
  });

  /**
   * A file to select once the folder holding it is on screen.
   *
   * Search opens a folder *at* a result, and the result is a file id in a
   * listing that has not been read yet — so the selection cannot be made when
   * it is asked for, only when the rows arrive.
   */
  let pendingSelect: number | null = null;

  function applyPending() {
    const id = pendingSelect;
    if (id === null) return;
    pendingSelect = null;
    setSelection(new Set([id]));
    setSelected(id);
    // Scrolled to only if the row is in a window that has actually loaded.
    // Everything past the first is somewhere in a folder that may hold forty
    // thousand rows, and there is no way to ask where without reading them.
    const at = indexOf(id);
    if (at >= 0) virtualizer.scrollToIndex(at, { align: "center" });
  }

  /**
   * Restores the tab's scroll offset, once and only on the listing it came back
   * to.
   *
   * The effect below resets every new listing to the top, which is right for a
   * navigation and wrong for a tab that is simply being shown again. The flag is
   * what tells them apart.
   */
  let restoring = props.tab.scroll;
  /** The directory the rows on screen came from, so a re-read can be told from a navigation. */
  let shownDir: number | null = null;

  // A new listing invalidates every cached window. Drop them, seed page 0 from
  // the window that arrived with the listing, and go back to the top — otherwise
  // a folder opened at scroll offset 12,000 would start out of range.
  createEffect(
    on(listing, (l) => {
      if (!l) return;
      const refresh = shownDir === l.dirId;
      shownDir = l.dirId;
      inflight = new Set();
      setWindowError("");
      setProbed(0);
      if (refresh) {
        // The same folder read again: stay where you were. The windows on screen
        // are replaced in place, so nothing blinks; the rest are dropped so they
        // are fetched fresh when scrolled to, since rows may have come or gone.
        const keep = visiblePages();
        setPages(
          reconcile(
            Object.fromEntries(Object.entries(pages).filter(([page]) => keep.has(Number(page)))),
          ),
        );
        if (l.first.rows.length > 0) setPages(0, l.first.rows);
        refreshVisible(l.dirId);
      } else {
        setPages(reconcile({}));
        if (l.first.rows.length > 0) setPages(0, l.first.rows);
        if (scrollEl) scrollEl.scrollTop = restoring;
      }
      restoring = 0;
      applyPending();
      void runProbe(l.dirId);
    }),
  );

  // Filtering changes how many rows there are, not just their order, and the
  // total comes from the listing — so unlike a sort this has to re-open the
  // directory. Deferred: the first listing already asked with the right filter.
  createEffect(
    on(
      () => props.show,
      () => void refetchListing(),
      { defer: true },
    ),
  );

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

  /** The page indices the visible rows fall in. */
  function visiblePages(): Set<number> {
    const out = new Set<number>();
    const items = virtualizer.getVirtualItems();
    if (items.length === 0) return out;
    const from = Math.floor(firstOf(items[0].index) / PAGE);
    const to = Math.floor(lastOf(items[items.length - 1].index) / PAGE);
    for (let p = from; p <= to; p++) out.add(p);
    return out;
  }

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
    const shown = props.show;
    const from = Math.floor(firstOf(items[0].index) / PAGE);
    const to = Math.floor(lastOf(items[items.length - 1].index) / PAGE);
    for (let p = from; p <= to; p++) {
      listRows(dirId, p * PAGE, PAGE, asked, shown)
        .then((w) => {
          if (listing()?.dirId !== dirId || order() !== asked || props.show !== shown) return;
          setPages(Math.floor(w.offset / PAGE), w.rows);
        })
        .catch(() => {});
    }
  }

  /**
   * Reads the folder's tags in the background, a batch at a time.
   *
   * The listing is already on screen by now; this is what fills in BPM, key,
   * duration and cover art behind it. Each batch is a separate call so the index
   * is released between them — a folder-long hold is what used to make opening a
   * large folder feel like the app had stopped.
   *
   * Stops as soon as the pane navigates elsewhere: the tags of a folder nobody
   * is looking at are not worth the disk.
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

  function request(page: number, l: DirListing) {
    if (pages[page] !== undefined || inflight.has(page)) return;
    inflight.add(page);
    const dirId = l.dirId;
    const asked = order();
    const shown = props.show;

    listRows(dirId, page * PAGE, PAGE, asked, shown)
      .then((w) => {
        // A window that lands after we have navigated, or after the sort or the
        // filter changed, belongs to a listing that is no longer on screen;
        // filing it would show another folder's rows, or these rows in the
        // wrong places. The filter needs checking by hand because it leaves the
        // directory id alone — same folder, different list.
        if (listing()?.dirId !== dirId || order() !== asked || props.show !== shown) return;
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

    const from = Math.floor(firstOf(items[0].index) / PAGE);
    const to = Math.floor(lastOf(items[items.length - 1].index) / PAGE);
    for (let p = from; p <= to; p++) request(p, l);
  });

  function navigate(path: string) {
    // Truncate any forward history, exactly like a browser.
    const next = props.tab.history.slice(0, props.tab.cursor + 1);
    next.push(path);
    props.onTab({ history: next, cursor: next.length - 1 });
    setSelected(null);
    setSelection(new Set<number>());
  }

  const canBack = () => props.tab.cursor > 0;
  const canForward = () => props.tab.cursor < props.tab.history.length - 1;

  function back() {
    if (!canBack()) return;
    props.onTab({ cursor: props.tab.cursor - 1 });
    setSelected(null);
  }
  function forward() {
    if (!canForward()) return;
    props.onTab({ cursor: props.tab.cursor + 1 });
    setSelected(null);
  }
  async function up() {
    const parent = await parentOf(current());
    if (parent) navigate(parent);
  }

  /**
   * Clicking a header sorts by it; clicking the one already active reverses it.
   *
   * Directories stay grouped first either way — `desc` reverses the sort column,
   * not the grouping, so reversing by size does not bury the folders at the
   * bottom of the listing.
   */
  function chooseSort(column: SortColumn) {
    const now = order();
    const next: Order =
      now.sort === column ? { sort: column, desc: !now.desc } : { sort: column, desc: false };
    props.onTab({ order: next });
    // Also kept as the seed for the next new tab. The order belongs to this tab,
    // but "how do you like your listings" is an answer worth carrying forward.
    rememberOrder(next);
  }

  /** Ctrl adds and removes, shift extends from the anchor, a plain click replaces. */
  function selectRow(row: RowDisplay, e: MouseEvent) {
    const count = total();

    if (e.ctrlKey || e.metaKey) {
      const next = new Set(selection());
      if (next.has(row.fileId)) next.delete(row.fileId);
      else next.add(row.fileId);
      setSelection(next);
      setSelected(row.fileId);
      return;
    }

    if (e.shiftKey && selected() !== null) {
      // Both ends have to be on screen for the range to be known — the rows
      // between them may not be loaded, and inventing ids would select the
      // wrong files.
      const from = indexOf(selected()!);
      const to = indexOf(row.fileId);
      if (from >= 0 && to >= 0) {
        const next = new Set(selection());
        for (let i = Math.min(from, to); i <= Math.max(from, to) && i < count; i++) {
          const at = rowAt(i);
          if (at) next.add(at.fileId);
        }
        setSelection(next);
        return;
      }
    }

    setSelection(new Set([row.fileId]));
    setSelected(row.fileId);
  }

  /**
   * Begins a native drag once the pointer has actually moved.
   *
   * A drag has to be told apart from a click, and the threshold is what does it:
   * without it, every click on a row would hand the mouse to the shell's modal
   * drag loop and selecting a track would become impossible.
   *
   * Everything selected is dragged, so dragging one of several selected rows
   * takes the whole selection — as Explorer does. Where it lands is not decided
   * here: once `startDrag` returns the shell owns the mouse, and the drop is
   * seen by `App` as a Tauri drag event like any other, whether it comes down on
   * another tab, the other pane, or a window belonging to something else.
   */
  function startRowDrag(e: PointerEvent, row: RowDisplay) {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startY = e.clientY;

    const move = (ev: PointerEvent) => {
      if (Math.abs(ev.clientX - startX) < 5 && Math.abs(ev.clientY - startY) < 5) return;
      stop();
      const ids = selection().has(row.fileId) ? [...selection()] : [row.fileId];
      startDrag(ids).catch(() => {});
    };

    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  /**
   * Which columns can be edited in place, and what writing one does.
   *
   * The name renames the file; the rest write that one tag. Audio rows only -
   * a folder or a stray text file has no tags, and renaming those belongs to
   * Explorer.
   */
  const EDITABLE = new Set<SortColumn>([
    "name", "title", "artist", "album", "albumArtist", "genre",
    "year", "trackNo", "comment", "isrc", "bpm", "key",
  ]);
  const canEdit = (row: RowDisplay, col: SortColumn) => row.kind === 1 && EDITABLE.has(col);

  const [editing, setEditing] = createSignal<{ fileId: number; col: SortColumn } | null>(null);
  const [editError, setEditError] = createSignal("");
  /**
   * The row that was already the only selection when it was pressed, and the
   * cell pressed in. A second, unhurried click on a selected row edits, the way
   * Explorer does - a double-click still plays.
   */
  let armed: { fileId: number; col: SortColumn } | null = null;
  let editTimer: number | undefined;
  onCleanup(() => clearTimeout(editTimer));

  function beginEdit(fileId: number, col: SortColumn) {
    clearTimeout(editTimer);
    setEditError("");
    setEditing({ fileId, col });
  }

  /** Writes an edited cell, or renames the file for the name column. */
  async function commitEdit(row: RowDisplay, col: SortColumn, value: string) {
    const before = String(row[col as keyof RowDisplay] ?? "");
    if (value === before || (col === "name" && !value.trim())) {
      setEditing(null);
      return;
    }
    if (col === "name") {
      try {
        const report = await applyRename([[row.fileId, value.trim()]]);
        if (report.failures.length > 0) throw new Error(report.failures[0].reason);
        setEditing(null);
        refetchListing();
      } catch (e) {
        // Left open with the reason, so the name can be fixed rather than retyped.
        setEditError(e instanceof Error ? e.message : String(e));
      }
      return;
    }

    // Shown at once and taken back if the write fails, as ratings are.
    const put = (v: string) => {
      for (const [page, rows] of Object.entries(pages)) {
        const at = rows.findIndex((r) => r.fileId === row.fileId);
        if (at >= 0) setPages(Number(page), at, col as keyof RowDisplay, v as never);
      }
    };
    setEditing(null);
    put(value);
    writeTags([row.fileId], { [col as TagField]: value })
      .then((report) => {
        if (report.failures.length > 0) throw new Error(report.failures[0].reason);
      })
      .catch(() => put(before));
  }

  /** The box a cell turns into while it is being edited. */
  function CellEditor(p: { row: RowDisplay; col: SortColumn }) {
    const start = String(p.row[p.col as keyof RowDisplay] ?? "");
    // `done` once closed, so the blur that follows removing the box does not
    // write the same edit a second time.
    let busy = false;
    let done = false;
    const commit = async (el: HTMLInputElement) => {
      if (busy || done) return;
      busy = true;
      await commitEdit(p.row, p.col, el.value);
      busy = false;
      if (editing() === null) done = true;
    };
    return (
      <input
        class={`celledit ${editError() ? "bad" : ""}`}
        value={start}
        title={editError() || undefined}
        spellcheck={false}
        ref={(el) =>
          queueMicrotask(() => {
            el.focus();
            // The name without its extension, as Explorer selects it.
            const dot = p.col === "name" ? start.lastIndexOf(".") : -1;
            el.setSelectionRange(0, dot > 0 ? dot : start.length);
          })
        }
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => e.stopPropagation()}
        onDblClick={(e) => e.stopPropagation()}
        onContextMenu={(e) => e.stopPropagation()}
        onInput={() => setEditError("")}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") {
            e.preventDefault();
            void commit(e.currentTarget);
          } else if (e.key === "Escape") {
            e.preventDefault();
            done = true;
            setEditing(null);
          }
        }}
        onBlur={(e) => {
          if (done) return;
          // A failed rename keeps its box while focused; leaving it gives up.
          if (editError()) {
            done = true;
            setEditing(null);
          }
          else void commit(e.currentTarget);
        }}
      />
    );
  }

  /** Whether this cell of this row is the one open for editing. */
  const isEditing = (row: RowDisplay, col: SortColumn) =>
    editing()?.fileId === row.fileId && editing()?.col === col;

  function onRow(row: RowDisplay) {
    // Selection is handled by the pointer handler; this is the open action.
    if (row.kind === 0) {
      const here = current();
      const sep = here.endsWith("\\") || here.endsWith("/") ? "" : "\\";
      navigate(`${here}${sep}${row.name}`);
    }
  }

  /**
   * Everything a row and a tile do to the pointer, which is all of it.
   *
   * Spread onto both rather than written twice: the two views differ in how an
   * entry is drawn and in nothing else, and a right-click that replaced the
   * selection in one view but not the other would be a bug nobody found until
   * they used the icon view on a folder they cared about.
   */
  function itemHandlers(row: () => RowDisplay | undefined) {
    return {
      onPointerDown: (e: PointerEvent) => {
        const r = row();
        if (!r) return;
        const cell = (e.target as HTMLElement).closest<HTMLElement>("[data-edit]");
        const col = cell?.dataset.edit as SortColumn | undefined;
        armed =
          col &&
          props.focused &&
          e.button === 0 &&
          !e.shiftKey &&
          !e.ctrlKey &&
          !e.metaKey &&
          selected() === r.fileId &&
          selection().size === 1 &&
          canEdit(r, col)
            ? { fileId: r.fileId, col }
            : null;
        clearTimeout(editTimer);
        selectRow(r, e);
        startRowDrag(e, r);
      },
      onClick: () => {
        const r = row();
        if (!r) return;
        onRow(r);
        const a = armed;
        armed = null;
        // Late enough that a double-click cancels it first.
        if (a && a.fileId === r.fileId) {
          editTimer = window.setTimeout(() => beginEdit(a.fileId, a.col), 450);
        }
      },
      onDblClick: () => {
        clearTimeout(editTimer);
        const r = row();
        if (r) props.onPlay(r, props.tab.id);
      },
      onContextMenu: (e: MouseEvent) => {
        const r = row();
        if (!r) return;
        e.preventDefault();
        e.stopPropagation();
        // Right-clicking outside the selection replaces it, as Explorer does;
        // right-clicking inside it keeps it, so a menu acts on what is selected.
        let ids = selection();
        if (!ids.has(r.fileId)) {
          ids = new Set([r.fileId]);
          setSelection(ids);
          setSelected(r.fileId);
        }
        props.onMenu(e, r, [...ids]);
      },
    };
  }

  /** The marks a row and a tile share: what kind it is, and how it is selected. */
  function itemClass(base: string, row: RowDisplay | undefined): string {
    if (!row) return `${base} pending`;
    const kind = row.kind === 0 ? " dir" : row.kind === 2 ? " other" : "";
    const chosen = selection().has(row.fileId) ? " sel" : "";
    // The anchor is marked separately: with several rows selected it is the one
    // shift extends from, and it would otherwise be indistinguishable.
    const anchor = selected() === row.fileId && selection().size > 1 ? " anchor" : "";
    return base + kind + chosen + anchor;
  }


  const shown = () => props.columns.filter((c) => c.visible);
  const dropKey = () => `pane:${props.tab.id}`;

  /**
   * A folder row as a drop target of its own: files dropped on it go into it,
   * not into the folder on screen. Found by the same hit test as the pane,
   * which takes the innermost `data-drop` under the pointer.
   */
  const folderDrop = (row: RowDisplay | undefined) => {
    if (!row || row.kind !== 0) return {};
    const here = current();
    const sep = here.endsWith("\\") || here.endsWith("/") ? "" : "\\";
    const path = `${here}${sep}${row.name}`;
    return { "data-drop": `dir:${path}`, "data-path": path };
  };
  const dropping = (row: RowDisplay | undefined) =>
    row?.kind === 0 && props.dropKey === folderDrop(row)["data-drop"] ? " dropping" : "";

  return (
    <div
      class={`pane ${props.focused && props.split ? "focused" : ""} ${
        props.dropKey === dropKey() ? "dropping" : ""
      }`}
      style={{ "--cols": gridTemplate(props.columns) }}
      // Any press inside the pane makes it the focused one. The row's own
      // handler has already run by the time this fires, which does not matter:
      // both land in the same batch, so the toolbar never sees the new selection
      // attributed to the old pane.
      onPointerDown={() => props.onFocus()}
      // Read by the file-drop hit test in `App`: anywhere in the pane that is
      // not a tab means "into the folder this pane is showing".
      data-drop={dropKey()}
      data-path={current()}
    >
      <div class="crumbs">
        <button class="navbtn" onClick={back} disabled={!canBack()} data-tip="Back" data-tip-at="start" aria-label="Back">
          ←
        </button>
        <button
          class="navbtn"
          onClick={forward}
          disabled={!canForward()}
          data-tip="Forward"
          aria-label="Forward"
        >
          →
        </button>
        <button class="navbtn" onClick={up} data-tip="Up" aria-label="Up one folder">
          ↑
        </button>
        <span class="trail" ref={trail}>
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
        </span>
      </div>

      {/* The header is a second grid on the same template as the rows, in its own
          box outside the scroll container — so it has to be scrolled sideways by
          hand to stay over the columns it names. Visible in a pane too narrow
          for every column, which is most of what a split pane is. */}
      <div
        class="hdr"
        hidden={grid()}
        ref={(el) => (headEl = el)}
        onContextMenu={(e) => {
          // Right-clicking the header chooses columns, as it does in Explorer.
          e.preventDefault();
          e.stopPropagation();
          props.onColumnMenu(e.clientX, e.clientY);
        }}
      >
        <span class="art" />
        <For each={shown()}>
          {(col) => (
            <span
              data-col={col.key}
              class={`col ${col.num ? "num" : ""} ${order().sort === col.key ? "sorted" : ""} ${
                props.columnDragging === col.key ? "moving" : ""
              } ${
                props.columnOver === col.key &&
                props.columnDragging &&
                props.columnDragging !== col.key
                  ? "over"
                  : ""
              }`}
              // A drag that moved must not also sort on release.
              onClick={() => {
                if (props.columnDragging) return;
                chooseSort(col.key);
              }}
              title={`Sort by ${col.label.toLowerCase()} · drag to reorder`}
              onPointerDown={(e) => props.onColumnMove(e, col.key)}
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
                onPointerDown={(e) => props.onColumnResize(e, col.key)}
                onClick={(e) => e.stopPropagation()}
                onDragStart={(e) => e.preventDefault()}
              />
            </span>
          )}
        </For>
        <span
          class="colpick"
          data-tip="Columns"
          data-tip-at="end"
          aria-label="Choose columns"
          onClick={(e) => {
            e.stopPropagation();
            const box = e.currentTarget.getBoundingClientRect();
            props.onColumnMenu(box.right - 170, box.bottom + 4);
          }}
        >
          ⋮
        </span>
      </div>

      {/* The scroll container is the virtualizer's measuring stick, so it is
          always mounted — the loading and empty states render inside it. */}
      <div
        class="list"
        ref={(el) => (scrollEl = el)}
        // Rows stop their own right-click, so one that arrives here was on the
        // empty space around them - which is the folder itself, as in Explorer.
        onContextMenu={(e) => {
          e.preventDefault();
          props.onFolderMenu(e, current());
        }}
        onScroll={() => {
          if (headEl && scrollEl) headEl.scrollLeft = scrollEl.scrollLeft;
        }}
      >
        <Show
          when={!listing.error}
          fallback={<div class="empty">Can't open folder</div>}
        >
          <Show
            when={!reading()}
            fallback={<div class="empty">Loading…</div>}
          >
            <Show when={total() > 0} fallback={<div class="empty">Empty folder</div>}>
              {/* One tall spacer holds the scrollbar honest; only what is in
                  view exists inside it. In the grid the spacer counts rows of
                  tiles, which is what the virtualizer measures there. */}
              <div
                class={`canvas ${grid() ? "tiles" : ""}`}
                style={{ height: `${virtualizer.getTotalSize()}px` }}
              >
                <Show
                  when={grid()}
                  fallback={
                    <For each={virtualizer.getVirtualItems()}>
                      {(item) => {
                        const row = () => rowAt(item.index);
                        return (
                          <div
                            class={itemClass("row", row()) + dropping(row())}
                            style={{
                              height: `${item.size}px`,
                              transform: `translateY(${item.start}px)`,
                            }}
                            {...itemHandlers(row)}
                            {...folderDrop(row())}
                          >
                            <Show when={row()} fallback={<span class="ph" />}>
                              {(r) => { return (
                                <>
                                  {/* Only rows the probe found a picture in get
                                      an image at all; the rest render the empty
                                      cell directly rather than firing a request
                                      that can only 404. On error the image hides
                                      itself and the cell's own background shows
                                      through, which covers art that turns out
                                      not to decode. */}
                                  {/* Clicking the artwork opens the editor on
                                      that row, where the cover can be seen at a
                                      size worth looking at, replaced, or
                                      removed. The row is selected first, so the
                                      editor opens on what was clicked rather
                                      than on whatever was selected before. */}
                                  <span
                                    class="art"
                                    title={r().hasArt ? "Open cover art" : undefined}
                                    onClick={(e) => {
                                      if (!r().hasArt || r().kind !== 1) return;
                                      e.stopPropagation();
                                      setSelection(new Set([r().fileId]));
                                      setSelected(r().fileId);
                                      props.onArt(r().fileId);
                                    }}
                                  >
                                    <Show when={r().hasArt}>
                                      <img
                                        src={thumbUrl(r().fileId, props.artSize)}
                                        alt=""
                                        loading="lazy"
                                        decoding="async"
                                        draggable={false}
                                        onLoad={(e) =>
                                          e.currentTarget.classList.remove("blank")
                                        }
                                        onError={(e) =>
                                          e.currentTarget.classList.add("blank")
                                        }
                                      />
                                    </Show>
                                  </span>
                                  <For each={shown()}>
                                    {(col) =>
                                      col.key === "rating" ? (
                                        <Stars row={r()} onRate={(n) => rate(r(), n)} />
                                      ) : (
                                        <Show
                                          when={isEditing(r(), col.key)}
                                          fallback={cell(col)}
                                        >
                                          <span class="cellbox">
                                            <CellEditor row={r()} col={col.key} />
                                          </span>
                                        </Show>
                                      )
                                    }
                                  </For>
                                </>
                              );
                              /** Elapsed time, when this is the playing row's Time cell. */
                              function elapsed(col: Column): string | undefined {
                                const now = props.nowPlaying;
                                return col.key === "duration" && now?.fileId === r().fileId
                                  ? now.position
                                  : undefined;
                              }
                              function cell(col: Column) {
                                return col.key === "name" ? (
                                        <span class="name" title={r().name} data-edit="name">
                                          {r().name}
                                          <Show when={r().state}>
                                            <span class="bad"> [{r().state}]</span>
                                          </Show>
                                        </span>
                                      ) : (
                                        <span
                                          data-edit={EDITABLE.has(col.key) ? col.key : undefined}
                                          class={`${
                                            col.num ? "num mono" : col.dim ? "dim" : "mono"
                                          }${col.key === "key" ? " keycol" : ""}${
                                            elapsed(col) ? " nowtime" : ""
                                          }`}
                                          style={
                                            col.key === "key"
                                              ? { color: keyColour(r().keyNorm) }
                                              : undefined
                                          }
                                          title={cellText(r(), col.key)}
                                        >
                                          {elapsed(col)
                                            ? `${elapsed(col)} / ${cellText(r(), col.key)}`
                                            : cellText(r(), col.key)}
                                        </span>
                                      );
                              }
                            }}
                            </Show>
                          </div>
                        );
                      }}
                    </For>
                  }
                >
                  <For each={virtualizer.getVirtualItems()}>
                    {(item) => (
                      <div
                        class="tilerow"
                        style={{
                          height: `${item.size}px`,
                          transform: `translateY(${item.start}px)`,
                        }}
                      >
                        <For each={slotsIn(item.index)}>
                          {(index) => {
                            const row = () => rowAt(index);
                            return (
                              <Show when={row()} fallback={<div class="tile pending" />}>
                                {(r) => (
                                  <div
                                    class={itemClass("tile", r()) + dropping(r())}
                                    {...itemHandlers(row)}
                                    {...folderDrop(r())}
                                  >
                                    {/* The shell's own folder icon, which is what
                                        makes a folder that has set a custom one
                                        look like itself. */}
                                    <span class="tileart">
                                      <Show when={r().hasArt}>
                                        <img
                                          src={thumbUrl(r().fileId, "medium")}
                                          alt=""
                                          loading="lazy"
                                          decoding="async"
                                          draggable={false}
                                          onError={(e) =>
                                            e.currentTarget.classList.add("blank")
                                          }
                                        />
                                      </Show>
                                    </span>
                                    <span class="tilename" title={r().name}>
                                      {r().name}
                                    </span>
                                  </div>
                                )}
                              </Show>
                            );
                          }}
                        </For>
                      </div>
                    )}
                  </For>
                </Show>
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
        </Show>
        <Show when={selection().size > 0}>
          <span>{selection().size} selected</span>
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
    </div>
  );
}
