import { createSignal, createEffect, on, For, Show } from "solid-js";
import { createStore } from "solid-js/store";
import { folderIconUrl, roots, subdirs, type Segment } from "./api";

const PIN_KEY = "noceur.pinned";

/** Reads the pinned paths, tolerating a preference written by an older build. */
function storedPins(): string[] {
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

const DRIVES_KEY = "noceur.showDrives";

/**
 * The folder tree.
 *
 * Two sections: folders the user pinned, and the drives. A DJ library lives in a
 * handful of places on one disk, so the drives are mostly scaffolding to be
 * folded away once the folders that matter are pinned.
 *
 * Children are fetched only when a branch is first opened. A music library is
 * wide and deep enough that walking it eagerly would read thousands of
 * directories to draw a dozen rows, and over SMB that is the difference between
 * instant and unusable.
 */
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

  roots().then(setRootList);

  /** Case-insensitive: Windows paths are, and the same folder may arrive spelled either way. */
  const norm = (p: string) => p.replace(/[\\/]+$/, "").toLowerCase();

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
   * Navigating from the list — a breadcrumb, a double-click, the up button —
   * must not leave the tree pointing somewhere else, or the two halves of the
   * window start telling different stories about where you are.
   */
  createEffect(
    on(
      () => props.current,
      (path) => {
        const parts = path.split(/[\\/]+/).filter(Boolean);
        if (parts.length === 0) return;

        // Unfold under the deepest pin that contains this path, rather than from
        // the drive. Pinning a folder is a statement about where you work; having
        // the drive unfold its whole spine every time you click a pin is exactly
        // the sprawl pinning was meant to avoid. Only when the path is under no
        // pin at all does the drive branch open.
        const containing = pins()
          .filter((pin) => norm(path) === norm(pin) || norm(path).startsWith(norm(pin) + "\\"))
          .sort((a, b) => b.length - a.length)[0];

        let acc: string;
        let rest: string[];

        if (containing) {
          acc = containing;
          const depth = norm(containing).split("\\").filter(Boolean).length;
          rest = parts.slice(depth);
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
   * One node.
   *
   * `label` is passed rather than taken from the node so a pinned folder can show
   * its own name at the top level, where its parent is not on screen to give it
   * context.
   */
  function Branch(p: { node: Segment; depth: number; label?: string }) {
    const kids = () => children[p.node.path];
    // Until a branch has been opened once there is no way to know whether it has
    // children without reading it, so the twisty is shown optimistically and
    // disappears if the read comes back empty.
    const expandable = () => kids() === undefined || kids()!.length > 0;

    return (
      <>
        <div
          class={`tnode ${isCurrent(p.node.path) ? "on" : ""}`}
          style={{ "padding-left": `${8 + p.depth * 13}px` }}
          onClick={() => props.onNavigate(p.node.path)}
          title={p.node.path}
        >
          <span
            class={`twisty ${open[p.node.path] ? "open" : ""} ${expandable() ? "" : "leaf"}`}
            onClick={(e) => {
              // The twisty unfolds without navigating; the label navigates.
              e.stopPropagation();
              toggle(p.node.path);
            }}
          />
          {/* The same shell icon the list draws, so a folder looks like itself
              on both sides of the window. Hidden rather than broken if the shell
              has nothing to give — a drive that went offline, most likely. */}
          <img
            class="ticon"
            src={folderIconUrl(p.node.path)}
            alt=""
            loading="lazy"
            decoding="async"
            draggable={false}
            onLoad={(e) => e.currentTarget.classList.remove("blank")}
            onError={(e) => e.currentTarget.classList.add("blank")}
          />
          <span class="tlabel">{p.label ?? p.node.label}</span>
          <span
            class={`pin ${isPinned(p.node.path) ? "on" : ""}`}
            title={isPinned(p.node.path) ? "Unpin this folder" : "Pin this folder"}
            onClick={(e) => {
              e.stopPropagation();
              togglePin(p.node.path);
            }}
          >
            {isPinned(p.node.path) ? "★" : "☆"}
          </span>
        </div>

        <Show when={open[p.node.path] && kids()}>
          <For each={kids()}>
            {(kid) => <Branch node={kid} depth={p.depth + 1} />}
          </For>
        </Show>
      </>
    );
  }

  return (
    <div class="tree">
      <Show when={pins().length > 0}>
        <div class="tsection">Pinned</div>
        <For each={pins()}>
          {(path) => (
            <Branch
              node={{ label: basename(path), path }}
              depth={0}
              label={basename(path)}
            />
          )}
        </For>
      </Show>

      {/* Once the folders that matter are pinned, the drives are scaffolding.
          Collapsing them is the difference between a tree of your library and a
          tree of the whole machine. */}
      <div class="tsection clickable" onClick={toggleDrives}>
        <span class={`twisty ${showDrives() ? "open" : ""}`} />
        Drives
      </div>
      <Show when={showDrives()}>
        <For each={rootList()}>{(r) => <Branch node={r} depth={0} />}</For>
      </Show>
    </div>
  );
}
