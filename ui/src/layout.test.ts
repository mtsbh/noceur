/**
 * The layout: what survives a restart, and what the tab reducers do to it.
 *
 * Run with the runtime the project already needs and nothing else —
 * `npm --prefix ui test` — because Node strips the type annotations itself and
 * this file imports no framework. A frontend test that needed a test framework,
 * a jsdom and a config file would be a fourth build to keep working; this is
 * `node --test` over pure functions.
 *
 * Pure functions are all this covers, and deliberately. The reducers are where
 * the edge cases live — closing the last tab of the last pane, dragging a pane
 * empty, joining a split — and every one of them is a plain transformation of a
 * `Layout`. Rendering a pane needs a window, a web view and a music library, and
 * is not something a unit test should pretend to have.
 */

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  activeTab,
  clampRatio,
  currentPath,
  insertTab,
  loadLayout,
  newGroup,
  newTab,
  relocateTab,
  removeTab,
  saveLayout,
  toggleSplit,
  type Group,
  type Layout,
} from "./layout.ts";
import type { Order } from "./api.ts";

/** `layout.ts` reads and writes preferences; Node has no browser storage. */
const store = new Map<string, string>();
(globalThis as { localStorage?: unknown }).localStorage = {
  getItem: (k: string) => (store.has(k) ? store.get(k)! : null),
  setItem: (k: string, v: string) => void store.set(k, v),
};

const ORDER: Order = { sort: "name", desc: false };
const HOME = "E:\\music";

const paths = (g: Group) => g.tabs.map(currentPath).join("|");
const make = (...groups: Group[]): Layout => ({
  groups,
  dir: "row",
  ratio: 0.5,
  focus: groups[0].id,
});

/**
 * What must be true of a layout after anything at all has happened to it.
 *
 * Asserted after every mutation rather than only where it seems at risk. The
 * failure these guard against is not a wrong tab order — it is a window with no
 * pane in it, or a pane whose active tab was closed, and there is no way back
 * from either without deleting a preference file.
 */
function usable(l: Layout) {
  assert.ok(l.groups.length >= 1, "the window has at least one pane");
  assert.ok(l.groups.length <= 2, "the window has no more than two panes");
  for (const g of l.groups) {
    assert.ok(g.tabs.length >= 1, "every pane has a tab");
    assert.ok(activeTab(g) !== undefined, "every pane's active tab exists");
  }
  assert.ok(
    l.groups.some((g) => g.id === l.focus),
    "focus names a pane that is there",
  );
  const ids = l.groups.flatMap((g) => g.tabs.map((t) => t.id));
  assert.equal(new Set(ids).size, ids.length, "no tab is in two places");
}

test("a fresh profile opens one tab at home", () => {
  store.clear();
  const l = loadLayout(HOME, ORDER);
  assert.equal(l.groups.length, 1);
  assert.equal(currentPath(l.groups[0].tabs[0]), HOME);
  usable(l);
});

test("a split with several tabs survives a restart", () => {
  store.clear();
  const a = newGroup([newTab("E:\\a", ORDER), newTab("E:\\b", { sort: "bpm", desc: true })]);
  const b = newGroup([newTab("E:\\c", ORDER)]);
  a.active = a.tabs[1].id;
  const before = { groups: [a, b], dir: "column", ratio: 0.3, focus: b.id } as Layout;
  saveLayout(before, before.ratio);

  const after = loadLayout(HOME, ORDER);
  assert.equal(after.groups.length, 2);
  assert.equal(paths(after.groups[0]), "E:\\a|E:\\b");
  assert.equal(currentPath(activeTab(after.groups[0])!), "E:\\b");
  assert.deepEqual(after.groups[0].tabs[1].order, { sort: "bpm", desc: true });
  assert.equal(after.dir, "column");
  assert.equal(after.ratio, 0.3);
  assert.equal(after.focus, after.groups[1].id);
  // Ids are per-run: a restored tab is a new tab that happens to be at the same
  // path, and nothing may assume otherwise.
  assert.notEqual(after.groups[0].id, a.id);
  usable(after);
});

