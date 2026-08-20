import { createSignal, createEffect, on, For, Show } from "solid-js";
import { createStore } from "solid-js/store";
import { folderIconUrl, roots, subdirs, type Segment } from "./api";

/**
 * The folder tree.
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
  const [nodes] = createSignal<Promise<Segment[]>>(roots());
  const [rootList, setRootList] = createSignal<Segment[]>([]);

  // Children per path. A missing entry means "never opened"; an empty array
  // means "opened, and there is nothing inside" — which is why this cannot be a
  // plain lookup with a falsy default.
  const [children, setChildren] = createStore<Record<string, Segment[]>>({});
  const [open, setOpen] = createStore<Record<string, boolean>>({});
  const loading = new Set<string>();

  nodes().then(setRootList);

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

        // Rebuild each ancestor path in turn: "E:" -> "E:\music" -> ...
        let acc = parts[0].endsWith(":") ? `${parts[0]}\\` : `\\${parts[0]}`;
        setOpen(acc, true);
        load(acc);

        for (const part of parts.slice(1)) {
          acc = acc.endsWith("\\") ? `${acc}${part}` : `${acc}\\${part}`;
          setOpen(acc, true);
          load(acc);
        }
      },
    ),
  );

  /** Case-insensitive, because Windows paths are and the current path may differ in case. */
  const isCurrent = (path: string) =>
    path.replace(/\\+$/, "").toLowerCase() ===
    props.current.replace(/\\+$/, "").toLowerCase();

  function Branch(p: { node: Segment; depth: number }) {
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
          <span class="tlabel">{p.node.label}</span>
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
      <For each={rootList()}>{(r) => <Branch node={r} depth={0} />}</For>
    </div>
  );
}
