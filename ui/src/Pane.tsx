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

/** Case-insensitive, and blind to a trailing separator: Windows paths are both. */
const norm = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();

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
  rowHeight: number;
  onColumnMenu: (x: number, y: number) => void;
  onColumnResize: (e: PointerEvent, key: SortColumn) => void;
  onColumnMove: (e: PointerEvent, key: SortColumn) => void;
  columnDragging: SortColumn | null;
  columnOver: SortColumn | null;

  /** Opens the row menu. The selection travels with it, so the menu acts on what was selected. */
  onMenu: (e: MouseEvent, row: RowDisplay, ids: number[]) => void;
  /** Opens the tag editor on one row, from a click on its cover art. */
  onArt: (fileId: number) => void;
  /** Loads a row into the player. */
  onPlay: (row: RowDisplay, tabId: number) => void;
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

  /** Row count of the whole listing — what the virtualizer sizes itself against. */
  const total = () => (listing.error ? 0 : (listing()?.total ?? 0));

  // Rows the frontend actually holds, keyed by page index. A store rather than a
  // signal so an arriving window re-renders its own rows and not the entire list.
  const [pages, setPages] = createStore<Record<number, RowDisplay[]>>({});
  // Pages already asked for. Deliberately not reactive: it exists only to stop
  // duplicate requests, and nothing is rendered from it.
  let inflight = new Set<number>();

  const virtualizer = createVirtualizer({
    get count() {
      return total();
    },
    getScrollElement: () => scrollEl ?? null,
    estimateSize: () => props.rowHeight,
    overscan: OVERSCAN,
  });

  // Changing the art size changes every row's height at once. The virtualizer
  // caches what it has measured, so it has to be told the old numbers are void —
  // without this the rows keep their previous spacing and overlap or gap.
  createEffect(on(() => props.rowHeight, () => virtualizer.measure(), { defer: true }));

  /** The row at a listing-wide index, or `undefined` while its window is in flight. */
  function rowAt(index: number): RowDisplay | undefined {
    return pages[Math.floor(index / PAGE)]?.[index % PAGE];
  }

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
      if (scrollEl) {
        scrollEl.scrollTop = restoring;
        restoring = 0;
      }
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
    const from = Math.floor(items[0].index / PAGE);
    const to = Math.floor(items[items.length - 1].index / PAGE);
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

    const from = Math.floor(items[0].index / PAGE);
    const to = Math.floor(items[items.length - 1].index / PAGE);
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

  function onRow(row: RowDisplay) {
    // Selection is handled by the pointer handler; this is the open action.
    if (row.kind === 0) {
      const here = current();
      const sep = here.endsWith("\\") || here.endsWith("/") ? "" : "\\";
      navigate(`${here}${sep}${row.name}`);
    }
  }

  function rowClass(row: RowDisplay | undefined): string {
    if (!row) return "row pending";
    const kind = row.kind === 0 ? " dir" : row.kind === 2 ? " other" : "";
    const chosen = selection().has(row.fileId) ? " sel" : "";
    // The anchor is marked separately: with several rows selected it is the one
    // shift extends from, and it would otherwise be indistinguishable.
    const anchor = selected() === row.fileId && selection().size > 1 ? " anchor" : "";
    return "row" + kind + chosen + anchor;
  }

  const shown = () => props.columns.filter((c) => c.visible);
  const dropKey = () => `pane:${props.tab.id}`;

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
        <button class="navbtn" onClick={back} disabled={!canBack()} title="Back">
          ←
        </button>
        <button class="navbtn" onClick={forward} disabled={!canForward()} title="Forward">
          →
        </button>
        <button class="navbtn" onClick={up} title="Up one folder">
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
          title="Choose columns"
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
        onScroll={() => {
          if (headEl && scrollEl) headEl.scrollLeft = scrollEl.scrollLeft;
        }}
      >
        <Show
          when={!listing.error}
          fallback={<div class="empty">Could not read {current()}.</div>}
        >
          <Show
            when={!listing.loading}
            fallback={<div class="empty">Reading {current()}…</div>}
          >
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
                        onPointerDown={(e) => {
                          const r = row();
                          if (!r) return;
                          selectRow(r, e);
                          startRowDrag(e, r);
                        }}
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
                          if (r) props.onPlay(r, props.tab.id);
                        }}
                        onContextMenu={(e) => {
                          const r = row();
                          if (!r) return;
                          e.preventDefault();
                          e.stopPropagation();
                          // Right-clicking outside the selection replaces it, as
                          // Explorer does; right-clicking inside it keeps it, so
                          // a menu acts on what is selected.
                          let ids = selection();
                          if (!ids.has(r.fileId)) {
                            ids = new Set([r.fileId]);
                            setSelection(ids);
                            setSelected(r.fileId);
                          }
                          props.onMenu(e, r, [...ids]);
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
                              {/* Clicking the artwork opens the editor on that
                                  row, where the cover can be seen at a size
                                  worth looking at, replaced, or removed. The
                                  row is selected first, so the editor opens on
                                  what was clicked rather than on whatever
                                  happened to be selected before. */}
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
                                    onLoad={(e) => e.currentTarget.classList.remove("blank")}
                                    onError={(e) => e.currentTarget.classList.add("blank")}
                                  />
                                </Show>
                              </span>
                              <For each={shown()}>
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
                                      class={`${
                                        col.num ? "num mono" : col.dim ? "dim" : "mono"
                                      }${col.key === "key" ? " keycol" : ""}`}
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