test("nothing the file can say leaves the window unusable", () => {
  const junk = [
    "{",
    "null",
    "[]",
    '{"groups":[]}',
    '{"groups":[{"tabs":[]}]}',
    '{"groups":[{"tabs":[{"path":""}]}]}',
    '{"groups":[{"tabs":[{"path":"E:\\\\x"}],"active":9}]}',
    '{"groups":[{"tabs":[{"path":"E:\\\\1"}]},{"tabs":[{"path":"E:\\\\2"}]},{"tabs":[{"path":"E:\\\\3"}]}]}',
  ];
  for (const raw of junk) {
    store.set("noceur.layout", raw);
    usable(loadLayout(HOME, ORDER));
  }
});

test("out-of-range values in the file are clamped, not trusted", () => {
  store.set(
    "noceur.layout",
    '{"groups":[{"tabs":[{"path":"E:\\\\x","order":{"sort":"nope","desc":"yes"}}],"active":9}],' +
      '"dir":"sideways","ratio":99,"focus":7}',
  );
  const l = loadLayout(HOME, ORDER);
  assert.equal(l.groups[0].active, l.groups[0].tabs[0].id, "an active index past the end clamps");
  assert.deepEqual(l.groups[0].tabs[0].order, ORDER, "an unknown sort column falls back");
  assert.equal(l.dir, "row", "an unknown direction falls back");
  assert.equal(l.ratio, clampRatio(99));
  assert.equal(l.focus, l.groups[0].id);
  usable(l);
});

test("a new tab lands beside the one it was opened from", () => {
  const g = newGroup([newTab("E:\\a", ORDER), newTab("E:\\c", ORDER)]);
  const l = make(g);
  insertTab(l, g.id, newTab("E:\\b", ORDER));
  assert.equal(paths(l.groups[0]), "E:\\a|E:\\b|E:\\c");
  assert.equal(currentPath(activeTab(l.groups[0])!), "E:\\b");
  usable(l);
});

test("closing the active tab moves right, then left at the end", () => {
  const g = newGroup([newTab("E:\\a", ORDER), newTab("E:\\b", ORDER), newTab("E:\\c", ORDER)]);
  g.active = g.tabs[1].id;
  const l = make(g);

  removeTab(l, g.id, g.tabs[1].id, HOME, ORDER);
  assert.equal(paths(l.groups[0]), "E:\\a|E:\\c");
  assert.equal(currentPath(activeTab(l.groups[0])!), "E:\\c");

  removeTab(l, g.id, activeTab(l.groups[0])!.id, HOME, ORDER);
  assert.equal(currentPath(activeTab(l.groups[0])!), "E:\\a");
  usable(l);
});

test("closing an inactive tab leaves you where you were", () => {
  const g = newGroup([newTab("E:\\a", ORDER), newTab("E:\\b", ORDER)]);
  const l = make(g);
  removeTab(l, g.id, g.tabs[1].id, HOME, ORDER);
  assert.equal(currentPath(activeTab(l.groups[0])!), "E:\\a");
  usable(l);
});

test("closing the only tab of the only pane reopens at home", () => {
  const g = newGroup([newTab("E:\\only", ORDER)]);
  const l = make(g);
  removeTab(l, g.id, g.tabs[0].id, HOME, ORDER);
  assert.equal(paths(l.groups[0]), HOME);
  usable(l);
});

test("closing a split pane's last tab collapses the split", () => {
  const a = newGroup([newTab("E:\\a", ORDER)]);
  const b = newGroup([newTab("E:\\b", ORDER)]);
  const l = make(a, b);
  l.focus = b.id;

  removeTab(l, b.id, b.tabs[0].id, HOME, ORDER);
  assert.equal(l.groups.length, 1);
  assert.equal(paths(l.groups[0]), "E:\\a");
  assert.equal(l.focus, l.groups[0].id, "focus moves to what is left");
  usable(l);
});

test("closing a tab that is not there changes nothing", () => {
  const g = newGroup([newTab("E:\\a", ORDER)]);
  const l = make(g);
  const before = JSON.stringify(l);
  removeTab(l, g.id, 99999, HOME, ORDER);
  removeTab(l, 99999, g.tabs[0].id, HOME, ORDER);
  assert.equal(JSON.stringify(l), before);
});

