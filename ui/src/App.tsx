import {
  createSignal,
  createEffect,
  createMemo,
  on,
  onCleanup,
  untrack,
  For,
  Show,
  type JSX,
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
  openWith,
  sendToTelegram,
  telegramAvailable,
  playlists as fetchPlaylists,
  playlistsHolding,
  togglePlaylist,
  type Playlist,
  type Matched,
  undoRename,
  previewTool,
  applyCleanup,
  previewRename,
  applyRename,
  previewFromName,
  applyFromName,
  clipboardPut,
  clipboardFiles,
  clipboardRelease,
  type Clip,
  folderIconUrl,
  siteIconUrl,
  type Action,
  type Transport as TransportState,
  type ArtSize,
  type SortColumn,
  type RowDisplay,
  type TagLink,
  indexFolders,
} from "./api";
import Tree, { storedPins } from "./Tree";
import Search from "./Search";
import Transport from "./Transport";
import Menu, { type MenuItem } from "./Menu";
import SettingsPanel, { setSettingsTab } from "./SettingsPanel";
import TagEditor from "./TagEditor";
import CoverPicker from "./CoverPicker";
import MatchPicker from "./MatchPicker";
import QuickTag from "./QuickTag";
import Sidebar from "./Sidebar";
import Renamer from "./Renamer";
import Toolbar, {
  BAR_VERSION,
  DEFAULT_LEAD,
  GROUPS,
  resolveBar,
  type ActionId,
  type BarItem,
  type GroupId,
} from "./Toolbar";
import Quality, { startQuality, type QualityRun } from "./Quality";
import Analysis from "./Analysis";
import Spectrogram from "./Spectrogram";
import ActionRunner from "./ActionRunner";
import Tools from "./Tools";
import Duplicates from "./Duplicates";
import Overview from "./Overview";
import Converter from "./Converter";
import NewFolder from "./NewFolder";
import Playlists from "./Playlists";
import NewPlaylist from "./NewPlaylist";
import Shortcuts from "./Shortcuts";
import FromName from "./FromName";
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
  ChevronIcon,
  ClipboardIcon,
  ConvertIcon,
  CopiesIcon,
  GlobeIcon,
  ListenIcon,
  ImageIcon,
  NameToTagIcon,
  MoveIcon,
  NewFolderIcon,
  KeyboardIcon,
  ChartIcon,
  PlaylistIcon,
  SendIcon,
  OpenIcon,
  OpenWithIcon,
  PigIcon,
  PlayIcon,
  RenameIcon,
  SearchIcon,
  SettingsIcon,
  SpectrumIcon,
  TempoIcon,
  SplitColsIcon,
  SplitRowsIcon,
  StarIcon,
  TabIcon,
  TagIcon,
  TrashIcon,
  WindowsIcon,
  CommandIcon,
  FolderIcon,
  actionIcon,
} from "./icons";
import Palette from "./Palette";
import {
  loadSettings,
  saveSettings,
  searchUrl,
  type Launcher,
  type Settings as SettingsState,
} from "./settings";
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
/** A program's own icon, as Explorer shows it, falling back to a plain glyph. */
function programIcon(path: string) {
  return () => <ImageGlyph src={folderIconUrl(path.trim().replace(/^"|"$/g, ""))} fallback={OpenWithIcon} />;
}

/** A site's favicon, falling back to a globe. */
function siteIcon(template: string) {
  return () => <ImageGlyph src={siteIconUrl(template)} fallback={GlobeIcon} />;
}

