import { createSignal, createEffect, createMemo, on, For, Show, onCleanup } from "solid-js";
import { search, searchTag, type SearchHit, type TagLink } from "./api";
import {
  clampWidth,
  keyColour,
  loadSearchColumns,
  reorder,
  saveSearchColumns,
  type Column,
  type SearchKey,
} from "./columns";
import Menu from "./Menu";

/** The text a result shows in a column. */
function cell(hit: SearchHit, key: SearchKey): string {
  // Marked left-to-right: the cell runs right-to-left so a long path loses its
  // start rather than its end, and without the marks a trailing "-" or ")"
  // would jump to the front.
  if (key === "path") return `‎${hit.dirPath}‎`;
  if (key === "rating") return hit.rating ? "★".repeat(hit.rating) : "";
  const v = hit[key as keyof SearchHit];
  return typeof v === "string" ? v : "";
}

/** The value a column sorts by: numbers as numbers, blanks last either way. */
function sortValue(hit: SearchHit, key: SearchKey): string | number | null {
  if (key === "rating") return hit.rating || null;
  const text = cell(hit, key);
  return text === "" || text === "—" ? null : text;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

const SIZE_KEY = "noceur.searchSize";
const MIN_W = 480;
const MIN_H = 220;

const SORT_KEY = "noceur.searchSort";

function loadSort(): { key: SearchKey; desc: boolean } | null {
  try {
    const s = JSON.parse(localStorage.getItem(SORT_KEY) ?? "null");
    if (s && typeof s.key === "string") return { key: s.key, desc: !!s.desc };
  } catch {
    // Relevance order, then.
  }
  return null;
}

function loadSize(): { w: number; h: number } | null {
  try {
    const s = JSON.parse(localStorage.getItem(SIZE_KEY) ?? "null");
    if (s && Number(s.w) >= MIN_W && Number(s.h) >= MIN_H) {
      // A size saved on a bigger screen still has to fit this one.
      return {
        w: Math.min(Number(s.w), window.innerWidth - 24),
        h: Math.min(Number(s.h), window.innerHeight * 0.88 - 12),
      };
    }
  } catch {
    // Default size, then.
  }
  return null;
}

function saveSize(size: { w: number; h: number } | null) {
  try {
    if (size) localStorage.setItem(SIZE_KEY, JSON.stringify(size));
    else localStorage.removeItem(SIZE_KEY);
  } catch {
    // Storage unavailable: the size lasts until the popover closes.
  }
}

/**
 * How long typing settles before a query is sent.
 *
 * Long enough that a typed word is one query rather than five, short enough that
 * results feel attached to the keystroke. The spec budgets 80ms to first results;
 * this is the part of that budget spent deliberately.
 */
const DEBOUNCE_MS = 90;

/**
 * The `Ctrl+F` search popover.
 *
 * Searches every pinned folder plus the tree the user is currently in — not the
 * current directory alone, which is the case the feature exists for.
 */
export default function Search(props: {
  scopes: string[];
  /** Opens on every track with this tag - a sidebar link - until typed over. */
  tag?: TagLink;
  onClose: () => void;
  onOpen: (dirPath: string, fileId: number) => void;
}) {
  const [query, setQuery] = createSignal("");
  const [hits, setHits] = createSignal<SearchHit[]>([]);
  const [cursor, setCursor] = createSignal(0);
  const [busy, setBusy] = createSignal(false);
  /** The tag being listed; gone once something is typed. */
  const [tag, setTag] = createSignal(props.tag ?? null);

  const [columns, setColumns] = createSignal(loadSearchColumns());
  const shown = () => columns().filter((c) => c.visible);
  const template = () => `${shown().map((c) => `${c.width}px`).join(" ")} 1fr`;
  /** Null keeps the search's own relevance order. */
  const [sort, setSort] = createSignal<{ key: SearchKey; desc: boolean } | null>(loadSort());
  const [colMenu, setColMenu] = createSignal<{ x: number; y: number } | null>(null);
  const [dragging, setDragging] = createSignal<SearchKey | null>(null);
  const [dragOver, setDragOver] = createSignal<SearchKey | null>(null);

  /** The results in the order on screen; what the cursor indexes into. */
  const rows = createMemo(() => {
    const s = sort();
    const all = hits();
    if (!s) return all;
    const sign = s.desc ? -1 : 1;
    return [...all].sort((a, b) => {
      const x = sortValue(a, s.key);
      const y = sortValue(b, s.key);
      if (x === null || y === null) return x === y ? 0 : x === null ? 1 : -1;
      const order =
        typeof x === "number" && typeof y === "number"
          ? x - y
          : collator.compare(String(x), String(y));
      return order * sign;
    });
  });

  function commitColumns(next: Column<SearchKey>[]) {
    setColumns(next);
    saveSearchColumns(next);
  }

  /** Ascending, then descending, then back to relevance. */
  function chooseSort(key: SearchKey) {
    const s = sort();
    const next = !s || s.key !== key ? { key, desc: false } : s.desc ? null : { key, desc: true };
    setSort(next);
    setCursor(0);
    try {
      if (next) localStorage.setItem(SORT_KEY, JSON.stringify(next));
      else localStorage.removeItem(SORT_KEY);
    } catch {
      // Storage unavailable: the order lasts until the popover closes.
    }
  }

  function startResize(e: PointerEvent, key: SearchKey) {
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
      saveSearchColumns(columns());
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  /** Drags a heading onto another to move it there; a still click sorts. */
  function startMove(e: PointerEvent, key: SearchKey) {
    if (e.button !== 0) return;
    const startX = e.clientX;
    let moved = false;
    const columnAt = (x: number) =>
      (document.elementFromPoint(x, e.clientY)?.closest<HTMLElement>("[data-scol]")?.dataset
        .scol as SearchKey | undefined) ?? null;
    const move = (ev: PointerEvent) => {
      if (!moved) {
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
      // Cleared after the click that follows, so a moved heading does not sort.
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

  let input: HTMLInputElement | undefined;
  let listEl: HTMLDivElement | undefined;
  let headEl: HTMLDivElement | undefined;
  let boxEl: HTMLDivElement | undefined;

  /** The popover's size once the user has dragged it; null is the default. */
  const [size, setSize] = createSignal<{ w: number; h: number } | null>(loadSize());

  /**
   * Drags an edge or corner of the popover. It stays centred, so a side edge
   * moves both sides at once - the edge under the pointer follows it.
   */
  function startBoxResize(e: PointerEvent, edge: "e" | "w" | "s" | "se" | "sw") {
    if (e.button !== 0 || !boxEl) return;
    e.preventDefault();
    e.stopPropagation();
    const box = boxEl.getBoundingClientRect();
    const startX = e.clientX;
    const startY = e.clientY;
    document.body.classList.add("resizing");
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      let w = box.width;
      let h = box.height;
      if (edge.includes("e")) w += dx * 2;
      if (edge.includes("w")) w -= dx * 2;
      if (edge.includes("s")) h += dy;
      setSize({
        w: Math.round(Math.min(window.innerWidth - 24, Math.max(MIN_W, w))),
        h: Math.round(Math.min(window.innerHeight - box.top - 12, Math.max(MIN_H, h))),
      });
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      document.body.classList.remove("resizing");
      saveSize(size());
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  function resetSize() {
    setSize(null);
    saveSize(null);
  }
  let timer: number | undefined;
  // Only the newest query may write results: a slow one landing late would
  // otherwise replace the answers to what the user is typing now.
  let generation = 0;

  const listed = tag();
  if (listed) {
    const mine = ++generation;
    setBusy(true);
    searchTag(listed, props.scopes)
      .then((found) => {
        if (mine === generation) setHits(found);
      })
      .catch(() => {})
      .finally(() => {
        if (mine === generation) setBusy(false);
      });
  }

  createEffect(
    on(query, (q) => {
      if (q !== "") setTag(null);
      if (tag()) return;
      window.clearTimeout(timer);
      const mine = ++generation;

      if (q.trim() === "") {
        setHits([]);
        setBusy(false);
        return;
      }

      setBusy(true);
      timer = window.setTimeout(() => {
        search(q, props.scopes)
          .then((found) => {
            if (mine !== generation) return;
            setHits(found);
            setCursor(0);
          })
          .catch(() => {
            if (mine === generation) setHits([]);
          })
          .finally(() => {
            if (mine === generation) setBusy(false);
          });
      }, DEBOUNCE_MS);
    }),
  );

  onCleanup(() => window.clearTimeout(timer));

  /** Keeps the highlighted result in view when moving through it with the keyboard. */
  function revealCursor() {
    listEl
      ?.querySelector(".hit.on")
      ?.scrollIntoView({ block: "nearest" });
  }

  function move(delta: number) {
    const n = rows().length;
    if (n === 0) return;
    // Wraps, so holding a key does not stall at the end of a long result list.
    setCursor((cursor() + delta + n) % n);
    revealCursor();
  }

  /** Opens the folder holding the track and asks the list to select it. */
  function openHit(hit: SearchHit) {
    props.onOpen(hit.dirPath, hit.fileId);
    props.onClose();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const hit = rows()[cursor()];
      if (hit) openHit(hit);
    }
  }

  return (
    <>
      {/* Clicking anywhere outside dismisses, which is what a popover over a
          file list should do. */}
      <div class="scrim" onClick={() => props.onClose()} />
      <div
        class="finder"
        ref={(el) => (boxEl = el)}
        onKeyDown={onKey}
        style={{
          "--scols": template(),
          ...(size() ? { width: `${size()!.w}px`, height: `${size()!.h}px`, "max-height": "none" } : {}),
        }}
      >
        {/* Edges and corners to drag the popover to a size; double-click one
            to go back to the default. */}
        <For each={["e", "w", "s", "se", "sw"] as const}>
          {(edge) => (
            <div
              class={`fgrip ${edge}`}
              onPointerDown={(e) => startBoxResize(e, edge)}
              onDblClick={resetSize}
            />
          )}
        </For>
        <input
          ref={(el) => {
            input = el;
            // Focus after mount, so the first keystroke after Ctrl+F is typed
            // into the box rather than lost.
            queueMicrotask(() => input?.focus());
          }}
          class="finput"
          type="text"
          spellcheck={false}
          placeholder={
            tag()
              ? `${tag()!.field[0].toUpperCase()}${tag()!.field.slice(1)}: ${tag()!.value} · type to search`
              : "Search pinned folders and this tree…"
          }
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
        />

        {/* Same grid as the rows, scrolled sideways with them by hand. */}
        <div
          class="hdr fhdr"
          ref={(el) => (headEl = el)}
          onContextMenu={(e) => {
            e.preventDefault();
            e.stopPropagation();
            setColMenu({ x: e.clientX, y: e.clientY });
          }}
        >
          <For each={shown()}>
            {(col) => (
              <span
                data-scol={col.key}
                class={`col ${col.num ? "num" : ""} ${sort()?.key === col.key ? "sorted" : ""} ${
                  dragging() === col.key ? "moving" : ""
                } ${dragOver() === col.key && dragging() && dragging() !== col.key ? "over" : ""}`}
                title={`Sort by ${col.label.toLowerCase()} · drag to reorder`}
                onClick={() => {
                  if (!dragging()) chooseSort(col.key);
                }}
                onPointerDown={(e) => startMove(e, col.key)}
              >
                {col.label}
                <Show when={sort()?.key === col.key}>
                  <span class="arrow">{sort()!.desc ? "▾" : "▴"}</span>
                </Show>
                <span
                  class="grip"
                  title={`Resize ${col.label.toLowerCase()}`}
                  onPointerDown={(e) => startResize(e, col.key)}
                  onClick={(e) => e.stopPropagation()}
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
              setColMenu({ x: box.right - 170, y: box.bottom + 4 });
            }}
          >
            ⋮
          </span>
        </div>

        <div
          class="fresults"
          ref={(el) => (listEl = el)}
          onScroll={() => {
            if (headEl && listEl) headEl.scrollLeft = listEl.scrollLeft;
          }}
        >
          <Show
            when={rows().length > 0}
            fallback={
              <div class="fempty">
                {query().trim() === "" && !tag()
                  ? "Type to search."
                  : busy()
                    ? "Searching…"
                    : "Nothing found."}
              </div>
            }
          >
            <For each={rows()}>
              {(hit, i) => (
                <div
                  class={`hit ${i() === cursor() ? "on" : ""}`}
                  onClick={() => openHit(hit)}
                  onMouseEnter={() => setCursor(i())}
                >
                  <For each={shown()}>
                    {(col) => (
                      <span
                        class={`hcell ${col.num ? "num mono" : ""} ${col.dim ? "dim" : ""} ${
                          col.key === "key" ? "mono" : ""
                        } ${col.key === "path" ? "hpath" : ""} ${col.key === "rating" ? "hstars" : ""}`}
                        style={
                          col.key === "key" && keyColour(hit.keyNorm)
                            ? { color: keyColour(hit.keyNorm) }
                            : undefined
                        }
                        title={cell(hit, col.key)}
                      >
                        {cell(hit, col.key)}
                      </span>
                    )}
                  </For>
                </div>
              )}
            </For>
          </Show>
        </div>

        <div class="fhint">
          <span>↑↓ move</span>
          <span>↵ open</span>
          <span>esc close</span>
          <Show when={hits().length > 0}>
            <span class="grow" />
            <span>{hits().length} results</span>
          </Show>
        </div>
      </div>

      <Show when={colMenu()}>
        <Menu
          x={colMenu()!.x}
          y={colMenu()!.y}
          items={columns().map((col) => ({
            kind: "item" as const,
            label: col.label,
            checked: col.visible,
            disabled: col.key === "name",
            run: () =>
              commitColumns(
                columns().map((c) => (c.key === col.key ? { ...c, visible: !c.visible } : c)),
              ),
          }))}
          onClose={() => setColMenu(null)}
        />
      </Show>
    </>
  );
}