test("a tab dragged within its strip lands where the mark was", () => {
  const g = newGroup([newTab("E:\\a", ORDER), newTab("E:\\b", ORDER), newTab("E:\\c", ORDER)]);
  const l = make(g);

  relocateTab(l, l.groups[0].tabs[0].id, { group: g.id, index: 3 });
  assert.equal(paths(l.groups[0]), "E:\\b|E:\\c|E:\\a");

  relocateTab(l, l.groups[0].tabs[2].id, { group: g.id, index: 0 });
  assert.equal(paths(l.groups[0]), "E:\\a|E:\\b|E:\\c");
  usable(l);
});

test("dropping a tab on itself, or just past itself, does nothing", () => {
  const g = newGroup([newTab("E:\\a", ORDER), newTab("E:\\b", ORDER), newTab("E:\\c", ORDER)]);
  const l = make(g);
  const middle = g.tabs[1].id;

  relocateTab(l, middle, { group: g.id, index: 1 });
  assert.equal(paths(l.groups[0]), "E:\\a|E:\\b|E:\\c");
  relocateTab(l, middle, { group: g.id, index: 2 });
  assert.equal(paths(l.groups[0]), "E:\\a|E:\\b|E:\\c");
});

test("a tab dragged across the divider leaves the source usable", () => {
  const a = newGroup([newTab("E:\\a", ORDER), newTab("E:\\b", ORDER)]);
  const b = newGroup([newTab("E:\\c", ORDER)]);
  const l = make(a, b);

  relocateTab(l, a.tabs[0].id, { group: b.id, index: 0 });
  assert.equal(paths(l.groups[1]), "E:\\a|E:\\c");
  assert.equal(paths(l.groups[0]), "E:\\b");
  assert.equal(currentPath(activeTab(l.groups[0])!), "E:\\b", "the source picks a new active tab");
  assert.equal(currentPath(activeTab(l.groups[1])!), "E:\\a", "the moved tab is active where it lands");
  assert.equal(l.focus, l.groups[1].id, "focus follows the tab");
  usable(l);
});

test("dragging a pane's last tab out collapses it without losing the tab", () => {
  const a = newGroup([newTab("E:\\a", ORDER)]);
  const b = newGroup([newTab("E:\\c", ORDER)]);
  const l = make(a, b);

  relocateTab(l, a.tabs[0].id, { group: b.id, index: 1 });
  assert.equal(l.groups.length, 1);
  assert.equal(paths(l.groups[0]), "E:\\c|E:\\a");
  usable(l);
});

test("a drop index off either end is clamped", () => {
  const a = newGroup([newTab("E:\\a", ORDER), newTab("E:\\b", ORDER)]);
  const b = newGroup([newTab("E:\\c", ORDER)]);
  const l = make(a, b);

  relocateTab(l, a.tabs[0].id, { group: b.id, index: 99 });
  assert.equal(paths(l.groups[1]), "E:\\c|E:\\a");

  relocateTab(l, l.groups[1].tabs[1].id, { group: l.groups[0].id, index: -5 });
  assert.equal(paths(l.groups[0]), "E:\\a|E:\\b");
  usable(l);
});

test("splitting opens the folder in front of you, joining keeps everything", () => {
  const g = newGroup([newTab("E:\\a", ORDER), newTab("E:\\b", ORDER)]);
  g.active = g.tabs[1].id;
  const l = make(g);

  toggleSplit(l, HOME, ORDER);
  assert.equal(paths(l.groups[1]), "E:\\b");
  assert.equal(l.focus, l.groups[1].id);
  usable(l);

  insertTab(l, l.groups[1].id, newTab("E:\\d", ORDER));
  toggleSplit(l, HOME, ORDER);
  assert.equal(l.groups.length, 1);
  assert.equal(paths(l.groups[0]), "E:\\a|E:\\b|E:\\b|E:\\d", "no tab is dropped on the way back");
  usable(l);
});
