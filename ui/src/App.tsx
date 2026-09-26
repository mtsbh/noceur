import {
  createSignal,
  createEffect,
  createMemo,
  onCleanup,
  untrack,
  For,
  Show,
} from "solid-js";
import { createStore, produce } from "solid-js/store";
import { listen } from "@tauri-apps/api/event";
import {
  play,
  transportCmd,
  reveal,
  pathOf,
  parentOf,
  fileOp,
  fileOpPaths,
  dropEffect,
  shellMenu,
  spectrogram,
  writeTags,
  type TagField,
  actions,
  runAction as runUserAction,
  actionWrites,
  openUrl,
  type Action,
  type Transport as TransportState,
  type ArtSize,
  type SortColumn,
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
import Toolbar, { type ActionId } from "./Toolbar";
import Quality from "./Quality";
import Spectrogram from "./Spectrogram";
import ActionRunner from "./ActionRunner";
import Duplicates from "./Duplicates";
import Converter from "./Converter";
import NewFolder from "./NewFolder";
import Titlebar from "./Titlebar";
import Pane, { type PaneHandle } from "./Pane";
import Tabs, { type TabDrop } from "./Tabs";
import {
  activeTab,
  clampRatio,
  defaultOrder,
  currentPath,
  insertTab,
  loadLayout,
  newTab,
  relocateTab,
  removeTab,
  saveLayout,
  toggleSplit,
  type Group,
  type Layout,
  type Tab,
} from "./layout";
import {
  BoltIcon,
  BroomIcon,
  ClipboardIcon,
  ConvertIcon,
  CopiesIcon,
  GlobeIcon,
  ImageIcon,
  NewFolderIcon,
  OpenIcon,
  PigIcon,
  PlayIcon,
  RenameIcon,
  SettingsIcon,
  SpectrumIcon,
  SplitColsIcon,
  SplitRowsIcon,
  TabIcon,
  TagIcon,
  TrashIcon,
  VideoIcon,
  VinylIcon,
  WindowsIcon,
  actionIcon,
} from "./icons";
import { loadSettings, saveSettings, type Settings as SettingsState } from "./settings";
import { stackFor } from "./fonts";
import { applyTheme } from "./themes";
import {
  clampWidth,
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

/** Case-insensitive, and blind to a trailing separator: Windows paths are both. */
const norm = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();

/** The folder a path sits in. Empty for a drive root, which has no parent here. */
function parentDir(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = trimmed.replace(/[\\/][^\\/]+$/, "");
  return cut === trimmed ? "" : cut;
}

/** Whether `inner` is `outer` or sits somewhere beneath it. */
function within(inner: string, outer: string): boolean {
  const a = norm(inner);
  const b = norm(outer);
  return a === b || a.startsWith(b + "\\") || a.startsWith(b + "/");
}

/**
 * A dropped file's destination, hit-tested against the panes and tab strips.
 *
 * Every drop target carries `data-drop` (a key, unique per target, so only the
 * one under the pointer lights up) and `data-path` (where the files would go).
 * Hit-testing rather than DOM drag events because a drag that started in this
 * window is being run by the shell by the time it gets here: the web view sees
 * no pointer events during it, and the only thing that arrives is a Tauri drag
 * event carrying a physical screen position.
 */
function targetAt(x: number, y: number): { key: string; path: string } | null {
  const host = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-drop]");
  const key = host?.dataset.drop;
  const path = host?.dataset.path;
  return key && path ? { key, path } : null;
}

/** Whether a key event is somebody typing rather than reaching for a shortcut. */
function isTyping(e: KeyboardEvent): boolean {
  return (
    e.target instanceof HTMLElement &&
    (e.target.tagName === "INPUT" ||
      e.target.tagName === "TEXTAREA" ||
      e.target.isContentEditable)
  );
}

/**
 * Tauri reports drag positions in physical pixels; the DOM works in CSS pixels.
 *
 * On a 100% display these are the same number, which is exactly why the
 * conversion has to be here rather than discovered later on somebody's 150%
 * laptop, where every drop would land a third of the way up the window from
 * where it was aimed.
 */
function toClient(position: { x: number; y: number } | undefined) {
  if (!position) return null;
  const dpr = window.devicePixelRatio || 1;
  return { x: position.x / dpr, y: position.y / dpr };
}

export default function App() {
  const [artSize, setArtSize] = createSignal<ArtSize>(storedArtSize());
  const [treeWidth, setTreeWidth] = createSignal(storedTreeWidth());
  const [columns, setColumns] = createSignal<Column[]>(loadColumns());
  const [settings, setSettings] = createSignal<SettingsState>(loadSettings());
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [quickOpen, setQuickOpen] = createSignal(false);

  /* ── Tabs and panes ────────────────────────────────────────────────────── */

  const [layout, setLayout] = createStore<Layout>(loadLayout(home(), defaultOrder()));

  /**
   * Every mounted pane, by tab id.
   *
   * A pane owns its listing, so anything outside one that needs to touch it —
   * the transport stepping to the next track, a dialog that has just written to
   * files, the toolbar counting a selection — comes through here. A signal
   * rather than a plain map so those readings are reactive.
   */
  const [handles, setHandles] = createSignal<Record<number, PaneHandle>>({});

  const groupAt = (id: number) => layout.groups.findIndex((g) => g.id === id);

  const focusGroup = (): Group =>
    layout.groups.find((g) => g.id === layout.focus) ?? layout.groups[0];

  /** The pane the tree, the toolbar and the keyboard are talking to. */
  const focusHandle = (): PaneHandle | undefined => {
    const tab = activeTab(focusGroup());
    return tab ? handles()[tab.id] : undefined;
  };

  const focusPath = () => {
    const tab = activeTab(focusGroup());
    return tab ? currentPath(tab) : home();
  };

  /** The folder the other pane shows, when split - where files are filed to. */
  const otherFolder = () => {
    const other = layout.groups.find((g) => g.id !== focusGroup().id);
    const tab = other ? activeTab(other) : undefined;
    return tab ? currentPath(tab) : undefined;
  };

  const audioSelection = createMemo(() => focusHandle()?.audioSelection() ?? []);

  /** Re-reads every visible pane. A write can land in a folder either one shows. */
  function refetchAll() {
    for (const handle of Object.values(handles())) handle.refetch();
  }

  /**
   * Opens a folder in the focused pane, from the tree or a search result.
   *
   * Through the pane rather than by writing the tab record, because navigating
   * is more than changing the path: the selection belongs to the folder being
   * left, and a stale one would leave Delete pointed at files nobody can see.
   */
  /**
   * Opens the playing track's folder in the focused pane, with it selected.
   *
   * The focused pane rather than the one the track was started from: this is
   * "show me that file", and the answer belongs where the user is looking. The
   * pane is already told to hold the selection until the listing arrives, so
   * this works whether the folder is the one on screen or somewhere else.
   */
  async function locatePlaying() {
    const playing = transport();
    if (!playing?.path) return;
    const folder = await parentOf(playing.path).catch(() => null);
    if (folder) navigateFocused(folder, playing.fileId);
  }

  function navigateFocused(path: string, select?: number) {
    const tab = activeTab(focusGroup());
    if (tab) handles()[tab.id]?.navigate(path, select);
  }

  // The layout is remembered across restarts. Only paths and sorts are read
  // here, so this tracks those and not the selection or scroll offset that share
  // the record — and the split ratio is read untracked, because a divider drag
  // changes it sixty times a second and each one would be a JSON write.
  createEffect(() => saveLayout(layout, untrack(() => layout.ratio)));

  function patchTab(tabId: number, patch: Partial<Tab>) {
    setLayout(
      produce((l) => {
        for (const group of l.groups) {
          const tab = group.tabs.find((t) => t.id === tabId);
          if (tab) {
            Object.assign(tab, patch);
            return;
          }
        }
      }),
    );
  }

  function openTab(groupId: number, path: string) {
    setLayout(produce((l) => insertTab(l, groupId, newTab(path, defaultOrder()))));
  }

  function closeTab(groupId: number, tabId: number) {
    setLayout(produce((l) => removeTab(l, groupId, tabId, home(), defaultOrder())));
  }

  function moveTab(tabId: number, to: TabDrop) {
    setLayout(produce((l) => relocateTab(l, tabId, to)));
  }

  /**
   * Where a tab being dragged would land.
   *
   * Up here rather than in the strip, because a tab dragged across the divider
   * is tracked by the strip it left and drawn by the strip it is over.
   */
  const [tabMark, setTabMark] = createSignal<TabDrop | null>(null);

  function splitOrJoin() {
    setLayout(produce((l) => toggleSplit(l, home(), defaultOrder())));
  }

  /** Flips a split between side by side and stacked. */
  function flipSplit() {
    setLayout("dir", layout.dir === "row" ? "column" : "row");
  }

  /**
   * Drags the divider between two panes.
   *
   * A ratio rather than a width, so the split survives the window being resized
   * — a pixel width would leave one pane the same size and the other taking
   * every pixel the window gained.
   */
  function startSplitDrag(e: PointerEvent) {
    e.preventDefault();
    const host = (e.currentTarget as HTMLElement).parentElement;
    if (!host) return;
    const box = host.getBoundingClientRect();
    document.body.classList.add(layout.dir === "row" ? "resizing" : "resizing-v");

    const move = (ev: PointerEvent) => {
      const along =
        layout.dir === "row"
          ? (ev.clientX - box.left) / box.width
          : (ev.clientY - box.top) / box.height;
      setLayout("ratio", clampRatio(along));
    };

    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      document.body.classList.remove("resizing", "resizing-v");
      saveLayout(layout, layout.ratio);
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  /* ── Dialogs ───────────────────────────────────────────────────────────── */

  /** The user's own buttons, and the one being run. */
  const [buttons, setButtons] = createSignal<Action[]>([]);
  const [running, setRunning] = createSignal<{ action: Action; fileIds: number[] } | null>(
    null,
  );
  const [actionMenu, setActionMenu] = createSignal<{ x: number; y: number } | null>(null);

  /** Whether the spectrogram strip is showing. */
  const [spekOpen, setSpekOpen] = createSignal(false);
  const [spek, setSpek] = createSignal<Uint8Array>(new Uint8Array());
  /** Files the rename dialog is open on, or null when it is closed. */
  const [renaming, setRenaming] = createSignal<number[] | null>(null);
  const [cleaning, setCleaning] = createSignal<number[] | null>(null);
  /** The folder a new folder is being named in, or null. */
  const [newFolderIn, setNewFolderIn] = createSignal<string | null>(null);
  /** Tells the tree to re-read a branch something was just made in. */
  const [treeReload, setTreeReload] = createSignal<{ path: string; n: number } | null>(null);
  /** The menu for a folder itself, from the empty space in a listing. */
  const [folderMenu, setFolderMenu] = createSignal<{ x: number; y: number; path: string } | null>(
    null,
  );

  /** A folder was made: show it in the listing and in the tree. */
  function folderCreated(parent: string) {
    refetchAll();
    setTreeReload((r) => ({ path: parent, n: (r?.n ?? 0) + 1 }));
  }

  /** Files the converter is open on, or null when it is closed. */
  const [converting, setConverting] = createSignal<number[] | null>(null);
  /** Files the quality report is open on, or null when it is closed. */
  const [checking, setChecking] = createSignal<number[] | null>(null);
  /** Files the duplicate finder is measuring, or null when it is closed. */
  const [comparing, setComparing] = createSignal<number[] | null>(null);
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

  const [finding, setFinding] = createSignal(false);
  const [transport, setTransport] = createSignal<TransportState | null>(null);
  /**
   * The tab whose listing the player is walking.
   *
   * Next and previous follow the folder the track was started from, not whatever
   * pane happens to be focused now — otherwise clicking into the other pane
   * mid-track would silently repoint the transport at a different folder.
   */
  const [playingTab, setPlayingTab] = createSignal<number | null>(null);

  /**
   * Runs one of the track actions.
   *
   * Each opens exactly what the matching right-click entry opens; the button is
   * a second route to the same dialogs rather than a second implementation.
   */
  function runAction(id: ActionId, ids: number[] = audioSelection()) {
    if (id === "quick") {
      setQuickOpen((open) => !open);
      return;
    }
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
      case "quality":
        setChecking(ids);
        break;
      case "copies":
        setComparing(ids);
        break;
      case "convert":
        setConverting(ids);
        break;
    }
  }

  /** Every settings change is written straight through — there is no Save. */
  function commitSettings(next: SettingsState) {
    setSettings(next);
    saveSettings(next);
  }

  // The chosen face, pushed at the one custom property the whole theme reads.
  // Set on the root element rather than swapped into the stylesheet so it
  // survives every rule that inherits from it, and applies the moment it
  // changes rather than on the next reload.
  createEffect(() => {
    const s = settings();
    document.documentElement.style.setProperty("--ui-font", stackFor(s.font, s.fontCustom));
  });

  createEffect(() => applyTheme(settings().theme));

  function commitColumns(next: Column[]) {
    setColumns(next);
    saveColumns(next);
  }

  function toggleColumn(key: SortColumn) {
    commitColumns(columns().map((c) => (c.key === key ? { ...c, visible: !c.visible } : c)));
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
          c.key === key ? { ...c, width: clampWidth(startWidth + ev.clientX - startX) } : c,
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

  /** The column being dragged to a new position, and the one it is over. */
  const [dragging, setDragging] = createSignal<SortColumn | null>(null);
  const [dragOver, setDragOver] = createSignal<SortColumn | null>(null);

  /**
   * Reorders columns by dragging a heading onto another one.
   *
   * Pointer events rather than HTML5 drag-and-drop, which does not work here at
   * all: this window has Tauri's native file drop enabled so tracks can be
   * dragged in from Explorer, and on Windows that hands every drag gesture in
   * the web view to the OS. `dragstart` and `drop` simply never fire, which is
   * why the headings looked draggable and were not.
   *
   * A drag only begins after a few pixels of movement, so a click still sorts.
   */
  function startColumnMove(e: PointerEvent, key: SortColumn) {
    if (e.button !== 0) return;
    const startX = e.clientX;
    let moved = false;

    const columnAt = (x: number): SortColumn | null => {
      const under = document.elementFromPoint(x, e.clientY);
      const head = under?.closest<HTMLElement>("[data-col]");
      return (head?.dataset.col as SortColumn | undefined) ?? null;
    };

    const move = (ev: PointerEvent) => {
      if (!moved) {
        // Below this it is a click that wobbled, not a drag.
        if (Math.abs(ev.clientX - startX) < 4) return;
        moved = true;
        setDragging(key);
        document.body.classList.add("resizing");
      }
      setDragOver(columnAt(ev.clientX));
    };

    const stop = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      if (!moved) return;

      document.body.classList.remove("resizing");
      const target = columnAt(ev.clientX);
      setDragOver(null);
      // Cleared after the click that follows this pointerup, so a heading
      // dragged somewhere else does not also sort by itself on release.
      setTimeout(() => setDragging(null), 0);
      if (!target || target === key) return;

      const all = columns();
      const from = all.findIndex((c) => c.key === key);
      const to = all.findIndex((c) => c.key === target);
      if (from >= 0 && to >= 0) commitColumns(reorder(all, from, to));
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  function chooseArtSize(size: ArtSize) {
    setArtSize(size);
    localStorage.setItem(ART_KEY, size);
  }

  /**
   * Drags the divider between the tree and the panes.
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

    const move = (ev: PointerEvent) => setTreeWidth(clampTree(startWidth + ev.clientX - startX));

    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      document.body.classList.remove("resizing");
      localStorage.setItem(TREE_KEY, String(treeWidth()));
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  // Measured only while the strip is open — a full decode per track is not
  // something to do for a panel nobody is looking at. Keyed on the file id
  // rather than on the transport, which is replaced ten times a second while
  // something plays.
  createEffect<string>((previous) => {
    const id = spekOpen() ? (transport()?.fileId ?? 0) : 0;
    const key = `${spekOpen()}:${id}`;
    if (key === previous) return key;

    setSpek(new Uint8Array());
    if (id > 0) {
      spectrogram(id)
        .then((grid) => {
          if ((transport()?.fileId ?? 0) === id) setSpek(grid);
        })
        .catch(() => {});
    }
    return key;
  }, "");

  // Re-read whenever the settings panel closes, which is the only place they
  // are edited.
  createEffect(() => {
    if (settingsOpen()) return;
    actions()
      .then(setButtons)
      .catch(() => {});
  });

  /**
   * The user's buttons, as menu entries.
   *
   * Anything that changes a file opens its preview; opening a program or a link
   * changes nothing and simply runs, because a dialog asking permission to do
   * what was just clicked is a dialog nobody reads.
   */
  function buttonItems(): MenuItem[] {
    const ids = audioSelection();
    if (buttons().length === 0) {
      return [
        {
          kind: "item",
          label: "No buttons yet — add them in Settings",
          disabled: true,
          run: () => {},
        },
      ];
    }

    return buttons().map((action) => ({
      kind: "item" as const,
      label: ids.length > 1 ? `${action.label} — ${ids.length} tracks` : action.label,
      icon: actionIcon(action.icon),
      disabled: ids.length === 0,
      run: () => runButton(action, ids),
    }));
  }

  /** Runs a user-defined button on these files, through the preview if it writes. */
  function runButton(action: Action, ids: number[]) {
    if (actionWrites(action.kind)) {
      setRunning({ action, fileIds: ids });
      return;
    }
    runUserAction(action, ids).catch(() => {});
  }

  /** Buttons given an icon, which also sit in the toolbar. */
  const toolbarButtons = () =>
    buttons().flatMap((action) => {
      const icon = actionIcon(action.icon);
      return icon
        ? [{ id: action.id, label: action.label, icon, run: () => runButton(action, audioSelection()) }]
        : [];
    });

  /** Loads a row into the player, and remembers which tab it is walking. */
  function playRow(row: RowDisplay, tabId: number) {
    if (row.kind !== 1) return;
    setPlayingTab(tabId);
    play(row.fileId).then(setTransport).catch(() => {});
  }

  /**
   * Steps to the next or previous audio row in the playing tab's listing.
   *
   * The listing is the playlist — auditioning means walking a folder in the
   * order on screen, so "next" follows the sort the user chose rather than a
   * queue built somewhere else. If that tab has since been closed, the focused
   * pane takes over rather than the transport going dead.
   */
  function step(by: number) {
    const from = transport()?.fileId;
    if (from == null) return;
    const tabId = playingTab();
    const handle = (tabId !== null ? handles()[tabId] : undefined) ?? focusHandle();
    if (!handle) return;
    const row = handle.step(from, by);
    if (row) playRow(row, handle.tabId);
  }

  /* ── Drag and drop ─────────────────────────────────────────────────────── */

  /** The drop target under a file drag, or null when nothing is being dragged. */
  const [dropOver, setDropOver] = createSignal<{ key: string; path: string } | null>(null);

  /**
   * Files dropped on the window, from anywhere.
   *
   * There is deliberately one path here for all of it. A drag out of this
   * window's own list becomes a shell drag the moment it starts — `start_drag`
   * calls `DoDragDrop`, which owns the mouse until the button comes up — so a
   * drag from one tab to another arrives back here as exactly the same Tauri
   * event as a drag from Explorer, carrying the same real paths. Treating them
   * as one case is not a shortcut; it is what they are.
   *
   * Where it lands is hit-tested against `data-drop`, so a track can be dropped
   * on the other pane, or on a tab that is not even in front of you.
   */
  createEffect(() => {
    const pending = [
      listen<{ position?: { x: number; y: number } }>("tauri://drag-enter", (event) => {
        const at = toClient(event.payload?.position);
        setDropOver(at ? targetAt(at.x, at.y) : null);
      }),
      listen<{ position?: { x: number; y: number } }>("tauri://drag-over", (event) => {
        const at = toClient(event.payload?.position);
        setDropOver(at ? targetAt(at.x, at.y) : null);
      }),
      listen("tauri://drag-leave", () => setDropOver(null)),
      listen<{ paths: string[]; position?: { x: number; y: number } }>(
        "tauri://drag-drop",
        async (event) => {
          const target = dropOver();
          setDropOver(null);

          const paths = event.payload?.paths ?? [];
          if (paths.length === 0) return;

          // Hit-tested again at the drop position rather than trusting the last
          // hover: a drop can arrive without a preceding `drag-over` at the same
          // place, and landing files in the pane the pointer passed over on the
          // way would be worse than not landing them at all.
          const at = toClient(event.payload?.position);
          const into = (at ? targetAt(at.x, at.y) : null)?.path ?? target?.path ?? focusPath();

          // Files already sitting in the destination. Not an error — it is
          // dropping a selection back where it came from, which is a gesture
          // people make constantly and which should do nothing.
          if (paths.every((p) => norm(parentDir(p)) === norm(into))) return;

          // A folder cannot be moved inside itself. The shell would refuse, but
          // it would refuse in a dialog, and tabs make this an easy thing to aim
          // at by accident.
          if (paths.some((p) => within(into, p))) return;

          try {
            const effect = await dropEffect(paths, into);
            await fileOpPaths(effect === "move" ? "move" : "copy", paths, into);
            // Both panes: a move empties one folder and fills another, and they
            // may well both be on screen.
            refetchAll();
          } catch {
            // The shell reports its own failures in its own dialog.
          }
        },
      ),
    ];

    onCleanup(() => {
      for (const p of pending) p.then((off) => off()).catch(() => {});
    });
  });

  // A move drops rows out of a folder, so every pane is re-read once the shell
  // says the drag is over — the destination may be the other pane, or a tab
  // that is not in front of anybody.
  createEffect(() => {
    const unlisten = listen<string>("drag-finished", (event) => {
      if (event.payload === "moved") refetchAll();
    });
    onCleanup(() => {
      unlisten.then((off) => off()).catch(() => {});
    });
  });

  /** Deletes the focused pane's selection to the recycle bin, through the shell. */
  function deleteSelection() {
    const ids = focusHandle()?.selection() ?? [];
    if (ids.length === 0) return;
    fileOp("delete", ids)
      .then(() => refetchAll())
      .catch(() => {});
  }

  /* ── Menus ─────────────────────────────────────────────────────────────── */

  /** The row menu: what was right-clicked, where, and what was selected then. */
  const [menu, setMenu] = createSignal<{
    row: RowDisplay;
    ids: number[];
    x: number;
    y: number;
  } | null>(null);

  function openMenu(e: MouseEvent, row: RowDisplay, ids: number[]) {
    // Shift goes straight to the shell's menu, the way shift-right-click has
    // always meant "give me the longer menu" in Explorer.
    if (e.shiftKey) {
      openShellMenu(ids);
      return;
    }
    // The menu measures itself after mount and nudges back inside the window,
    // so the raw pointer position is the right starting point.
    setMenu({ row, ids, x: e.clientX, y: e.clientY });
  }

  // Where the column chooser was opened from, or null when it is closed.
  const [columnMenu, setColumnMenu] = createSignal<{ x: number; y: number } | null>(null);

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
  function openShellMenu(ids: number[]) {
    setMenu(null);
    shellMenu(ids)
      .then(() => refetchAll())
      .catch(() => {});
  }

  /**
   * What the row menu offers.
   *
   * The things a music browser knows and the shell does not go here; everything
   * the shell does better stays one entry away, under "Windows menu".
   */
  function rowMenuItems(row: RowDisplay, ids: number[]): MenuItem[] {
    const many = ids.length > 1;
    const lookup = [row.artist, row.title || row.name].filter(Boolean).join(" ");
    const audio = row.kind !== 1;

    // Grouped the way the toolbar is - the file itself, what it says, what is
    // done to it, where to look it up - with the toolbar's icons, so the same
    // action looks the same wherever it is reached from.
    return [
      {
        kind: "item",
        label: "Play",
        hint: "double-click",
        icon: PlayIcon,
        disabled: audio,
        run: () => playRow(row, playingTab() ?? (activeTab(focusGroup())?.id ?? 0)),
      },
      { kind: "separator" },
      {
        kind: "item",
        label: "Open in new tab",
        icon: TabIcon,
        disabled: row.kind !== 0,
        run: () => {
          const here = focusPath();
          const sep = here.endsWith("\\") || here.endsWith("/") ? "" : "\\";
          openTab(focusGroup().id, `${here}${sep}${row.name}`);
        },
      },
      {
        kind: "item",
        label: "New folder…",
        hint: "ctrl+shift+n",
        icon: NewFolderIcon,
        run: () => setNewFolderIn(focusPath()),
      },
      {
        kind: "item",
        label: "Reveal in Explorer",
        icon: OpenIcon,
        run: () => {
          reveal(row.fileId).catch(() => {});
        },
      },
      {
        kind: "item",
        label: many ? `Copy ${ids.length} paths` : "Copy path",
        icon: ClipboardIcon,
        run: () => {
          Promise.all(ids.map((id) => pathOf(id)))
            .then((paths) => copyText(paths.join("\r\n")))
            .catch(() => {});
        },
      },
      { kind: "separator" },
      {
        kind: "item",
        label: many ? `Edit metadata of ${ids.length} tracks…` : "Edit metadata…",
        hint: "f2",
        icon: TagIcon,
        disabled: audio,
        run: () => setEditing(ids),
      },
      {
        kind: "item",
        label: "Find metadata online…",
        icon: GlobeIcon,
        disabled: audio,
        // Matched on the row that was right-clicked, applied to whatever is
        // selected — the same rule the rest of the menu follows.
        run: () => setMatching({ fileId: row.fileId, fileIds: ids }),
      },
      {
        kind: "item",
        label: many ? `Find album art for ${ids.length} tracks…` : "Find album art…",
        icon: ImageIcon,
        disabled: audio,
        // Searched on the row that was right-clicked, applied to the whole
        // selection: an album's tracks share a sleeve, and searching on each of
        // them would be twelve requests for one answer.
        run: () => setCovering({ fileId: row.fileId, fileIds: ids }),
      },
      { kind: "separator" },
      {
        kind: "item",
        label: many ? `Rename ${ids.length} tracks…` : "Rename…",
        icon: RenameIcon,
        disabled: audio,
        run: () => setRenaming(ids),
      },
      {
        kind: "item",
        label: many ? `Clean up ${ids.length} tracks…` : "Clean up…",
        icon: BroomIcon,
        disabled: audio,
        run: () => setCleaning(ids),
      },
      {
        kind: "item",
        label: many ? `Convert ${ids.length} tracks…` : "Convert…",
        icon: ConvertIcon,
        disabled: audio,
        run: () => setConverting(ids),
      },
      {
        kind: "item",
        label: many ? `Check quality of ${ids.length} tracks…` : "Check quality…",
        icon: SpectrumIcon,
        disabled: audio,
        run: () => setChecking(ids),
      },
      {
        kind: "item",
        label: many ? `Find duplicates of ${ids.length} tracks…` : "Find duplicates…",
        icon: CopiesIcon,
        disabled: audio,
        run: () => setComparing(ids),
      },
      { kind: "separator" },
      {
        kind: "item",
        label: "Search Discogs",
        icon: VinylIcon,
        disabled: audio || lookup === "",
        run: () => {
          openUrl(
            `https://www.discogs.com/search/?q=${encodeURIComponent(lookup)}&type=release`,
          ).catch(() => {});
        },
      },
      {
        kind: "item",
        label: "Search YouTube",
        icon: VideoIcon,
        disabled: audio || lookup === "",
        run: () => {
          openUrl(
            `https://www.youtube.com/results?search_query=${encodeURIComponent(lookup)}`,
          ).catch(() => {});
        },
      },
      { kind: "separator" },
      {
        kind: "item",
        label: many ? `Delete ${ids.length} items` : "Delete",
        hint: "del",
        icon: TrashIcon,
        danger: true,
        run: deleteSelection,
      },
      { kind: "separator" },
      {
        kind: "item",
        label: "Windows menu",
        hint: "shift-click",
        icon: WindowsIcon,
        run: () => openShellMenu(ids),
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

  /* ── Keyboard ──────────────────────────────────────────────────────────── */

  // Registered on the window rather than a focusable element so the shortcuts
  // work without the list having been clicked first.
  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;

      if (mod && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setFinding(true);
        return;
      }

      // The tab and split shortcuts. Ctrl+T is the reason every bare-letter
      // shortcut below checks the modifier: without it, opening a tab would also
      // toggle quick tag.
      if (mod && e.key.toLowerCase() === "t") {
        e.preventDefault();
        openTab(focusGroup().id, focusPath());
        return;
      }
      if (mod && e.key.toLowerCase() === "w") {
        e.preventDefault();
        const group = focusGroup();
        closeTab(group.id, group.active);
        return;
      }
      if (mod && e.key === "\\") {
        e.preventDefault();
        splitOrJoin();
        return;
      }
      // Back and forward walk the focused pane's own history, as they do in a
      // browser tab — which is what a tab here is.
      if (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        e.preventDefault();
        const handle = focusHandle();
        if (e.key === "ArrowLeft") handle?.back();
        else handle?.forward();
        return;
      }
      // Alt+1..5 rates the selected tracks, Alt+0 clears. Alt rather than
      // Ctrl, which already jumps between tabs by number.
      if (e.altKey && !mod && /^[0-5]$/.test(e.key) && !isTyping(e)) {
        const ids = audioSelection();
        if (ids.length > 0) {
          e.preventDefault();
          writeTags(ids, {}, {}, e.key)
            .then(refetchAll)
            .catch(() => {});
        }
        return;
      }
      // Ctrl+Shift+N makes a folder where you are, as it does in Explorer.
      if (mod && e.shiftKey && e.key.toLowerCase() === "n" && !isTyping(e)) {
        e.preventDefault();
        setNewFolderIn(focusPath());
        return;
      }
      // Bare Tab moves between panes, per the spec. Safe to take: nothing in
      // the browsing surface is reached by tabbing to it, and while the user is
      // in a text field the guard below has already returned.
      if (e.key === "Tab" && !mod && !e.altKey && !isTyping(e)) {
        e.preventDefault();
        const groups = layout.groups;
        if (groups.length > 1) {
          setLayout("focus", groups[(groupAt(layout.focus) + 1) % groups.length].id);
        }
        return;
      }
      if (mod && e.key === "Tab") {
        e.preventDefault();
        const group = focusGroup();
        const at = group.tabs.findIndex((t) => t.id === group.active);
        const by = e.shiftKey ? -1 : 1;
        const next = (at + by + group.tabs.length) % group.tabs.length;
        setLayout("groups", groupAt(group.id), "active", group.tabs[next].id);
        return;
      }
      // Ctrl+1..9 jumps to a tab by position, and Ctrl+9 to the last one however
      // many there are — the rule browsers use.
      if (mod && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        const group = focusGroup();
        const at = e.key === "9" ? group.tabs.length - 1 : Number(e.key) - 1;
        if (at < group.tabs.length) {
          setLayout("groups", groupAt(group.id), "active", group.tabs[at].id);
        }
        return;
      }

      // Below here are the bare-key shortcuts, which must not fire while the
      // user is typing — the search box is a text field and a space there is a
      // space, not a transport command.
      if (isTyping(e) || finding() || mod || e.altKey) return;

      if (e.key === " ") {
        e.preventDefault();
        transportCmd("toggle").then(setTransport).catch(() => {});
        return;
      }

      // Delete sends the selection to the recycle bin, as it does in Explorer.
      if (e.key === "Delete") {
        e.preventDefault();
        deleteSelection();
        return;
      }

      // T toggles quick tag. A single letter because the whole point is that a
      // hand stays on the keyboard.
      if (e.key.toLowerCase() === "t") {
        e.preventDefault();
        setQuickOpen((open) => !open);
        return;
      }

      // F2 opens the tag editor. Explorer renames on F2, but this is a music
      // browser: the thing you want to change about a track is almost never the
      // name of the file it lives in.
      if (e.key === "F2") {
        const ids = focusHandle()?.selection() ?? [];
        if (ids.length > 0) {
          e.preventDefault();
          setEditing(ids);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  /**
   * Where search looks: every pinned folder, plus the folder in front of you.
   *
   * The current path is included whether or not it is pinned — searching only
   * pinned folders would ignore wherever the user actually is.
   */
  const searchScopes = () => {
    const pins = storedPins();
    const here = focusPath();
    return pins.some((p) => within(here, p)) ? pins : [...pins, here];
  };

  const metrics = () => ART[artSize()];

  return (
    <div
      class="app"
      style={{
        "--row-h": `${metrics().row}px`,
        "--art": `${metrics().art}px`,
        "--tree-w": `${treeWidth()}px`,
      }}
    >
      <Titlebar />

      <div class="topbar">
        <span class="grow" />
        <Toolbar
          count={audioSelection().length}
          quickOpen={quickOpen()}
          onRun={runAction}
          custom={toolbarButtons()}
        />
        <div class="sizes" role="group" aria-label="Cover art size">
          <For each={ART_SIZES}>
            {(size) => (
              <button
                class={`sizebtn ${artSize() === size ? "on" : ""}`}
                data-tip={`${size[0].toUpperCase()}${size.slice(1)}`}
                aria-label={`${size} cover art`}
                onClick={() => chooseArtSize(size)}
              >
                {size[0].toUpperCase()}
              </button>
            )}
          </For>
        </div>
        <div class="tgroup" role="group" aria-label="Panes">
        <button
          class={`gearbtn ${layout.groups.length > 1 ? "on" : ""}`}
          data-tip={layout.groups.length > 1 ? "Single pane" : "Dual pane"}
          data-tip-at="end"
          aria-label="Split"
          onClick={splitOrJoin}
        >
          <SplitColsIcon />
        </button>
        <Show when={layout.groups.length > 1}>
          <button
            class="gearbtn"
            data-tip={layout.dir === "row" ? "Stack" : "Side by side"}
          data-tip-at="end"
            aria-label="Split orientation"
            onClick={flipSplit}
          >
            <SplitRowsIcon />
          </button>
        </Show>
        </div>
        <div class="tgroup" role="group" aria-label="App">
        <button
          class={`gearbtn ${actionMenu() ? "on" : ""}`}
          data-tip="Functions"
          data-tip-at="end"
          aria-label="Functions"
          onClick={(e) => {
            const box = e.currentTarget.getBoundingClientRect();
            setActionMenu(actionMenu() ? null : { x: box.left - 150, y: box.bottom + 4 });
          }}
        >
          <BoltIcon />
        </button>
        <button
          class={`gearbtn ${spekOpen() ? "on" : ""}`}
          data-tip="Spek"
          data-tip-at="end"
          aria-label="Spectrogram"
          onClick={() => setSpekOpen((open) => !open)}
        >
          <PigIcon />
        </button>
        <button
          class={`gearbtn ${settingsOpen() ? "on" : ""}`}
          data-tip="Settings"
          data-tip-at="end"
          aria-label="Settings"
          onClick={() => setSettingsOpen((open) => !open)}
        >
          <SettingsIcon />
        </button>
        </div>
      </div>

      <div class="body">
        <Tree
          current={focusPath()}
          onNavigate={(path) => navigateFocused(path)}
          dropKey={dropOver()?.key ?? null}
          onNewFolder={setNewFolderIn}
          onOpenTab={(path) => openTab(focusGroup().id, path)}
          reload={treeReload()}
        />
        <div
          class="vsplit"
          onPointerDown={startTreeDrag}
          onDblClick={() => {
            setTreeWidth(TREE_DEFAULT);
            localStorage.setItem(TREE_KEY, String(TREE_DEFAULT));
          }}
          title="Drag to resize · double-click to reset"
        />

        <div class="panes" data-split={layout.dir}>
          <For each={layout.groups}>
            {(group, gi) => (
              <>
                <Show when={gi() > 0}>
                  <div
                    class="psplit"
                    onPointerDown={startSplitDrag}
                    onDblClick={() => {
                      setLayout("ratio", 0.5);
                      saveLayout(layout, 0.5);
                    }}
                    title="Drag to resize · double-click to even out"
                  />
                </Show>
                <div
                  class="slot"
                  style={{
                    // Only meaningful with two groups; a lone pane takes the
                    // whole thing whatever the ratio says.
                    flex:
                      layout.groups.length > 1
                        ? `${gi() === 0 ? layout.ratio : 1 - layout.ratio} 1 0`
                        : "1 1 0",
                  }}
                >
                  <Tabs
                    group={group}
                    focused={layout.focus === group.id}
                    dropKey={dropOver()?.key ?? null}
                    onActivate={(tabId) => {
                      setLayout("groups", gi(), "active", tabId);
                      setLayout("focus", group.id);
                    }}
                    onClose={(tabId) => closeTab(group.id, tabId)}
                    onNew={() => {
                      const tab = activeTab(group);
                      openTab(group.id, tab ? currentPath(tab) : home());
                    }}
                    onMove={moveTab}
                    mark={tabMark()}
                    onMark={setTabMark}
                  />
                  {/* Keyed on the tab, so switching tabs mounts a fresh pane
                      rather than re-pointing this one: a pane holds a
                      virtualizer and a folder's loaded windows, and neither
                      belongs to the tab you just switched to. */}
                  <Show when={activeTab(group)} keyed>
                    {(tab) => (
                      <Pane
                        tab={tab}
                        focused={layout.focus === group.id}
                        split={layout.groups.length > 1}
                        onFocus={() => setLayout("focus", group.id)}
                        onTab={(patch) => patchTab(tab.id, patch)}
                        onReady={(handle) =>
                          setHandles((all) => ({ ...all, [handle.tabId]: handle }))
                        }
                        onGone={(tabId) =>
                          setHandles((all) => {
                            const next = { ...all };
                            delete next[tabId];
                            return next;
                          })
                        }
                        columns={columns()}
                        artSize={artSize()}
                        show={settings().tracksOnly ? "tracks" : "everything"}
                        folderGrid={settings().folderGrid}
                        rowHeight={metrics().row}
                        onColumnMenu={(x, y) => setColumnMenu({ x, y })}
                        onColumnResize={startColumnDrag}
                        onColumnMove={startColumnMove}
                        columnDragging={dragging()}
                        columnOver={dragOver()}
                        onMenu={openMenu}
                        onFolderMenu={(e, path) =>
                          setFolderMenu({ x: e.clientX, y: e.clientY, path })
                        }
                        onArt={(fileId) => setEditing([fileId])}
                        onPlay={playRow}
                        dropKey={dropOver()?.key ?? null}
                      />
                    )}
                  </Show>
                </div>
              </>
            )}
          </For>
        </div>
      </div>

      <Show when={columnMenu()}>
        <Menu
          x={columnMenu()!.x}
          y={columnMenu()!.y}
          items={columnMenuItems()}
          onClose={() => setColumnMenu(null)}
        />
      </Show>

      <Show when={menu()}>
        <Menu
          x={menu()!.x}
          y={menu()!.y}
          items={rowMenuItems(menu()!.row, menu()!.ids)}
          onClose={() => setMenu(null)}
        />
      </Show>

      <Show when={renaming()}>
        {(ids) => (
          <Renamer fileIds={ids()} onClose={() => setRenaming(null)} onRenamed={refetchAll} />
        )}
      </Show>

      <Show when={cleaning()}>
        {(ids) => (
          <Cleanup fileIds={ids()} onClose={() => setCleaning(null)} onChanged={refetchAll} />
        )}
      </Show>

      <Show when={actionMenu()}>
        <Menu
          x={actionMenu()!.x}
          y={actionMenu()!.y}
          items={buttonItems()}
          onClose={() => setActionMenu(null)}
        />
      </Show>

      <Show when={running()}>
        {(target) => (
          <ActionRunner
            action={target().action}
            fileIds={target().fileIds}
            onClose={() => setRunning(null)}
            onDone={refetchAll}
          />
        )}
      </Show>

      <Show when={folderMenu()}>
        {(m) => (
          <Menu
            x={m().x}
            y={m().y}
            onClose={() => setFolderMenu(null)}
            items={[
              {
                kind: "item",
                label: "New folder…",
                hint: "ctrl+shift+n",
                icon: NewFolderIcon,
                run: () => setNewFolderIn(m().path),
              },
            ]}
          />
        )}
      </Show>

      <Show when={newFolderIn()}>
        {(parent) => (
          <NewFolder
            parent={parent()}
            onClose={() => setNewFolderIn(null)}
            onCreated={() => folderCreated(parent())}
          />
        )}
      </Show>

      <Show when={converting()}>
        {(ids) => (
          <Converter
            fileIds={ids()}
            otherFolder={otherFolder()}
            onClose={() => setConverting(null)}
            onDone={refetchAll}
          />
        )}
      </Show>

      <Show when={checking()}>
        {(ids) => <Quality fileIds={ids()} onClose={() => setChecking(null)} />}
      </Show>

      <Show when={comparing()}>
        {(ids) => (
          <Duplicates
            fileIds={ids()}
            // Reported across the pinned folders as well as this one: a copy
            // matters precisely when it is somewhere else.
            scopes={[...storedPins(), focusPath()]}
            onClose={() => setComparing(null)}
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
            onWritten={refetchAll}
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
              refetchAll();
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

      <Show when={spekOpen()}>
        <Spectrogram
          grid={spek()}
          progress={
            (transport()?.durationMs ?? 0) > 0
              ? (transport()!.positionMs ?? 0) / transport()!.durationMs
              : 0
          }
          sampleRate={transport()?.sampleRate ?? 0}
          durationMs={transport()?.durationMs ?? 0}
          name={transport()?.path ?? ""}
        />
      </Show>

      <Show when={quickOpen()}>
        <QuickTag
          fileId={transport()?.fileId ?? 0}
          groups={settings().quick}
          rating={settings().quickRating}
          onClose={() => setQuickOpen(false)}
          onWritten={refetchAll}
        />
      </Show>

      <Transport
        state={transport()}
        onState={setTransport}
        onPrev={() => step(-1)}
        onNext={() => step(1)}
        onArt={(fileId) => setEditing([fileId])}
        onLocate={locatePlaying}
        settings={settings()}
        onMeter={(meter) => commitSettings({ ...settings(), meter })}
      />

      <Show when={finding()}>
        <Search
          scopes={searchScopes()}
          onClose={() => setFinding(false)}
          // Opened in the focused pane, with the row that matched selected
          // once the folder holding it has been read.
          onOpen={(dirPath, fileId) => navigateFocused(dirPath, fileId)}
        />
      </Show>
    </div>
  );
}
