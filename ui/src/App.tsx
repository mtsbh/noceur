import {
  createSignal,
  createResource,
  createEffect,
  createMemo,
  on,
  onCleanup,
  For,
  Show,
} from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { listen } from "@tauri-apps/api/event";
import { createVirtualizer } from "@tanstack/solid-virtual";
import {
  openDir,
  listRows,
  parentOf,
  thumbUrl,
  probeBatch,
  play,
  transportCmd,
  reveal,
  pathOf,
  startDrag,
  fileOp,
  fileOpPaths,
  dropEffect,
  shellMenu,
  type TagField,
  openUrl,
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
import Menu, { type MenuItem } from "./Menu";
import SettingsPanel from "./SettingsPanel";
import TagEditor from "./TagEditor";
import CoverPicker from "./CoverPicker";
import MatchPicker from "./MatchPicker";
import QuickTag from "./QuickTag";
import Renamer from "./Renamer";
import Cleanup from "./Cleanup";
import Titlebar from "./Titlebar";
import { CaretIcon, SettingsIcon, TagIcon } from "./icons";
import {
  ACTIONS,
  loadSettings,
  saveSettings,
  type ActionId,
  type Settings as SettingsState,
} from "./settings";
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
  small: { art: 16, row: 24 },
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
  /**
   * Every selected row, by file id.
   *
   * Separate from `selected`, which is the anchor the keyboard and the range
   * shortcut work from. A set rather than a list because the only questions
   * asked of it are "is this row in it" and "what is in it".
   */
  const [selection, setSelection] = createSignal<Set<number>>(new Set());

  /** Ctrl adds and removes, shift extends from the anchor, a plain click replaces. */
  function selectRow(row: RowDisplay, e: MouseEvent) {
    const total = listing()?.total ?? 0;

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
        for (let i = Math.min(from, to); i <= Math.max(from, to) && i < total; i++) {
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

  /**
   * Runs one of the track actions.
   *
   * Each opens exactly what the matching right-click entry opens; the button is
   * a second route to the same dialogs rather than a second implementation.
   */
  function runAction(id: ActionId) {
    if (id === "quick") {
      setQuickOpen((open) => !open);
      return;
    }
    const ids = audioSelection();
    if (ids.length === 0) return;

    switch (id) {
      case "tags":
        setEditing(ids);
        break;
      // These two search on one track and apply to the whole selection, which is
      // the rule the menu follows: twelve tracks off one release share a cover
      // and an album, and searching for each of them would ask the same question
      // twelve times.
      case "match":
        setMatching({ fileId: ids[0], fileIds: ids });
        break;
      case "art":
        setCovering({ fileId: ids[0], fileIds: ids });
        break;
      case "rename":
        setRenaming(ids);
        break;
      case "clean":
        setCleaning(ids);
        break;
    }
  }

  /** Position of a row in the listing, or -1 if its window is not loaded. */
  function indexOf(fileId: number): number {
    for (const [page, rows] of Object.entries(pages)) {
      const at = rows.findIndex((r) => r.fileId === fileId);
      if (at >= 0) return Number(page) * PAGE + at;
    }
    return -1;
  }
  const [windowError, setWindowError] = createSignal("");
  // Files tag-read so far in this folder; 0 once the folder is fully probed.
  const [probed, setProbed] = createSignal(0);
  const [artSize, setArtSize] = createSignal<ArtSize>(storedArtSize());
  const [treeWidth, setTreeWidth] = createSignal(storedTreeWidth());
  const [order, setOrder] = createSignal<Order>(storedOrder());
  const [columns, setColumns] = createSignal<Column[]>(loadColumns());
  const [settings, setSettings] = createSignal<SettingsState>(loadSettings());
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [quickOpen, setQuickOpen] = createSignal(false);
  /** Files the rename dialog is open on, or null when it is closed. */
  const [renaming, setRenaming] = createSignal<number[] | null>(null);
  const [cleaning, setCleaning] = createSignal<number[] | null>(null);
  /** File ids the tag editor is open on, or null when it is closed. */
  const [editing, setEditing] = createSignal<number[] | null>(null);
  /** The row the cover search runs on, plus every file a pick applies to. */
  const [covering, setCovering] = createSignal<{ fileId: number; fileIds: number[] } | null>(
    null,
  );
  /** The row being matched online, and the files a chosen match would edit. */
  const [matching, setMatching] = createSignal<{ fileId: number; fileIds: number[] } | null>(
    null,
  );
  /** Fields a match pre-filled the editor with, cleared when it closes. */
  const [prefill, setPrefill] = createSignal<Partial<Record<TagField, string>>>({});
  /** Cover art the chosen match came with, offered in the editor alongside it. */
  const [prefillArt, setPrefillArt] = createSignal<string | undefined>(undefined);
  /**
   * Bumped whenever artwork is written from outside the tag editor.
   *
   * The editor draws the file's own cover, and the URL it draws it from names
   * the file rather than the picture — so nothing about it changes when the
   * cover search replaces one underneath it.
   */
  const [artNonce, setArtNonce] = createSignal(0);

  /** Every settings change is written straight through — there is no Save. */
  function commitSettings(next: SettingsState) {
    setSettings(next);
    saveSettings(next);
  }

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

  /**
   * Steps to the next or previous audio row in the listing.
   *
   * The listing is the playlist — auditioning means walking a folder in the
   * order on screen, so "next" follows the sort the user chose rather than a
   * queue built somewhere else. Rows whose window is not loaded stop the walk
   * rather than being guessed at.
   */
  function step(by: number) {
    const from = transport()?.fileId ?? selected();
    if (from == null) return;
    let at = indexOf(from);
    if (at < 0) return;

    const total = listing()?.total ?? 0;
    for (let i = at + by; i >= 0 && i < total; i += by) {
      const row = rowAt(i);
      if (!row) return;
      if (row.kind === 1) {
        playRow(row);
        return;
      }
    }
  }

  /**
   * Begins a native drag once the pointer has actually moved.
   *
   * A drag has to be told apart from a click, and the threshold is what does it:
   * without it, every click on a row would hand the mouse to the shell's modal
   * drag loop and selecting a track would become impossible.
   *
   * Everything selected is dragged, so dragging one of several selected rows
   * takes the whole selection — as Explorer does.
   */
  function startRowDrag(e: PointerEvent, row: RowDisplay) {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startY = e.clientY;

    const move = (ev: PointerEvent) => {
      if (Math.abs(ev.clientX - startX) < 5 && Math.abs(ev.clientY - startY) < 5) {
        return;
      }
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

  // A move drops rows out of this folder, so the listing is re-read once the
  // shell says the drag is over.
  createEffect(() => {
    const unlisten = listen<string>("drag-finished", (event) => {
      if (event.payload === "moved") refetchListing();
    });
    onCleanup(() => {
      unlisten.then((off) => off()).catch(() => {});
    });
  });

  const [dropping, setDropping] = createSignal(false);

  /**
   * Files dragged in from Explorer.
   *
   * The shell decides copy or move the same way Explorer does — move within a
   * volume, copy across one — and performs it through `IFileOperation`, so the
   * progress dialog, the name-collision prompt and the undo stack are the ones
   * the user already knows.
   */
  createEffect(() => {
    const pending = [
      listen("tauri://drag-enter", () => setDropping(true)),
      listen("tauri://drag-leave", () => setDropping(false)),
      listen<{ paths: string[] }>("tauri://drag-drop", async (event) => {
        setDropping(false);
        const paths = event.payload?.paths ?? [];
        const into = current();
        if (paths.length === 0) return;

        // A folder dropped onto itself would be the shell asked to move files
        // to where they already are.
        const fromHere = paths.every(
          (p) => p.toLowerCase().startsWith(into.toLowerCase() + "\\"),
        );
        if (fromHere) return;

        try {
          const effect = await dropEffect(paths, into);
          await fileOpPaths(effect === "move" ? "move" : "copy", paths, into);
          refetchListing();
        } catch {
          // The shell reports its own failures in its own dialog.
        }
      }),
    ];

    onCleanup(() => {
      for (const p of pending) p.then((off) => off()).catch(() => {});
    });
  });

  /** Deletes the selection to the recycle bin, through the shell. */
  function deleteSelection() {
    const ids = [...selection()];
    if (ids.length === 0) return;
    fileOp("delete", ids)
      .then(() => refetchListing())
      .catch(() => {});
  }

  // The row menu, and where it was opened.
  const [menu, setMenu] = createSignal<{
    row: RowDisplay;
    x: number;
    y: number;
  } | null>(null);

  /**
   * Right-click shows the shell's own menu — the same one Explorer shows, with
   * the same extensions in it.
   *
   * Falls back to the small built-in menu only if the shell refuses, which on a
   * non-Windows build it always does.
   */
  function openMenu(e: MouseEvent, row: RowDisplay) {
    e.preventDefault();
    e.stopPropagation();

    // Right-clicking outside the selection replaces it, as Explorer does;
    // right-clicking inside it keeps it, so a menu acts on what is selected.
    if (!selection().has(row.fileId)) {
      setSelection(new Set([row.fileId]));
      setSelected(row.fileId);
    }

    // Shift goes straight to the shell's menu, the way shift-right-click has
    // always meant "give me the longer menu" in Explorer.
    if (e.shiftKey) {
      openShellMenu();
      return;
    }

    setSelected(row.fileId);
    // The menu measures itself after mount and nudges back inside the window,
    // so the raw pointer position is the right starting point.
    setMenu({ row, x: e.clientX, y: e.clientY });
  }

  // Where the track-actions button was, or null when its menu is closed.
  const [actionMenu, setActionMenu] = createSignal<{ x: number; y: number } | null>(
    null,
  );

  /**
   * The track actions, as menu entries.
   *
   * The whole list every time, with the ones that need a selection greyed out
   * rather than missing. A menu whose entries come and go teaches nothing about
   * what the app can do — and "why is it not there" is a worse question than
   * "why is it grey".
   */
  function actionItems(): MenuItem[] {
    const count = audioSelection().length;
    return ACTIONS.map(({ id, label, key }) => ({
      kind: "item" as const,
      label: count > 1 && id !== "quick" ? `${label} — ${count} tracks` : label,
      hint: key,
      checked: id === "quick" ? quickOpen() : undefined,
      disabled: id !== "quick" && count === 0,
      run: () => runAction(id),
    }));
  }

  // Where the column chooser was opened from, or null when it is closed.
  const [columnMenu, setColumnMenu] = createSignal<{ x: number; y: number } | null>(
    null,
  );

  /** The column chooser, as menu entries that tick on and off. */
  function columnMenuItems(): MenuItem[] {
    return columns().map((col) => ({
      kind: "item" as const,
      label: col.label,
      checked: col.visible,
      // Name carries the row's identity and its state marker; a listing with it
      // hidden is a list of blanks.
      disabled: col.key === "name",
      run: () => toggleColumn(col.key),
    }));
  }

  /** Hands off to the shell's own menu, extensions and all. */
  function openShellMenu() {
    setMenu(null);
    shellMenu([...selection()])
      .then(() => refetchListing())
      .catch(() => {});
  }

  /**
   * What the row menu offers.
   *
   * The things a music browser knows and the shell does not go here; everything
   * the shell does better stays one entry away, under "Windows menu".
   */
  function rowMenuItems(row: RowDisplay): MenuItem[] {
    const many = selection().size > 1;
    const lookup = [row.artist, row.title || row.name].filter(Boolean).join(" ");

    return [
      {
        kind: "item",
        label: "Play",
        hint: "double-click",
        disabled: row.kind !== 1,
        run: () => playRow(row),
      },
      { kind: "separator" },
      {
        kind: "item",
        label: "Reveal in Explorer",
        run: () => {
          reveal(row.fileId).catch(() => {});
        },
      },
      {
        kind: "item",
        label: many ? `Copy ${selection().size} paths` : "Copy path",
        run: () => {
          Promise.all([...selection()].map((id) => pathOf(id)))
            .then((paths) => copyText(paths.join("\r\n")))
            .catch(() => {});
        },
      },
      {
        kind: "item",
        label: many ? `Edit metadata of ${selection().size} tracks…` : "Edit metadata…",
        hint: "f2",
        disabled: row.kind !== 1,
        run: () => setEditing([...selection()]),
      },
      {
        kind: "item",
        label: many ? `Rename ${selection().size} tracks…` : "Rename…",
        disabled: row.kind !== 1,
        run: () => setRenaming([...selection()]),
      },
      {
        kind: "item",
        label: many ? `Clean up ${selection().size} tracks…` : "Clean up…",
        disabled: row.kind !== 1,
        run: () => setCleaning([...selection()]),
      },
      {
        kind: "item",
        label: "Find metadata online…",
        disabled: row.kind !== 1,
        // Matched on the row that was right-clicked, applied to whatever is
        // selected — the same rule the rest of the menu follows.
        run: () => setMatching({ fileId: row.fileId, fileIds: [...selection()] }),
      },
      {
        kind: "item",
        label: many ? `Find album art for ${selection().size} tracks…` : "Find album art…",
        disabled: row.kind !== 1,
        // Searched on the row that was right-clicked, applied to the whole
        // selection: an album's tracks share a sleeve, and searching on each of
        // them would be twelve requests for one answer.
        run: () => setCovering({ fileId: row.fileId, fileIds: [...selection()] }),
      },
      { kind: "separator" },
      {
        kind: "item",
        label: "Search Discogs",
        disabled: row.kind !== 1 || lookup === "",
        run: () => {
          openUrl(
            `https://www.discogs.com/search/?q=${encodeURIComponent(lookup)}&type=release`,
          ).catch(() => {});
        },
      },
      {
        kind: "item",
        label: "Search YouTube",
        disabled: row.kind !== 1 || lookup === "",
        run: () => {
          openUrl(
            `https://www.youtube.com/results?search_query=${encodeURIComponent(lookup)}`,
          ).catch(() => {});
        },
      },
      { kind: "separator" },
      {
        kind: "item",
        label: many ? `Delete ${selection().size} items` : "Delete",
        hint: "del",
        danger: true,
        run: deleteSelection,
      },
      { kind: "separator" },
      {
        kind: "item",
        label: "Windows menu",
        hint: "shift-click",
        run: openShellMenu,
      },
    ];
  }

  /**
   * Copies text, preferring the async clipboard and falling back to a hidden
   * textarea.
   *
   * `navigator.clipboard` needs a secure context, which a custom-scheme web view
   * is not always considered to be; the fallback is deprecated but works
   * everywhere and costs nothing when it is not needed.
   */
  async function copyText(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      const box = document.createElement("textarea");
      box.value = text;
      box.style.position = "fixed";
      box.style.opacity = "0";
      document.body.appendChild(box);
      box.select();
      document.execCommand("copy");
      box.remove();
    }
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

      // Delete sends the selection to the recycle bin, as it does in Explorer.
      if (e.key === "Delete" && !typing && !finding()) {
        e.preventDefault();
        deleteSelection();
      }

      // T toggles quick tag. A single letter because the whole point is that a
      // hand stays on the keyboard.
      if (e.key.toLowerCase() === "t" && !typing && !finding()) {
        e.preventDefault();
        setQuickOpen((open) => !open);
      }

      // F2 opens the tag editor. Explorer renames on F2, but this is a music
      // browser: the thing you want to change about a track is almost never the
      // name of the file it lives in.
      if (e.key === "F2" && !typing && !finding() && selection().size > 0) {
        e.preventDefault();
        setEditing([...selection()]);
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
  const [listing, { refetch: refetchListing }] = createResource<DirListing, string>(
    current,
    (p) => openDir(p, order()),
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
    // Selection is handled by the pointer handler; this is the open action.
    if (row.kind === 0) {
      const sep = current().endsWith("\\") || current().endsWith("/") ? "" : "\\";
      navigate(`${current()}${sep}${row.name}`);
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

  return (
    <div
      class={`app ${dropping() ? "dropping" : ""}`}
      style={{
        "--row-h": `${metrics(artSize()).row}px`,
        "--art": `${metrics(artSize()).art}px`,
        "--tree-w": `${treeWidth()}px`,
        "--cols": gridTemplate(columns()),
      }}
    >
      <Titlebar />

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
        {/* One button rather than a row of them. Everything behind it is in
            the right-click menu too; this is the same list for anyone who
            would rather not hunt for the row first. */}
        <button
          class={`gearbtn caret ${actionMenu() ? "on" : ""}`}
          title="Track actions"
          aria-label="Track actions"
          onClick={(e) => {
            const box = e.currentTarget.getBoundingClientRect();
            setActionMenu(actionMenu() ? null : { x: box.left, y: box.bottom + 3 });
          }}
        >
          <TagIcon />
          <CaretIcon />
        </button>
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
        <button
          class={`gearbtn ${settingsOpen() ? "on" : ""}`}
          title="Settings"
          onClick={() => setSettingsOpen((open) => !open)}
        >
          <SettingsIcon />
        </button>
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
      <div
        class="hdr"
        onContextMenu={(e) => {
          // Right-clicking the header chooses columns, as it does in Explorer.
          e.preventDefault();
          e.stopPropagation();
          setColumnMenu({ x: e.clientX, y: e.clientY });
        }}
      >
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
            const box = e.currentTarget.getBoundingClientRect();
            setColumnMenu({ x: box.right - 170, y: box.bottom + 4 });
          }}
        >
          ⋮
        </span>
      </div>

      <Show when={columnMenu()}>
        <Menu
          x={columnMenu()!.x}
          y={columnMenu()!.y}
          items={columnMenuItems()}
          onClose={() => setColumnMenu(null)}
        />
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
                          if (r) playRow(r);
                        }}
                        onContextMenu={(e) => {
                          const r = row();
                          if (r) openMenu(e, r);
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
                                  setEditing([r().fileId]);
                                }}
                              >
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

      <Show when={menu()}>
        <Menu
          x={menu()!.x}
          y={menu()!.y}
          items={rowMenuItems(menu()!.row)}
          onClose={() => setMenu(null)}
        />
      </Show>

      <Show when={actionMenu()}>
        <Menu
          x={actionMenu()!.x}
          y={actionMenu()!.y}
          items={actionItems()}
          onClose={() => setActionMenu(null)}
        />
      </Show>

      <Show when={renaming()}>
        {(ids) => (
          <Renamer
            fileIds={ids()}
            onClose={() => setRenaming(null)}
            onRenamed={() => refetchListing()}
          />
        )}
      </Show>

      <Show when={cleaning()}>
        {(ids) => (
          <Cleanup
            fileIds={ids()}
            onClose={() => setCleaning(null)}
            onChanged={() => refetchListing()}
          />
        )}
      </Show>

      <Show when={matching()}>
        {(target) => (
          <MatchPicker
            fileId={target().fileId}
            onClose={() => setMatching(null)}
            onChoose={(fields, art) => {
              // Straight into the editor, pre-filled: the fields are visible as
              // proposed edits before anything is written, and the release's
              // cover comes with them rather than needing a second search.
              setPrefill(fields);
              setPrefillArt(art);
              setEditing(target().fileIds);
              setMatching(null);
            }}
          />
        )}
      </Show>

      <Show when={editing()}>
        {(ids) => (
          <TagEditor
            fileIds={ids()}
            initial={prefill()}
            initialArt={prefillArt()}
            artNonce={artNonce()}
            onFindArt={() => setCovering({ fileId: ids()[0], fileIds: ids() })}
            onClose={() => {
              setEditing(null);
              setPrefill({});
              setPrefillArt(undefined);
            }}
            onWritten={() => refetchListing()}
          />
        )}
      </Show>

      <Show when={covering()}>
        {(target) => (
          <CoverPicker
            fileId={target().fileId}
            fileIds={target().fileIds}
            onClose={() => setCovering(null)}
            onApplied={() => {
              refetchListing();
              // The editor may be open behind this, drawing the cover that was
              // just replaced.
              setArtNonce((n) => n + 1);
            }}
          />
        )}
      </Show>

      <Show when={settingsOpen()}>
        <SettingsPanel
          settings={settings()}
          onSettings={commitSettings}
          artSize={artSize()}
          onArtSize={chooseArtSize}
          columns={columns()}
          onToggleColumn={(key: string) => toggleColumn(key as SortColumn)}
          onClose={() => setSettingsOpen(false)}
        />
      </Show>

      <Show when={quickOpen()}>
        <QuickTag
          fileId={transport()?.fileId ?? 0}
          groups={settings().quick}
          rating={settings().quickRating}
          onClose={() => setQuickOpen(false)}
          onWritten={() => refetchListing()}
        />
      </Show>

      <Transport
        state={transport()}
        onState={setTransport}
        onPrev={() => step(-1)}
        onNext={() => step(1)}
        settings={settings()}
      />

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
