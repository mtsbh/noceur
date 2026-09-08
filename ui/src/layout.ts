/**
 * Tabs and panes: what the window is showing, and where.
 *
 * A **tab** is one place you are, with the history that got you there. A
 * **group** is a stack of tabs sharing one slot on screen, of which one is
 * visible. The window holds one group, or two side by side — the split.
 *
 * Two groups and no more. An arbitrary tree of splits is a thing a text editor
 * needs, because a text editor is where you compare four files at once; a file
 * browser is where you move tracks from one folder to another, and the second
 * pane is the other folder. Beyond two the panes are narrower than a filename
 * and the feature is being had rather than used.
 *
 * Everything here is plain data. The pane component owns what it is *showing* —
 * loaded windows of rows, the virtualizer, the scroll container — and none of
 * that survives a tab switch or belongs in storage. What survives is here.
 */

import type { Order, SortColumn } from "./api";

/** One place you are, with the history that got you there. */
export type Tab = {
  /** Stable for the life of the tab. Identity for keying, not an index. */
  id: number;
  /** Every path visited in this tab. `cursor` points at the current one. */
  history: string[];
  cursor: number;
  /**
   * Sort order, per tab rather than global.
   *
   * A split exists to look at two folders at once, and the reason to look at
   * two folders at once is usually that they are different — one sorted by BPM
   * against one sorted by date. A sort that reached across the divider would
   * make the second pane a copy of the first with different rows in it.
   */
  order: Order;
  /** Selected file ids, so switching away and back does not lose the selection. */
  selection: number[];
  /** The row shift-select extends from, or null. */
  anchor: number | null;
  /** Scroll offset in pixels, restored when the tab comes back. */
  scroll: number;
};

/** A stack of tabs sharing one slot on screen. */
export type Group = {
  id: number;
  tabs: Tab[];
  /** Which tab is visible, by id — not by index, which reordering would move. */
  active: number;
};

/** The whole window: one group, or two and the divider between them. */
export type Layout = {
  groups: Group[];
  /** How the split is oriented. `row` is side by side, `column` is stacked. */
  dir: "row" | "column";
  /** The first group's share of the split, 0.15–0.85. */
  ratio: number;
  /** Which group the tree, the toolbar and the keyboard are talking to. */
  focus: number;
};

/** The path a tab is currently showing. */
export function currentPath(tab: Tab): string {
  return tab.history[tab.cursor];
}

/**
 * Ids are handed out from a counter rather than derived from anything.
 *
 * A tab's identity has to survive being reordered, moved to the other pane and
 * having its path changed underneath it, so it cannot be the path, the index or
 * anything else that a user action can change.
 */
let nextId = 1;

export function freshId(): number {
  return nextId++;
}

/** A tab at a path, with everything else at its default. */
export function newTab(path: string, order: Order): Tab {
  return {
    id: freshId(),
    history: [path],
    cursor: 0,
    order: { ...order },
    selection: [],
    anchor: null,
    scroll: 0,
  };
}

export function newGroup(tabs: Tab[]): Group {
  return { id: freshId(), tabs, active: tabs[0].id };
}

/** Keeps a dragged divider usable: neither pane can be squeezed out of existence. */
export function clampRatio(value: number): number {
  return Math.min(0.85, Math.max(0.15, value));
}

const SORT_KEY = "noceur.sort";

/**
 * The sort a new tab starts with.
 *
 * The order itself lives on the tab, because a split exists to compare two
 * folders and they are rarely worth comparing in the same order. This is only
 * the seed — the last sort anybody chose, so opening a tab does not throw away
 * the answer to "how do you like your listings".
 */
export function defaultOrder(): Order {
  try {
    const raw = localStorage.getItem(SORT_KEY);
    if (raw) return validOrder(JSON.parse(raw), FALLBACK_ORDER);
  } catch {
    // A corrupt preference is not worth failing over; fall through.
  }
  return { ...FALLBACK_ORDER };
}

/** Remembers a sort as the seed for the next new tab. */
export function rememberOrder(order: Order) {
  try {
    localStorage.setItem(SORT_KEY, JSON.stringify(order));
  } catch {
    // See `saveLayout`: losing a preference is not worth an interruption.
  }
}

const FALLBACK_ORDER: Order = { sort: "name", desc: false };

