import { createSignal, createMemo, createEffect, on, For, Show } from "solid-js";
import { createStore } from "solid-js/store";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { folderIconUrl, roots, subdirs, type Segment } from "./api";

const PIN_KEY = "noceur.pinned";
const DRIVES_KEY = "noceur.showDrives";

/** Height of one tree row. Sections and nodes share it so the virtualizer needs no measuring. */
const TROW = 26;
const TOVERSCAN = 12;

/** Reads the pinned paths, tolerating a preference written by an older build. */
export function storedPins(): string[] {
  try {
    const raw = localStorage.getItem(PIN_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((p) => typeof p === "string") : [];
  } catch {
    return [];
  }
}

/** Last path segment, or the path itself for a drive root. */
function basename(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : path;
}

/** Case-insensitive: Windows paths are, and the same folder may arrive spelled either way. */
const norm = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();

/**
 * One rendered line of the tree.
 *
 * The tree is flattened to a list before rendering. Nesting the DOM the way the
 * folders nest is the natural shape, but it puts every expanded node on screen
 * whether or not it is in view — expand something with a few thousand
 * subdirectories and that is a few thousand elements and icon requests, most of
 * them scrolled out of sight. Flat, the virtualizer can do for the tree what it
 * already does for the list.
 */
type Line =
  | { kind: "section"; id: string; label: string; toggles?: boolean; open?: boolean }
  | { kind: "node"; id: string; path: string; label: string; depth: number };

export default function Tree(props: {
  current: string;
  onNavigate: (path: string) => void;
}) {
  const [rootList, setRootList] = createSignal<Segment[]>([]);
  const [pins, setPins] = createSignal<string[]>(storedPins());
  const [showDrives, setShowDrives] = createSignal(
    localStorage.getItem(DRIVES_KEY) !== "false",
  );

  // Children per path. A missing entry means "never opened"; an empty array
  // means "opened, and there is nothing inside" — which is why this cannot be a
  // plain lookup with a falsy default.
  const [children, setChildren] = createStore<Record<string, Segment[]>>({});
  const [open, setOpen] = createStore<Record<string, boolean>>({});
  const loading = new Set<string>();

  let scrollEl: HTMLDivElement | undefined;

  roots().then(setRootList);

  const isPinned = (path: string) => pins().some((p) => norm(p) === norm(path));

  function togglePin(path: string) {
    const next = isPinned(path)
      ? pins().filter((p) => norm(p) !== norm(path))
      : [...pins(), path];
    setPins(next);
    localStorage.setItem(PIN_KEY, JSON.stringify(next));
  }

  function toggleDrives() {
    const next = !showDrives();
    setShowDrives(next);
    localStorage.setItem(DRIVES_KEY, String(next));
  }

  function load(path: string) {
    if (children[path] !== undefined || loading.has(path)) return;
    loading.add(path);
    subdirs(path)
      .then((kids) => setChildren(path, kids))
      .catch(() => setChildren(path, []))
      .finally(() => loading.delete(path));
  }

  function toggle(path: string) {
    const next = !open[path];
    setOpen(path, next);
    if (next) load(path);
  }

  /**
   * Opens every ancestor of the current path so the list and the tree agree.
   *
   * Unfolds under the deepest pin that contains the path, rather than from the
   * drive. Pinning a folder is a statement about where you work; having the drive
   * unfold its whole spine every time you click a pin is the sprawl pinning was
   * meant to avoid. Only a path under no pin at all opens the drive branch.
   */
  createEffect(
    on(
      () => props.current,
      (path) => {
        const parts = path.split(/[\\/]+/).filter(Boolean);
        if (parts.length === 0) return;

        const containing = pins()
          .filter(
            (pin) => norm(path) === norm(pin) || norm(path).startsWith(norm(pin) + "\\"),
          )
          .sort((a, b) => b.length - a.length)[0];

        let acc: string;
        let rest: string[];

        if (containing) {
          acc = containing;
          rest = parts.slice(norm(containing).split("\\").filter(Boolean).length);
        } else {
          // Rebuild each ancestor path in turn: "E:" -> "E:\music" -> ...
          acc = parts[0].endsWith(":") ? `${parts[0]}\\` : `\\${parts[0]}`;
          rest = parts.slice(1);
        }

        setOpen(acc, true);
        load(acc);

        for (const part of rest) {
          acc = acc.endsWith("\\") ? `${acc}${part}` : `${acc}\\${part}`;
          setOpen(acc, true);
          load(acc);
        }
      },
    ),
  );

  const isCurrent = (path: string) => norm(path) === norm(props.current);

  /**
   * Every line the tree would show, in order.
   *
   * Recomputed when a branch opens, closes or loads. Only the open branches are
   * walked, so this is proportional to what is visible rather than to the tree.
   */
  const lines = createMemo<Line[]>(() => {
    const out: Line[] = [];

    const walk = (node: Segment, depth: number, label?: string) => {
      out.push({
        kind: "node",
        id: `${depth}:${node.path}`,
        path: node.path,
        label: label ?? node.label,
        depth,
      });
      if (!open[node.path]) return;
      for (const kid of children[node.path] ?? []) walk(kid, depth + 1);
    };

    if (pins().length > 0) {
      out.push({ kind: "section", id: "sec:pinned", label: "Pinned" });
      for (const path of pins()) {
        walk({ label: basename(path), path }, 0, basename(path));
      }
    }

    out.push({
      kind: "section",
      id: "sec:drives",
      label: "Drives",
      toggles: true,
      open: showDrives(),
    });
    if (showDrives()) {
      for (const drive of rootList()) walk(drive, 0);
    }

    return out;
  });

  const virtualizer = createVirtualizer({
    get count() {
      return lines().length;
    },
    getScrollElement: () => scrollEl ?? null,
    estimateSize: () => TROW,
    overscan: TOVERSCAN,
  });

  /** A branch never opened might still have children; the twisty shows until proven otherwise. */
  const expandable = (path: string) => {
    const kids = children[path];
    return kids === undefined || kids.length > 0;
  };

  return (
    <div class="tree" ref={(el) => (scrollEl = el)}>
      <div class="tcanvas" style={{ height: `${virtualizer.getTotalSize()}px` }}>
        <For each={virtualizer.getVirtualItems()}>
          {(item) => {
            const line = () => lines()[item.index];
            return (
              <div
                class="trow"
                style={{ height: `${item.size}px`, transform: `translateY(${item.start}px)` }}
              >
                <Show when={line()} keyed>
                  {(l) =>
                    l.kind === "section" ? (
                      <div
                        class={`tsection ${l.toggles ? "clickable" : ""}`}
                        onClick={() => l.toggles && toggleDrives()}
                      >
                        <Show when={l.toggles}>
                          <span class={`twisty ${l.open ? "open" : ""}`} />
                        </Show>
                        {l.label}
                      </div>
                    ) : (
                      <div
                        class={`tnode ${isCurrent(l.path) ? "on" : ""}`}
                        style={{ "padding-left": `${8 + l.depth * 13}px` }}
                        onClick={() => props.onNavigate(l.path)}
                        title={l.path}
                      >
                        <span
                          class={`twisty ${open[l.path] ? "open" : ""} ${
                            expandable(l.path) ? "" : "leaf"
                          }`}
                          onClick={(e) => {
                            // The twisty unfolds without navigating; the label navigates.
                            e.stopPropagation();
                            toggle(l.path);
                          }}
                        />
                        {/* The same shell icon the list draws, so a folder looks
                            like itself on both sides of the window. */}
                        <img
                          class="ticon"
                          src={folderIconUrl(l.path)}
                          alt=""
                          loading="lazy"
                          decoding="async"
                          draggable={false}
                          onLoad={(e) => e.currentTarget.classList.remove("blank")}
                          onError={(e) => e.currentTarget.classList.add("blank")}
                        />
                        <span class="tlabel">{l.label}</span>
                        <span
                          class={`pin ${isPinned(l.path) ? "on" : ""}`}
                          title={isPinned(l.path) ? "Unpin this folder" : "Pin this folder"}
                          onClick={(e) => {
                            e.stopPropagation();
                            togglePin(l.path);
                          }}
                        >
                          {isPinned(l.path) ? "★" : "☆"}
                        </span>
                      </div>
                    )
                  }
                </Show>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}
