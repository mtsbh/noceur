import { For, Show, createSignal } from "solid-js";
import { currentPath, type Group } from "./layout";

/** Last path segment, or the path itself for a drive root. */
function basename(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : path;
}

/** Where a dragged tab would land: which group, and at which index in it. */
export type TabDrop = { group: number; index: number };

/**
 * The tab strip for one pane.
 *
 * Tabs are draggable — within the strip to reorder, across the divider to hand
 * a folder to the other pane — and they are also drop targets for files, so a
 * track can be filed into a folder that is open but not in front of you.
 *
 * Those two are different mechanisms and have to be, because they are different
 * drags. Dragging a *tab* never leaves the web view, so it is pointer events
 * here. Dragging *files* leaves immediately: `start_drag` hands the mouse to
 * the shell's own modal drag loop, after which this window sees no pointer
 * events at all and hears about the drag only through Tauri's drag events. That
 * one is hit-tested in `App` against the `data-drop` attributes below, which is
 * why the highlight arrives as a prop rather than being decided here.
 */
export default function Tabs(props: {
  group: Group;
  /** Whether this pane is the one the tree and the keyboard are talking to. */
  focused: boolean;
  onActivate: (tabId: number) => void;
  onClose: (tabId: number) => void;
  onNew: () => void;
  /** A tab was dragged somewhere. The destination index is within `group`. */
  onMove: (tabId: number, to: TabDrop) => void;
  /**
   * Where a tab being dragged would land, wherever it started.
   *
   * Held by `App` rather than here, because a tab dragged across the divider is
   * tracked by the strip it *left* and has to be drawn by the strip it is over —
   * a marker kept per strip would be set on one and read by the other, and the
   * insertion mark would simply never appear on a cross-pane drag.
   */
  mark: TabDrop | null;
  onMark: (at: TabDrop | null) => void;
  /** Key of the drop target a file drag is currently over, or null. */
  dropKey: string | null;
}) {
  /** The tab being dragged out of this strip, if it is this one's. */
  const [dragging, setDragging] = createSignal<number | null>(null);

  /**
   * Where the pointer is, as a place a tab could go.
   *
   * Over the left half of a tab means before it, the right half means after it,
   * and anywhere else in a strip means the end — the same rule every tab bar
   * uses, and the reason the insertion mark can sit past the last tab.
   */
  function dropAt(x: number, y: number): TabDrop | null {
    const under = document.elementFromPoint(x, y);
    if (!under) return null;

    const tab = under.closest<HTMLElement>("[data-tab]");
    if (tab) {
      const group = Number(tab.dataset.group);
      const id = Number(tab.dataset.tab);
      const strip = tab.closest<HTMLElement>("[data-strip]");
      const ids = [...(strip?.querySelectorAll<HTMLElement>("[data-tab]") ?? [])].map((el) =>
        Number(el.dataset.tab),
      );
      const at = ids.indexOf(id);
      if (at < 0) return null;
      const box = tab.getBoundingClientRect();
      return { group, index: x < box.left + box.width / 2 ? at : at + 1 };
    }

    const strip = under.closest<HTMLElement>("[data-strip]");
    if (strip) {
      const count = strip.querySelectorAll("[data-tab]").length;
      return { group: Number(strip.dataset.strip), index: count };
    }
    return null;
  }

  /**
   * Drags a tab.
   *
   * Pointer events rather than HTML5 drag-and-drop, for the reason the column
   * headers are: this window has native file drop enabled so tracks can be
   * dragged in from Explorer, and on Windows that hands every drag gesture in
   * the web view to the OS — `dragstart` never fires.
   *
   * The drag only begins after a few pixels, so a click still switches tabs.
   */
  function startTabDrag(e: PointerEvent, tabId: number) {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startY = e.clientY;
    let moved = false;

    const move = (ev: PointerEvent) => {
      if (!moved) {
        if (Math.abs(ev.clientX - startX) < 4 && Math.abs(ev.clientY - startY) < 4) return;
        moved = true;
        setDragging(tabId);
        document.body.classList.add("tabbing");
      }
      props.onMark(dropAt(ev.clientX, ev.clientY));
    };

    const stop = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      if (!moved) return;

      document.body.classList.remove("tabbing");
      const to = dropAt(ev.clientX, ev.clientY);
      props.onMark(null);
      // Cleared after the click that follows this pointerup, so a tab dragged
      // somewhere else does not also activate itself on release.
      setTimeout(() => setDragging(null), 0);
      if (to) props.onMove(tabId, to);
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  /** Whether the insertion mark belongs before the tab at this index. */
  const markBefore = (index: number) => {
    const at = props.mark;
    return at !== null && at.group === props.group.id && at.index === index;
  };

  /** The tab in front in this pane — what the strip's blank space stands for. */
  const shown = () => props.group.tabs.find((t) => t.id === props.group.active);

  return (
    <div
      class={`tabs ${props.focused ? "focused" : ""}`}
      data-strip={props.group.id}
      // The strip's own blank space drops into the pane below it, under the same
      // key, so hovering it lights up that pane rather than nothing. Without
      // this a drop that landed a few pixels high — on the bar rather than the
      // list — would silently go to whichever pane happened to be focused.
      data-drop={shown() ? `pane:${shown()!.id}` : undefined}
      data-path={shown() ? currentPath(shown()!) : undefined}
      role="tablist"
    >
      <For each={props.group.tabs}>
        {(tab, i) => {
          const path = () => currentPath(tab);
          const key = () => `tab:${tab.id}`;
          return (
            <>
              <Show when={markBefore(i())}>
                <span class="tabmark" />
              </Show>
              <div
                class={`tab ${props.group.active === tab.id ? "on" : ""} ${
                  dragging() === tab.id ? "moving" : ""
                } ${props.dropKey === key() ? "dropping" : ""}`}
                data-tab={tab.id}
                data-group={props.group.id}
                // Read by the file-drop hit test in `App`. The key identifies
                // this tab so only the one under the pointer lights up; the
                // path is where the files would go.
                data-drop={key()}
                data-path={path()}
                role="tab"
                aria-selected={props.group.active === tab.id}
                title={path()}
                onPointerDown={(e) => {
                  // Activating on press rather than on click, so a tab dragged
                  // somewhere is the tab you were looking at while dragging it.
                  if (e.button === 0) {
                    props.onActivate(tab.id);
                    startTabDrag(e, tab.id);
                  }
                }}
                // Middle-click closes, as it does in every browser.
                onAuxClick={(e) => {
                  if (e.button === 1) {
                    e.preventDefault();
                    props.onClose(tab.id);
                  }
                }}
              >
                <span class="tabname">{basename(path())}</span>
                {/* Only ever closed by the button or the middle mouse button —
                    a tab that closed on a stray click would take its history
                    with it. */}
                <span
                  class="tabx"
                  role="button"
                  aria-label={`Close ${basename(path())}`}
                  onPointerDown={(e) => e.stopPropagation()}
                  onClick={(e) => {
                    e.stopPropagation();
                    props.onClose(tab.id);
                  }}
                >
                  ×
                </span>
              </div>
            </>
          );
        }}
      </For>
      <Show when={markBefore(props.group.tabs.length)}>
        <span class="tabmark" />
      </Show>

      {/* The only button left on the browser's own tooltip. `.tabs` clips its
          overflow so that too many tabs shrink rather than spill, and the app's
          tooltip is taller than the 27px strip — it would be sliced in half. */}
      <button class="tabnew" title="New tab · ctrl+t" aria-label="New tab" onClick={props.onNew}>
        +
      </button>
      {/* Fills the rest of the strip so the whole bar is a place to drop a tab,
          not just the tabs themselves. */}
      <span class="tabfill" />
    </div>
  );
}