const LAYOUT_KEY = "noceur.layout";

/** What actually goes to storage: paths and sorts, never runtime state. */
type StoredLayout = {
  groups: { tabs: { path: string; order: Order }[]; active: number }[];
  dir: "row" | "column";
  ratio: number;
  focus: number;
};

/**
 * Saves where the window was.
 *
 * Called both from an effect, which re-runs whenever the tabs or the split
 * change, and by hand at the end of a divider drag.
 *
 * Selections and scroll offsets are deliberately not written. They describe a
 * session rather than a workspace, and restoring a selection made before a
 * restart would be restoring an opinion about files that may no longer be
 * there.
 */
export function saveLayout(layout: Layout, ratio: number) {
  const stored: StoredLayout = {
    groups: layout.groups.map((g) => ({
      tabs: g.tabs.map((t) => ({ path: currentPath(t), order: t.order })),
      // Written as an index, because ids are per-run and would mean nothing to
      // the next one.
      active: Math.max(
        0,
        g.tabs.findIndex((t) => t.id === g.active),
      ),
    })),
    dir: layout.dir,
    // Passed in rather than read off `layout`, so the caller decides whether
    // reading it subscribes to it. A divider drag writes the ratio sixty times a
    // second, and a save behind each one would be sixty JSON writes.
    ratio: clampRatio(ratio),
    focus: Math.max(
      0,
      layout.groups.findIndex((g) => g.id === layout.focus),
    ),
  };
  try {
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(stored));
  } catch {
    // Storage being full or unavailable loses the layout, which is not worth
    // interrupting anybody over.
  }
}

const SORTABLE: SortColumn[] = [
  "name",
  "bpm",
  "key",
  "duration",
  "artist",
  "title",
  "album",
  "albumArtist",
  "genre",
  "year",
  "trackNo",
  "comment",
  "isrc",
  "sampleRate",
  "bitDepth",
  "codec",
  "size",
  "modified",
];

function validOrder(value: unknown, fallback: Order): Order {
  const o = value as Order | undefined;
  if (!o || typeof o.desc !== "boolean" || !SORTABLE.includes(o.sort)) return { ...fallback };
  return { sort: o.sort, desc: o.desc };
}

/**
 * Reads the saved layout, or builds a single tab at `home`.
 *
 * Every field is checked rather than trusted. This is JSON a previous version
 * wrote, and a layout that half-parses would leave the window with a pane
 * showing nothing and no way to say so.
 */
export function loadLayout(home: string, order: Order): Layout {
  const fallback = (): Layout => {
    const group = newGroup([newTab(home, order)]);
    return { groups: [group], dir: "row", ratio: 0.5, focus: group.id };
  };

  let raw: string | null = null;
  try {
    raw = localStorage.getItem(LAYOUT_KEY);
  } catch {
    return fallback();
  }
  if (!raw) return fallback();

  try {
    const stored = JSON.parse(raw) as StoredLayout;
    if (!Array.isArray(stored.groups) || stored.groups.length === 0) return fallback();

    const groups: Group[] = [];
    // Two at most, however many a future version or a hand-edited preference
    // put in the file.
    for (const g of stored.groups.slice(0, 2)) {
      const paths = Array.isArray(g.tabs) ? g.tabs : [];
      const tabs = paths
        .filter((t) => t && typeof t.path === "string" && t.path !== "")
        .map((t) => newTab(t.path, validOrder(t.order, order)));
      if (tabs.length === 0) continue;
      const at = Number.isInteger(g.active) ? Math.min(Math.max(g.active, 0), tabs.length - 1) : 0;
      groups.push({ id: freshId(), tabs, active: tabs[at].id });
    }
    if (groups.length === 0) return fallback();

    const focus = Number.isInteger(stored.focus)
      ? Math.min(Math.max(stored.focus, 0), groups.length - 1)
      : 0;

    return {
      groups,
      dir: stored.dir === "column" ? "column" : "row",
      ratio: Number.isFinite(stored.ratio) ? clampRatio(stored.ratio) : 0.5,
      focus: groups[focus].id,
    };
  } catch {
    return fallback();
  }
}