function ImageGlyph(props: { src: string; fallback: () => JSX.Element }) {
  const [broken, setBroken] = createSignal(!props.src);
  return (
    <Show when={!broken()} fallback={<props.fallback />}>
      <img
        class="glyphimg"
        src={props.src}
        alt=""
        draggable={false}
        onError={() => setBroken(true)}
      />
    </Show>
  );
}

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

  /**
   * Bumped by every re-read, so the sidebar knows the track it shows may have
   * been written.
   */
  const [written, setWritten] = createSignal(0);

  /** Re-reads every visible pane. A write can land in a folder either one shows. */
  function refetchAll() {
    for (const handle of Object.values(handles())) handle.refetch();
    setWritten((n) => n + 1);
  }

  /* ── Copy, cut, paste ──────────────────────────────────────────────────── */

  /**
   * What the Windows clipboard holds, when it is files. The clipboard itself is
   * the truth - shared with Explorer - and this is re-read whenever the window
   * comes back into focus or a menu opens, so Paste knows whether it can.
   */
  const [clip, setClip] = createSignal<Clip | null>(null);
  const readClip = () => clipboardFiles().then(setClip).catch(() => setClip(null));
  readClip();
  window.addEventListener("focus", readClip);
  window.addEventListener("contextmenu", readClip, true);
  onCleanup(() => {
    window.removeEventListener("focus", readClip);
    window.removeEventListener("contextmenu", readClip, true);
  });

  /** Puts the selection (or `ids`) on the clipboard, for here or Explorer. */
  async function clipFiles(cut: boolean, ids: number[] = focusHandle()?.selection() ?? []) {
    if (ids.length === 0) return;
    const paths = await Promise.all(ids.map((id) => pathOf(id)));
    try {
      await clipboardPut(paths, cut);
    } catch {
      return;
    }
    setClip({ paths, cut });
    flash(`${cut ? "Cut" : "Copied"} ${paths.length} ${paths.length === 1 ? "item" : "items"}`);
  }

  /**
   * Copies or moves files into a folder, through the shell - so collisions,
   * progress and Explorer's own Ctrl+Z all behave as they do there.
   */
  async function transfer(op: "copy" | "move", paths: string[], into: string) {
    // Pasting where they already are, or a folder into itself: nothing to do.
    if (paths.every((p) => norm(parentDir(p)) === norm(into))) return;
    if (paths.some((p) => within(into, p))) return;
    try {
      await fileOpPaths(op, paths, into);
      refetchAll();
    } catch {
      // The shell reports its own failures in its own dialog.
    }
  }

  async function paste(into: string = focusPath()) {
    // Read fresh: something may have been copied in Explorer a moment ago.
    const c = await clipboardFiles().catch(() => null);
    setClip(c);
    if (!c) return;
    await transfer(c.cut ? "move" : "copy", c.paths, into);
    // A cut is used up by pasting it, as in Explorer; a copy can be pasted again.
    if (c.cut) {
      await clipboardRelease(c.paths).catch(() => {});
      setClip(null);
    }
  }

  /** Copies or moves rows into the other pane's folder. */
  async function toOtherPane(op: "copy" | "move", ids: number[]) {
    const into = otherFolder();
    if (!into || ids.length === 0) return;
    const paths = await Promise.all(ids.map((id) => pathOf(id)));
    await transfer(op, paths, into);
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
  /** A built-in tag tool open on a selection. */
  const [tooling, setTooling] = createSignal<number[] | null>(null);

  /** Whether the spectrogram strip is showing. */
  const [spekOpen, setSpekOpen] = createSignal(false);
  const [spek, setSpek] = createSignal<Uint8Array>(new Uint8Array());
  /** Files the rename dialog is open on, or null when it is closed. */
  const [renaming, setRenaming] = createSignal<number[] | null>(null);
  /** The folder a new folder is being named in, or null. */
  const [newFolderIn, setNewFolderIn] = createSignal<string | null>(null);
  const [showKeys, setShowKeys] = createSignal(false);
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

  /**
   * Sends a tree folder to the Recycle Bin through the shell. A pane showing
   * it, or anything inside it, goes up to the folder that held it.
   */
  function deleteFolder(path: string) {
    // Asked before the delete: afterwards there is no folder to ask about.
    parentOf(path)
      .then(async (parent) => {
        await fileOpPaths("delete", [path]);
        const gone = (p: string) => {
          const a = p.toLowerCase().replace(/[\\/]+$/, "");
          const b = path.toLowerCase().replace(/[\\/]+$/, "");
          return a === b || a.startsWith(b + "\\") || a.startsWith(b + "/");
        };
        if (parent && gone(focusPath())) navigateFocused(parent);
        if (parent) setTreeReload((r) => ({ path: parent, n: (r?.n ?? 0) + 1 }));
        refetchAll();
      })
      .catch((e) => flash(String(e)));
  }

  /** Files being tagged from their names, or null when closed. */
  const [naming, setNaming] = createSignal<number[] | null>(null);
  /** Files the converter is open on, or null when it is closed. */
  const [converting, setConverting] = createSignal<number[] | null>(null);
  /** Files the quality report is open on, or null when it is closed. */
  // The quality check outlives its dialog: hiding the report leaves it running,
  // and the status line brings it back.
  const [checking, setChecking] = createSignal<QualityRun | null>(null);
  const [checkShown, setCheckShown] = createSignal(false);
  /** What the status line says about a hidden check, or nothing. */
  const checkStatus = () => {
    const run = checking();
    if (!run || checkShown()) return "";
    return run.running()
      ? `checking quality… ${run.done()} of ${run.fileIds.length}`
      : "quality report";
  };
  function checkQuality(ids: number[]) {
    checking()?.stop();
    setChecking(startQuality(ids));
    setCheckShown(true);
  }
  /** Files BPM and key detection is open on, or null when it is closed. */
  const [timing, setTiming] = createSignal<number[] | null>(null);
  /** Files the duplicate finder is measuring, or null when it is closed. */
  const [comparing, setComparing] = createSignal<number[] | null>(null);
  /** Whether the library overview is open. */
  const [surveying, setSurveying] = createSignal(false);
  /** Whether the Playlists dialog is open. */
  const [listing, setListing] = createSignal(false);
  /** Every playlist, for the right-click menu. */
  const [lists, setLists] = createSignal<Playlist[]>([]);
  /** Bumped when the app changes a playlist, so an open dialog reloads. */
  const [listsVersion, setListsVersion] = createSignal(0);
  /** The playlist P adds to: the one last picked in the dialog or made. */
  const [currentList, setCurrentListRaw] = createSignal<number | null>(
    (() => {
      try {
        const v = Number(localStorage.getItem("noceur.playlist"));
        return Number.isFinite(v) && v > 0 ? v : null;
      } catch {
        return null;
      }
    })(),
  );
  function setCurrentList(id: number | null) {
    setCurrentListRaw(id);
    try {
      if (id === null) localStorage.removeItem("noceur.playlist");
      else localStorage.setItem("noceur.playlist", String(id));
    } catch {
      /* remembered for this session only */
    }
  }
  /** Tracks a new playlist is being named for, or null when the prompt is closed. */
  const [newList, setNewList] = createSignal<number[] | null>(null);
  /** Playlists holding the whole right-clicked selection, for the menu's ticks. */
  const [held, setHeld] = createSignal<number[]>([]);
  /** The playlist being played through, when play started from one; next and previous walk it. */
  const [queue, setQueue] = createSignal<RowDisplay[] | null>(null);
  function refreshLists() {
    fetchPlaylists().then(setLists).catch(() => {});
  }
  refreshLists();
  function listsChanged() {
    refreshLists();
    setListsVersion((v) => v + 1);
  }
  /** Adds the tracks to a playlist, or takes them out when all are already in. */
  function toggleInto(id: number, ids: number[]) {
    if (ids.length === 0) return;
    const name = lists().find((l) => l.id === id)?.name ?? "playlist";
    togglePlaylist(id, ids)
      .then((done) => {
        setCurrentList(id);
        listsChanged();
        if (done.removed > 0) flash(`Removed ${done.removed} from ${name}`);
        else flash(`Added ${done.added} to ${name}`);
      })
      .catch((e) => flash(String(e)));
  }
  /** P: into the current playlist, or name a first one. */
  function addToCurrent(ids: number[] = audioSelection()) {
    if (ids.length === 0) return;
    const id = currentList();
    if (id !== null && lists().some((l) => l.id === id)) toggleInto(id, ids);
    else setNewList(ids);
  }
  /** Whether Telegram Desktop is installed. Its button is hidden when not. */
  const [hasTelegram, setHasTelegram] = createSignal(false);
  telegramAvailable().then(setHasTelegram).catch(() => {});
  /** File ids the tag editor is open on, or null when it is closed. */
  const [editing, setEditing] = createSignal<number[] | null>(null);
  /** The row the cover search runs on, plus every file a pick applies to. */
  const [covering, setCovering] = createSignal<{ fileId: number; fileIds: number[] } | null>(
    null,
  );
  /** The row being matched online, and the files a chosen match would edit. */
  const [matching, setMatching] = createSignal<{
    fileId: number;
    fileIds: number[];
    bySound?: boolean;
  } | null>(null);
  /** The match the editor was filled from, for its Back button. */
  const [backTo, setBackTo] = createSignal<{ fileId: number; fileIds: number[] } | null>(null);
  /** What the last search found, so going back does not search again. */
  const [found, setFound] = createSignal<{ fileId: number; bySound?: boolean; result: Matched } | null>(null);
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
  /** The tag a sidebar link asked search to list, until search closes. */
  const [findTag, setFindTag] = createSignal<TagLink | null>(null);
  const [transport, setTransport] = createSignal<TransportState | null>(null);
  /**
   * The row the Time column counts up in: the loaded track, once it has
   * started. A stopped track shows its plain length again.
   */
  const nowPlaying = createMemo(() => {
    const t = transport();
    if (!t?.fileId || !(t.playing || t.positionMs > 0)) return null;
    return { fileId: t.fileId, position: t.position };
  });
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
      case "identify":
        setMatching({ fileId: ids[0], fileIds: ids, bySound: true });
        break;
      case "art":
        setCovering({ fileId: ids[0], fileIds: ids });
        break;
      case "rename":
        setRenaming(ids);
        break;
      case "clean":
        void cleanUp(ids);
        break;
      case "quality":
        checkQuality(ids);
        break;
      case "bpm":
        setTiming(ids);
        break;
      case "copies":
        setComparing(ids);
        break;
      case "convert":
        setConverting(ids);
        break;
      case "fromname":
        setNaming(ids);
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

  // Single settings, each behind a memo so a change elsewhere - a click on the
  // meter, a theme - does not reach them. Read straight from `settings()`,
  // every pane re-listed its folder and reloaded every cover on any change.
  const showMode = createMemo(() => (settings().tracksOnly ? "tracks" : "everything"));
  const folderGrid = createMemo(() => settings().folderGrid);
  const openWithList = createMemo(() => settings().openWith);
  const searchList = createMemo(() => settings().searchIn);
  const storedBar = createMemo(() => settings().bar);
  const sideSites = createMemo(
    () => searchList().filter((s) => s.inSide && s.target.trim()),
    undefined,
    { equals: (a, b) => a.length === b.length && a.every((s, i) => s === b[i]) },
  );

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

  /** Fraction of the playing track already played, for the spectrograms. */
  const played = () => {
    const t = transport();
    return t && t.durationMs > 0 ? (t.positionMs ?? 0) / t.durationMs : 0;
  };
  const seekTo = (ms: number) => transportCmd("seek", ms).then(setTransport).catch(() => {});

  // Measured only while the strip is open — a full decode per track is not
  // something to do for a panel nobody is looking at. Keyed on the file id
  // rather than on the transport, which is replaced ten times a second while
  // something plays.
  createEffect<string>((previous) => {
    // The sidebar carries a small one of its own.
    const want = spekOpen() || sideOpen();
    const id = want ? (transport()?.fileId ?? 0) : 0;
    const key = `${want}:${id}`;
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
  function buttonItems(ids: number[] = audioSelection()): MenuItem[] {
    return buttons().map((action) => ({
      kind: "item" as const,
      label: action.label,
      icon: actionIcon(action.icon),
      disabled: ids.length === 0,
      run: () => runButton(action, ids),
    }));
  }

  /**
   * Clean up: the selected file names down to `Artist - Title`, at once.
   * No dialog — renames are undone with Ctrl+Z, and the note says so.
   */
  async function cleanUp(ids: number[]) {
    if (ids.length === 0) return;
    try {
      const rows = await previewTool(["clean-name"], 0, ids);
      const go = rows.filter((r) => !r.unchanged && !r.problem);
      const skipped = rows.filter((r) => r.problem).length;
      const tail = skipped ? ` · ${skipped} skipped` : "";
      if (go.length === 0) {
        flash(skipped ? `Nothing cleaned${tail}` : "Already clean");
        return;
      }
      const done = await applyCleanup(go.map((r) => ({ fileId: r.fileId, name: r.to, fields: [] })));
      refetchAll();
      const failed = done.failures.length ? ` · ${done.failures.length} failed` : "";
      flash(`Cleaned ${done.written}${failed}${tail} · Ctrl+Z to undo`);
    } catch (e) {
      flash(String(e));
    }
  }

  /**
   * Alt+F2: file names from the tags, as `Artist - Title`, at once.
   * Like Clean up, no dialog — Ctrl+Z puts the names back.
   */
  async function nameFromTags(ids: number[]) {
    if (ids.length === 0) return;
    try {
      const rows = await previewRename(ids, "%artist% - %title%");
      const go = rows.filter((r) => !r.unchanged && !r.problem);
      const skipped = rows.filter((r) => r.problem).length;
      const tail = skipped ? ` · ${skipped} skipped` : "";
      if (go.length === 0) {
        flash(skipped ? `Nothing renamed${tail}` : "Names already match");
        return;
      }
      const done = await applyRename(go.map((r) => [r.fileId, r.to] as [number, string]));
      refetchAll();
      const failed = done.failures.length ? ` · ${done.failures.length} failed` : "";
      flash(`Renamed ${done.written}${failed}${tail} · Ctrl+Z to undo`);
    } catch (e) {
      flash(String(e));
    }
  }

  /**
   * Alt+F1: artist and title from `Artist - Title` file names, at once.
   * Names that do not fit are left alone and counted.
   */
  async function tagsFromName(ids: number[]) {
    if (ids.length === 0) return;
    const pattern = "%artist% - %title%";
    try {
      const rows = await previewFromName(ids, pattern);
      const skipped = rows.filter((r) => r.problem).length;
      const tail = skipped ? ` · ${skipped} skipped` : "";
      if (rows.length === skipped) {
        flash(`Nothing tagged${tail}`);
        return;
      }
      const done = await applyFromName(ids, pattern);
      refetchAll();
      const failed = done.failures.length ? ` · ${done.failures.length} failed` : "";
      flash(`Tagged ${done.written}${failed}${tail}`);
    } catch (e) {
      flash(String(e));
    }
  }

  /** Runs a user-defined button on these files, through the preview if it writes. */
  function runButton(action: Action, ids: number[]) {
    if (actionWrites(action.kind)) {
      setRunning({ action, fileIds: ids });
      return;
    }
    runUserAction(action, ids).catch(() => {});
  }

  /** A short note at the bottom of the window, gone after a moment. */
  const [notice, setNotice] = createSignal("");
  let noticeTimer: number | undefined;
  function flash(text: string) {
    setNotice(text);
    clearTimeout(noticeTimer);
    noticeTimer = window.setTimeout(() => setNotice(""), 2500);
  }

  /** Whether the top bar is being arranged. */
  const [barEditing, setBarEditing] = createSignal(false);
  const [barMenu, setBarMenu] = createSignal<{ x: number; y: number; group?: string } | null>(null);
  const [paletteOpen, setPaletteOpen] = createSignal(false);

  /**
   * Everything that can sit in the top bar. Settings is not among them: it is
   * always there, at the far end, so the bar can never be arranged into a state
   * with no way back to the settings.
   */
  const barItems = createMemo<BarItem[]>(() => {
    const none = () => audioSelection().length === 0;
    const tool = (
      id: ActionId,
      label: string,
      icon: () => JSX.Element,
      group: GroupId,
    ): BarItem => ({
      id,
      label,
      icon,
      group,
      off: id === "quick" ? undefined : none,
      on: id === "quick" ? quickOpen : undefined,
      run: () => runAction(id),
    });
    const list: BarItem[] = [
      tool("tags", "Edit metadata", TagIcon, "tags"),
      tool("match", "Find metadata online", GlobeIcon, "tags"),
      tool("identify", "Identify by sound", ListenIcon, "tags"),
      tool("fromname", "Tags from file name", NameToTagIcon, "tags"),
      tool("art", "Find album art", ImageIcon, "tags"),
      tool("quick", "Quick tag", StarIcon, "tags"),
      tool("rename", "Rename", RenameIcon, "fix"),
      tool("clean", "Clean up names", BroomIcon, "fix"),
      {
        id: "functions",
        label: "Batch fixes",
        icon: BoltIcon,
        group: "fix",
        off: none,
        run: () => setTooling(audioSelection()),
      },
      // Your own buttons, with the fixes: they are what you wrote to do to
      // the selection.
      ...buttons().map<BarItem>((action) => ({
        id: `user:${action.id}`,
        label: action.label,
        icon: actionIcon(action.icon),
        group: "fix",
        off: none,
        run: () => runButton(action, audioSelection()),
      })),
      tool("quality", "Check quality", SpectrumIcon, "analyse"),
      tool("bpm", "Detect BPM & key", TempoIcon, "analyse"),
      tool("copies", "Find duplicates", CopiesIcon, "analyse"),
      {
        id: "spek",
        label: "Spek",
        icon: PigIcon,
        group: "analyse",
        on: spekOpen,
        run: () => setSpekOpen((open) => !open),
      },
      tool("convert", "Convert", ConvertIcon, "files"),
      // Your programs and sites. In the bar only when switched on for it.
      ...openWithList().map<BarItem>((program) => ({
        id: `open:${program.id}`,
        label: `Open in ${program.name}`,
        icon: programIcon(program.target),
        group: "send",
        forced: program.inBar,
        off: () => (focusHandle()?.selection().length ?? 0) === 0,
        onToggle: (on) => setInBar("openWith", program.id, on),
        run: () => launch(program),
      })),
      ...searchList().map<BarItem>((site) => ({
        id: `search:${site.id}`,
        label: `Search ${site.name}`,
        icon: siteIcon(site.target),
        group: "send",
        forced: site.inBar,
        off: none,
        onToggle: (on) => setInBar("searchIn", site.id, on),
        run: () => searchIn(site),
      })),
      ...(hasTelegram()
        ? [
            {
              id: "telegram",
              label: "Send to Telegram",
              icon: SendIcon,
              group: "send",
              off: () => (focusHandle()?.selection().length ?? 0) === 0,
              run: () => toTelegram(),
            } as BarItem,
          ]
        : []),
      {
        id: "newfolder",
        label: "New folder",
        icon: NewFolderIcon,
        group: "files",
        run: () => setNewFolderIn(focusPath()),
      },
      {
        id: "paste",
        label: "Paste",
        icon: ClipboardIcon,
        group: "files",
        off: () => !clip(),
        run: () => void paste(),
      },
      {
        id: "palette",
        label: "Command palette · Ctrl+K",
        icon: CommandIcon,
        run: () => setPaletteOpen(true),
      },
      {
        id: "playlists",
        label: "Playlists",
        icon: PlaylistIcon,
        run: () => setListing(true),
      },
      {
        id: "overview",
        label: "Library overview",
        icon: ChartIcon,
        run: () => setSurveying(true),
      },
      {
        id: "shortcuts",
        label: "Keyboard shortcuts",
        icon: KeyboardIcon,
        auto: false,
        run: () => setShowKeys(true),
      },
      {
        id: "sizes",
        label: "Cover size",
        render: () => (
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
        ),
      },
      {
        id: "split",
        label: "Dual pane",
        icon: SplitColsIcon,
        on: () => layout.groups.length > 1,
        run: splitOrJoin,
      },
      {
        id: "splitdir",
        label: "Split direction",
        icon: SplitRowsIcon,
        absent: () => layout.groups.length < 2,
        run: flipSplit,
      },
    ];

    // The group cells, each holding the tools tagged with it.
    const groupIcon: Record<GroupId, () => JSX.Element> = {
      tags: TagIcon,
      fix: RenameIcon,
      analyse: SpectrumIcon,
      files: FolderIcon,
      send: SendIcon,
    };
    for (const g of GROUPS) {
      const members = () => list.filter((i) => i.group === g.id);
      list.push({
        id: `group:${g.id}`,
        label: g.label,
        icon: groupIcon[g.id],
        members,
        lead: () => [
          ...leadOf(g.id),
          ...members()
            .filter((m) => m.forced)
            .map((m) => m.id),
        ],
        onLead: (id) => toggleLead(g.id, id, members()),
      });
    }
    return list;
  });

  /** The tools a group shows as icons, as chosen or by default. */
  function leadOf(group: GroupId): string[] {
    const bar = storedBar();
    const chosen = bar?.v === BAR_VERSION ? bar.lead?.[group] : undefined;
    return chosen ?? DEFAULT_LEAD[group];
  }

  /**
   * Shows or hides one tool as an icon beside its group's arrow. A program or
   * site has its own toolbar switch in Settings, which this flips instead so
   * the two never disagree.
   */
  function toggleLead(group: GroupId, id: string, members: BarItem[]) {
    const item = members.find((m) => m.id === id);
    if (item?.onToggle) return item.onToggle(!item.forced);
    const now = leadOf(group);
    const next = now.includes(id) ? now.filter((x) => x !== id) : [...now, id];
    // Kept in the group's own order, so icons do not shuffle as they are ticked.
    const ordered = members.map((m) => m.id).filter((m) => next.includes(m));
    const layout = barLayout();
    commitSettings({ ...settings(), bar: { ...layout, lead: { ...layout.lead, [group]: ordered } } });
  }

  /* ── Open with, Search in ─────────────────────────────────────────────── */

  /** Opens the selection (or `ids`) in one of your programs. */
  /** Hands the selected files to Telegram's chat picker. */
  function toTelegram(ids: number[] = focusHandle()?.selection() ?? []) {
    if (ids.length === 0) return;
    sendToTelegram(ids).catch((e) => flash(String(e)));
  }

  function launch(program: Launcher, ids: number[] = focusHandle()?.selection() ?? []) {
    if (ids.length === 0) return;
    openWith(program.target, ids).catch(() => {});
  }

  /**
   * Searches a site for the selected tracks. One browser tab per track, capped
   * at five - a slip of the mouse over a whole folder should not open forty.
   */
  function searchIn(site: Launcher, ids: number[] = focusHandle()?.selection() ?? []) {
    const handle = focusHandle();
    for (const id of ids.slice(0, 5)) {
      const row = handle?.rowOf(id);
      if (!row || row.kind === 0) continue;
      openUrl(searchUrl(site.target, row)).catch(() => {});
    }
  }

  /** Turns one entry's toolbar switch on or off. */
  function setInBar(list: "openWith" | "searchIn", id: string, inBar: boolean) {
    const s = settings();
    commitSettings({ ...s, [list]: s[list].map((l) => (l.id === id ? { ...l, inBar } : l)) });
  }

  /** Opens settings on one tab - the "Add…" at the foot of each submenu. */
  function settingsAt(tab: "actions") {
    setSettingsTab(tab);
    setSettingsOpen(true);
  }

  /**
   * The out-of-the-box arrangement: the five groups, then the view switches.
   * Your own buttons live in Fix; tick them there to give them an icon.
   */
  const barDefaults = () => [
    "flex:start",
    "group:tags",
    "sep:a",
    "group:fix",
    "sep:b",
    "group:analyse",
    "sep:c",
    "group:files",
    "sep:d",
    "group:send",
    "sep:e",
    "sizes",
    "sep:f",
    "split", "splitdir",
    "sep:g",
    "playlists", "overview", "palette",
  ];

  const barLayout = createMemo(() => resolveBar(storedBar(), barItems(), barDefaults()));

  /** Loads a row into the player, and remembers which tab it is walking. */
  function playRow(row: RowDisplay, tabId: number) {
    if (row.kind !== 1) return;
    setQueue(null);
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
    const list = queue();
    const at = list?.findIndex((r) => r.fileId === from) ?? -1;
    if (list && at >= 0) {
      const next = list[at + by];
      if (next) play(next.fileId).then(setTransport).catch(() => {});
      return;
    }
    const tabId = playingTab();
    const handle = (tabId !== null ? handles()[tabId] : undefined) ?? focusHandle();
    if (!handle) return;
    const row = handle.step(from, by);
    if (row) playRow(row, handle.tabId);
  }

  /* ── Sidebar ───────────────────────────────────────────────────────────── */

  // Closed at every start: opening it (Ctrl+B) lasts until the app closes.
  const [sideOpen, setSideOpen] = createSignal(false);
  function toggleSide() {
    setSideOpen(!sideOpen());
  }

  /** The focused pane's current row, with the tab it is in. */
  const sideRow = createMemo(
    () => {
      const handle = focusHandle();
      const id = handle?.current() ?? null;
      if (!handle || id === null) return null;
      return { tabId: handle.tabId, fileId: id };
    },
    null,
    { equals: (a, b) => a?.tabId === b?.tabId && a?.fileId === b?.fileId },
  );

  /** The track the sidebar describes: the current row, when it is audio. */
  const sideTrack = () => {
    const at = sideRow();
    if (!at) return null;
    const row = handles()[at.tabId]?.rowOf(at.fileId);
    return row && row.kind !== 1 ? null : at.fileId;
  };

  // With the sidebar open, selecting a track plays it, so going down the list
  // is listening through it. Only a change of row within one pane counts:
  // opening the sidebar, or moving focus to the other pane, plays nothing.
  createEffect(
    on(sideRow, (now, before) => {
      if (!sideOpen() || !now || !before || now.tabId !== before.tabId) return;
      if (now.fileId === before.fileId || transport()?.fileId === now.fileId) return;
      const row = handles()[now.tabId]?.rowOf(now.fileId);
      if (row) playRow(row, now.tabId);
    }),
  );

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
    setHeld([]);
    playlistsHolding(ids).then(setHeld).catch(() => {});
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

    // In the toolbar's order and under its group names - Tags, Fix, Analyse,
    // Files, Send - with its icons, so the same action is in the same place and
    // looks the same wherever it is reached from. Playing and finding the file
    // come first, ungrouped, because they are what a right-click is most often for.
    return [
      {
        kind: "item",
        label: "Play",
        hint: "double-click",
        icon: PlayIcon,
        disabled: audio,
        run: () => playRow(row, playingTab() ?? (activeTab(focusGroup())?.id ?? 0)),
      },
      {
        kind: "item",
        label: "Playlists",
        icon: PlaylistIcon,
        disabled: audio,
        run: () => {},
        children: [
          ...lists().map<MenuItem>((list) => ({
            kind: "item",
            label: list.name,
            checked: held().includes(list.id),
            hint: list.id === currentList() ? "p" : undefined,
            run: () => toggleInto(list.id, ids),
          })),
          ...(lists().length ? [{ kind: "separator" } as MenuItem] : []),
          { kind: "item", label: "New playlist…", run: () => setNewList(ids) },
          { kind: "item", label: "Open playlists", hint: "ctrl+p", run: () => setListing(true) },
        ],
      },
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
        label: "Reveal in Explorer",
        icon: OpenIcon,
        run: () => {
          reveal(row.fileId).catch(() => {});
        },
      },

      { kind: "header", label: "Tags" },
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
        label: "Identify by sound…",
        icon: ListenIcon,
        disabled: audio,
        // For a file whose name and tags may be wrong: Shazam listens to it.
        run: () => setMatching({ fileId: row.fileId, fileIds: ids, bySound: true }),
      },
      {
        kind: "item",
        label: many ? `Tags from file name for ${ids.length} tracks…` : "Tags from file name…",
        icon: NameToTagIcon,
        disabled: audio,
        run: () => setNaming(ids),
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

      { kind: "header", label: "Fix" },
      {
        kind: "item",
        label: many ? `Rename ${ids.length} tracks…` : "Rename…",
        icon: RenameIcon,
        disabled: audio,
        run: () => setRenaming(ids),
      },
      {
        kind: "item",
        label: many ? `Clean up ${ids.length} names` : "Clean up name",
        icon: BroomIcon,
        disabled: audio,
        run: () => void cleanUp(ids),
      },
      {
        kind: "item",
        label: "Batch fixes…",
        icon: BoltIcon,
        disabled: audio,
        run: () => setTooling(ids),
      },
      ...(buttons().length
        ? ([
            {
              kind: "item",
              label: "Your actions",
              icon: BoltIcon,
              disabled: audio,
              run: () => {},
              children: buttonItems(ids),
            },
          ] as MenuItem[])
        : []),

      { kind: "header", label: "Analyse" },
      {
        kind: "item",
        label: many ? `Check quality of ${ids.length} tracks…` : "Check quality…",
        icon: SpectrumIcon,
        disabled: audio,
        run: () => checkQuality(ids),
      },
      {
        kind: "item",
        label: many ? `Detect BPM & key of ${ids.length} tracks…` : "Detect BPM & key…",
        icon: TempoIcon,
        disabled: audio,
        run: () => setTiming(ids),
      },
      {
        kind: "item",
        label: many ? `Find duplicates of ${ids.length} tracks…` : "Find duplicates…",
        icon: CopiesIcon,
        disabled: audio,
        run: () => setComparing(ids),
      },

      { kind: "header", label: "Files" },
      {
        kind: "item",
        label: many ? `Convert ${ids.length} tracks…` : "Convert…",
        icon: ConvertIcon,
        disabled: audio,
        run: () => setConverting(ids),
      },
      {
        kind: "item",
        label: "Copy",
        hint: "ctrl+c",
        icon: CopiesIcon,
        run: () => void clipFiles(false, ids),
      },
      {
        kind: "item",
        label: "Cut",
        hint: "ctrl+x",
        icon: MoveIcon,
        run: () => void clipFiles(true, ids),
      },
      {
        kind: "item",
        // On a folder, into that folder - as Explorer does it.
        label: row.kind === 0 ? `Paste into ${row.name}` : "Paste",
        hint: "ctrl+v",
        icon: ClipboardIcon,
        disabled: !clip(),
        run: () => {
          const here = focusPath();
          const sep = here.endsWith("\\") || here.endsWith("/") ? "" : "\\";
          void paste(row.kind === 0 ? `${here}${sep}${row.name}` : here);
        },
      },
      ...(otherFolder()
        ? ([
            {
              kind: "item",
              label: "Copy to other pane",
              run: () => void toOtherPane("copy", ids),
            },
            {
              kind: "item",
              label: "Move to other pane",
              run: () => void toOtherPane("move", ids),
            },
          ] as MenuItem[])
        : []),
      {
        kind: "item",
        label: "New folder…",
        hint: "ctrl+shift+n",
        icon: NewFolderIcon,
        run: () => setNewFolderIn(focusPath()),
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

      { kind: "header", label: "Send" },
      {
        kind: "item",
        label: "Open with",
        icon: OpenWithIcon,
        run: () => {},
        children: [
          ...settings().openWith.map<MenuItem>((program) => ({
            kind: "item",
            label: program.name,
            icon: programIcon(program.target),
            run: () => launch(program, ids),
          })),
          ...(settings().openWith.length ? [{ kind: "separator" } as MenuItem] : []),
          { kind: "item", label: "Add program…", run: () => settingsAt("actions") },
        ],
      },
      {
        kind: "item",
        label: "Search in",
        icon: SearchIcon,
        disabled: audio || lookup === "",
        run: () => {},
        children: [
          ...settings().searchIn.map<MenuItem>((site) => ({
            kind: "item",
            label: site.name,
            icon: siteIcon(site.target),
            run: () => searchIn(site, ids),
          })),
          ...(settings().searchIn.length ? [{ kind: "separator" } as MenuItem] : []),
          { kind: "item", label: "Add site…", run: () => settingsAt("actions") },
        ],
      },
      ...(hasTelegram()
        ? ([
            {
              kind: "item",
              label: "Send to Telegram",
              icon: SendIcon,
              disabled: row.kind === 0,
              run: () => toTelegram(ids),
            },
          ] as MenuItem[])
        : []),

      { kind: "separator" },
      {
        kind: "item",
        label: many ? `Delete ${ids.length} items` : "Delete",
        hint: "del",
        icon: TrashIcon,
        danger: true,
        run: deleteSelection,
      },
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

      // Alt+F1 tags files from their names, Alt+F2 names them from their tags,
      // Alt+F3 finds metadata — the three converters a folder goes through.
      if (e.altKey && !mod && !e.shiftKey && ["F1", "F2", "F3"].includes(e.key)) {
        e.preventDefault();
        const ids = audioSelection();
        if (ids.length === 0) return;
        if (e.key === "F1") void tagsFromName(ids);
        else if (e.key === "F2") void nameFromTags(ids);
        else runAction("match", ids);
        return;
      }

      // F1, or ? outside a text box, lists every shortcut.
      if (e.key === "F1" || (e.key === "?" && !mod && !isTyping(e))) {
        e.preventDefault();
        setShowKeys(true);
        return;
      }

      if (mod && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setFinding(true);
        return;
      }

      // Ctrl+K: every action, by name.
      if (mod && !e.shiftKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((open) => !open);
        return;
      }

      // Ctrl+P opens the playlists; the web view would otherwise print.
      if (mod && !e.shiftKey && e.key.toLowerCase() === "p") {
        e.preventDefault();
        setListing((open) => !open);
        return;
      }

      if (mod && !e.shiftKey && e.key.toLowerCase() === "i") {
        e.preventDefault();
        setSurveying((open) => !open);
        return;
      }

      if (mod && !e.shiftKey && e.key.toLowerCase() === "b") {
        e.preventDefault();
        toggleSide();
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
      // Copy, cut and paste files, as in Explorer. Only outside text boxes,
      // where these keys belong to the text.
      if (mod && !e.shiftKey && !isTyping(e) && ["c", "x", "v"].includes(e.key.toLowerCase())) {
        e.preventDefault();
        const k = e.key.toLowerCase();
        if (k === "v") void paste();
        else void clipFiles(k === "x");
        return;
      }
      // Ctrl+Z puts the last rename back — a rename job, a button, a cleanup or
      // an edit in the list. Text boxes keep their own undo.
      if (mod && !e.shiftKey && e.key.toLowerCase() === "z" && !isTyping(e)) {
        e.preventDefault();
        undoRename()
          .then((done) => {
            if (done.restored === 0 && done.failed === 0) return;
            refetchAll();
            flash(
              done.failed > 0
                ? `Undo: ${done.restored} restored · ${done.failed} failed`
                : `Undo: ${done.restored} renamed back`,
            );
          })
          .catch(() => {});
        return;
      }
      // Ctrl+A selects the whole folder in the focused pane. Text boxes keep
      // their own select-all.
      if (mod && !e.shiftKey && e.key.toLowerCase() === "a" && !isTyping(e)) {
        e.preventDefault();
        focusHandle()?.selectAll();
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

      // Moving through the list: arrows, Home / End, Page Up / Down, Enter,
      // Backspace. Not while a dialog sits over it.
      if (!document.querySelector(".scrim") && focusHandle()?.key(e)) {
        e.preventDefault();
        return;
      }

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

      // P puts the selection in the current playlist, or takes it out again.
      // Free in quick tag too, whose letter row stops at O.
      if (e.key.toLowerCase() === "p" && !document.querySelector(".scrim")) {
        e.preventDefault();
        addToCurrent();
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

  /**
   * Walks the pinned folders in the background so search finds files in folders
   * never opened. Once per set of folders: at launch, and again only when search
   * opens over somewhere the last walk did not cover.
   */
  let walked = "";
  createEffect(() => {
    if (!finding() && walked !== "") return;
    const roots = untrack(searchScopes);
    const key = roots.join("|");
    if (key === walked) return;
    walked = key;
    indexFolders(roots).catch(() => {});
  });

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

      <div
        class="topbar"
        onContextMenu={(e) => {
          e.preventDefault();
          const group = (e.target as HTMLElement).closest<HTMLElement>("[data-group]")?.dataset.group;
          if (!barEditing()) setBarMenu({ x: e.clientX, y: e.clientY, group });
        }}
      >
        <Toolbar
          items={barItems()}
          layout={barLayout()}
          editing={barEditing()}
          onLayout={(bar) => commitSettings({ ...settings(), bar })}
          onReset={() => commitSettings({ ...settings(), bar: null })}
          onDone={() => setBarEditing(false)}
        />
        <span class="tsep fixed" />
        {/* Fixed rather than a toolbar item, like Settings: search is how the
            library is reached, and a layout saved before it existed would
            otherwise never show it. */}
        <button
          class={`searchbox ${finding() ? "on" : ""}`}
          data-tip="Ctrl+F"
          aria-label="Search"
          onClick={() => setFinding(true)}
        >
          <SearchIcon />
          Search
        </button>
        <button
          class={`toolbtn ${settingsOpen() ? "on" : ""}`}
          data-tip="Settings"
          data-tip-at="end"
          aria-label="Settings"
          onClick={() => setSettingsOpen((open) => !open)}
        >
          <SettingsIcon />
        </button>
      </div>

      <Show when={notice()}>
        <div class="notice" role="status">
          {notice()}
        </div>
      </Show>

      <Show when={paletteOpen()}>
        <Palette
          items={barItems()}
          extra={[
            { id: "x:settings", label: "Settings", icon: SettingsIcon, run: () => setSettingsOpen(true) },
            { id: "x:search", label: "Search library", icon: SearchIcon, run: () => setFinding(true) },
            { id: "x:keys", label: "Keyboard shortcuts", icon: KeyboardIcon, run: () => setShowKeys(true) },
            { id: "x:bar", label: "Customize toolbar", icon: SettingsIcon, run: () => setBarEditing(true) },
          ]}
          onClose={() => setPaletteOpen(false)}
        />
      </Show>

      <Show when={barMenu()}>
        {(m) => (
          <Menu
            x={m().x}
            y={m().y}
            onClose={() => setBarMenu(null)}
            items={[
              // On a group: a tick per tool, for which ones show as icons.
              ...(() => {
                const group = barItems().find((i) => i.id === m().group);
                if (!group?.members) return [];
                const lead = group.lead?.() ?? [];
                const out = barLayout().out ?? [];
                const tools = group.members().filter((t) => !t.absent?.());
                const kept = tools.filter((t) => !out.includes(t.id));
                const gone = tools.filter((t) => out.includes(t.id));
                return [
                  ...kept.map<MenuItem>((t) => ({
                    kind: "item",
                    label: t.label,
                    checked: lead.includes(t.id),
                    run: () => group.onLead?.(t.id),
                  })),
                  // Taken out of the group; one click puts it back in the list.
                  ...(gone.length
                    ? [
                        { kind: "header", label: "Removed" } as MenuItem,
                        ...gone.map<MenuItem>((t) => ({
                          kind: "item",
                          label: `Bring back ${t.label}`,
                          run: () => {
                            const layout = barLayout();
                            commitSettings({
                              ...settings(),
                              bar: { ...layout, out: (layout.out ?? []).filter((x) => x !== t.id) },
                            });
                          },
                        })),
                      ]
                    : []),
                  { kind: "separator" } as MenuItem,
                ];
              })(),
              { kind: "item", label: "Command palette", hint: "ctrl+k", run: () => setPaletteOpen(true) },
              { kind: "item", label: "Customize toolbar…", run: () => setBarEditing(true) },
              { kind: "item", label: "Reset toolbar", run: () => commitSettings({ ...settings(), bar: null }) },
              { kind: "item", label: "Keyboard shortcuts", run: () => setShowKeys(true) },
            ]}
          />
        )}
      </Show>

      <div class="body">
        {/* The tree and the panes, with the spectrogram under them: it stops at
            the sidebar, which keeps the window's full height for its fields. */}
        <div class="main">
        <div class="browse">
        <Tree
          current={focusPath()}
          onNavigate={(path) => navigateFocused(path)}
          dropKey={dropOver()?.key ?? null}
          onNewFolder={setNewFolderIn}
          onOpenTab={(path) => openTab(focusGroup().id, path)}
          onDelete={deleteFolder}
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
                        show={showMode()}
                        folderGrid={folderGrid()}
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
                        nowPlaying={nowPlaying()}
                        dropKey={dropOver()?.key ?? null}
                        checkStatus={layout.focus === group.id ? checkStatus() : ""}
                        onCheckOpen={() => setCheckShown(true)}
                      />
                    )}
                  </Show>
                </div>
              </>
            )}
          </For>
        </div>
        </div>

        <Show when={spekOpen()}>
          <Spectrogram
            grid={spek()}
            progress={played()}
            sampleRate={transport()?.sampleRate ?? 0}
            durationMs={transport()?.durationMs ?? 0}
            name={transport()?.path ?? ""}
            onSeek={seekTo}
          />
        </Show>
        </div>

        <button
          class={`sidetoggle ${sideOpen() ? "open" : ""}`}
          onClick={toggleSide}
          data-tip={sideOpen() ? "Hide sidebar · ctrl+b" : "Sidebar · ctrl+b"}
          data-tip-at="start"
          aria-label="Sidebar"
        >
          <ChevronIcon />
        </button>
        <Show when={sideOpen()}>
          <Sidebar
            fileId={sideTrack()}
            sites={sideSites()}
            onSites={() => settingsAt("actions")}
            version={written() + artNonce()}
            onWritten={refetchAll}
            onArt={(fileId) => setEditing([fileId])}
            onList={(link) => {
              setFindTag(link);
              setFinding(true);
            }}
          >
            <Show when={!spekOpen() && (transport()?.fileId ?? 0) > 0}>
              <Spectrogram
                compact
                grid={spek()}
                progress={played()}
                sampleRate={transport()?.sampleRate ?? 0}
                durationMs={transport()?.durationMs ?? 0}
                name={transport()?.path ?? ""}
                onSeek={seekTo}
                onExpand={() => setSpekOpen(true)}
              />
            </Show>
          </Sidebar>
        </Show>
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

      <Show when={tooling()}>
        {(ids) => (
          <Tools
            fileIds={ids()}
            onClose={() => setTooling(null)}
            onDone={refetchAll}
          />
        )}
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
              {
                kind: "item",
                label: "Paste",
                hint: "ctrl+v",
                icon: ClipboardIcon,
                disabled: !clip(),
                run: () => void paste(m().path),
              },
            ]}
          />
        )}
      </Show>

      <Show when={showKeys()}>
        <Shortcuts onClose={() => setShowKeys(false)} />
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

      <Show when={naming()}>
        {(ids) => (
          <FromName fileIds={ids()} onClose={() => setNaming(null)} onDone={refetchAll} />
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

      <Show when={checkShown() && checking()}>
        {(run) => (
          <Quality
            run={run()}
            onHide={() => setCheckShown(false)}
            onClose={() => {
              run().stop();
              setChecking(null);
              setCheckShown(false);
            }}
            onSelect={(picked) => {
              focusHandle()?.select(picked);
              setCheckShown(false);
            }}
          />
        )}
      </Show>

      <Show when={timing()}>
        {(ids) => (
          <Analysis fileIds={ids()} onClose={() => setTiming(null)} onDone={refetchAll} />
        )}
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

      <Show when={listing()}>
        <Playlists
          current={currentList()}
          onCurrent={setCurrentList}
          playing={transport()?.fileId ?? null}
          version={listsVersion()}
          onChanged={refreshLists}
          onPlay={(row, list) => {
            setQueue(list);
            play(row.fileId).then(setTransport).catch(() => {});
          }}
          onClose={() => setListing(false)}
        />
      </Show>

      <Show when={newList()}>
        {(ids) => (
          <NewPlaylist
            fileIds={ids()}
            onClose={() => setNewList(null)}
            onCreated={(id, name, added) => {
              setCurrentList(id);
              listsChanged();
              flash(added > 0 ? `Added ${added} to ${name}` : `Made ${name}`);
            }}
          />
        )}
      </Show>

      <Show when={surveying()}>
        <Overview
          folder={focusPath()}
          onOpen={(path) => {
            navigateFocused(path);
            setSurveying(false);
          }}
          onClose={() => setSurveying(false)}
        />
      </Show>

      <Show when={matching()}>
        {(target) => (
          <MatchPicker
            fileId={target().fileId}
            bySound={target().bySound}
            cached={
              found()?.fileId === target().fileId && !!found()!.bySound === !!target().bySound
                ? found()!.result
                : undefined
            }
            onLoaded={(result) =>
              setFound({ fileId: target().fileId, bySound: target().bySound, result })
            }
            onClose={() => {
              setMatching(null);
              setFound(null);
            }}
            onChoose={(fields, art) => {
              setBackTo(target());
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
            onBack={
              backTo()
                ? () => {
                    const target = backTo();
                    setEditing(null);
                    setPrefill({});
                    setPrefillArt(undefined);
                    setBackTo(null);
                    setMatching(target);
                  }
                : undefined
            }
            onClose={() => {
              setEditing(null);
              setPrefill({});
              setPrefillArt(undefined);
              setBackTo(null);
              setFound(null);
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
          tag={findTag() ?? undefined}
          onClose={() => {
            setFinding(false);
            setFindTag(null);
          }}
          // Opened in the focused pane, with the row that matched selected
          // once the folder holding it has been read.
          onOpen={(dirPath, fileId) => navigateFocused(dirPath, fileId)}
        />
      </Show>
    </div>
  );
}