/* ── Changing the layout ──────────────────────────────────────────────────────
   Pure mutators over a `Layout`. The app calls each inside Solid's `produce`,
   so they read as ordinary mutation and are applied to the store as a patch.

   They live here rather than in the component because they are the part of this
   feature with the edge cases in it — closing the last tab of the last pane,
   dragging a tab out of a pane and emptying it, joining a split without losing
   what the second pane held — and none of that is testable from inside a
   component that needs a window to render. */

/** The tab in front in a group, or undefined for a group mid-edit. */
export function activeTab(group: Group): Tab | undefined {
  return group.tabs.find((t) => t.id === group.active);
}

export function groupOf(layout: Layout, tabId: number): Group | undefined {
  return layout.groups.find((g) => g.tabs.some((t) => t.id === tabId));
}

/**
 * Adds a tab to a group, after the one in front.
 *
 * Not at the end: a tab opened from the folder you are looking at belongs beside
 * it, which is where a browser puts one too.
 */
export function insertTab(layout: Layout, groupId: number, tab: Tab, activate = true) {
  const group = layout.groups.find((g) => g.id === groupId);
  if (!group) return;
  const at = group.tabs.findIndex((t) => t.id === group.active);
  group.tabs.splice(at + 1, 0, tab);
  if (activate) group.active = tab.id;
  layout.focus = group.id;
}

/**
 * Closes a tab, and the pane with it if it was the last one there.
 *
 * The window always has at least one tab: closing the only tab of the only pane
 * leaves a fresh one at `home` rather than an empty window. That is what every
 * tabbed application does, and the only behaviour that cannot strand somebody
 * looking at nothing with nothing to click.
 */
export function removeTab(layout: Layout, groupId: number, tabId: number, home: string, order: Order) {
  const gi = layout.groups.findIndex((g) => g.id === groupId);
  if (gi < 0) return;
  const group = layout.groups[gi];
  const at = group.tabs.findIndex((t) => t.id === tabId);
  if (at < 0) return;

  group.tabs.splice(at, 1);

  if (group.tabs.length > 0) {
    if (group.active === tabId) {
      // The one to its right, or the new last one — never back to the start,
      // which would lose your place in a strip of twelve.
      group.active = group.tabs[Math.min(at, group.tabs.length - 1)].id;
    }
    return;
  }

  if (layout.groups.length > 1) {
    layout.groups.splice(gi, 1);
    layout.focus = layout.groups[0].id;
  } else {
    const fresh = newTab(home, order);
    group.tabs.push(fresh);
    group.active = fresh.id;
  }
}

/**
 * Moves a dragged tab, within its strip or across the divider.
 *
 * Dropping the last tab of a pane into the other one collapses the split, which
 * is the rule `removeTab` follows and for the same reason: a pane with nothing
 * in it is not a pane.
 */
export function relocateTab(layout: Layout, tabId: number, to: { group: number; index: number }) {
  const from = groupOf(layout, tabId);
  const into = layout.groups.find((g) => g.id === to.group);
  if (!from || !into) return;

  const at = from.tabs.findIndex((t) => t.id === tabId);
  const [tab] = from.tabs.splice(at, 1);

  // Within one strip, an index measured before the tab was lifted out counts
  // the tab itself; past its old position that is one too many.
  let index = to.index;
  if (from === into && index > at) index -= 1;
  into.tabs.splice(Math.min(Math.max(index, 0), into.tabs.length), 0, tab);
  into.active = tab.id;
  layout.focus = into.id;

  if (from === into) return;
  if (from.tabs.length === 0) {
    layout.groups.splice(layout.groups.indexOf(from), 1);
  } else if (from.active === tabId) {
    from.active = from.tabs[Math.min(at, from.tabs.length - 1)].id;
  }
}

/**
 * Splits the window in two, or puts it back together.
 *
 * Joining moves the second pane's tabs into the first rather than closing them:
 * they are folders somebody opened, and un-splitting is not a request to lose
 * them.
 */
export function toggleSplit(layout: Layout, home: string, order: Order) {
  if (layout.groups.length > 1) {
    const [first, second] = layout.groups;
    first.tabs.push(...second.tabs);
    layout.groups.splice(1, 1);
    layout.focus = first.id;
    return;
  }

  const active = activeTab(layout.groups[0]);
  const group = newGroup([newTab(active ? currentPath(active) : home, order)]);
  layout.groups.push(group);
  layout.focus = group.id;
}
