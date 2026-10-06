import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";

/** One line of the overview: the keys, then what they do. */
type Shortcut = [keys: string[], does: string];

/**
 * Every shortcut the app answers to, grouped the way you reach for them.
 *
 * Written out rather than read from the key handlers: the handlers are spread
 * over several components and guard on context (not while typing, only with a
 * selection), and a list generated from them would read like code.
 */
const GROUPS: { title: string; items: Shortcut[] }[] = [
  {
    title: "Playback",
    items: [
      [["Space"], "Play / pause"],
      [["Double-click"], "Play track · open folder"],
    ],
  },
  {
    title: "Browsing",
    items: [
      [["Ctrl", "F"], "Search"],
      [["Ctrl", "I"], "Library overview"],
      [["Ctrl", "P"], "Playlists"],
      [["Ctrl", "B"], "Sidebar (selecting plays)"],
      [["Alt", "←"], "Back"],
      [["Alt", "→"], "Forward"],
      [["Ctrl", "Click"], "Add to selection"],
      [["Shift", "Click"], "Select range"],
    ],
  },
  {
    title: "Tabs & panes",
    items: [
      [["Ctrl", "T"], "New tab"],
      [["Ctrl", "W"], "Close tab"],
      [["Ctrl", "Tab"], "Next tab"],
      [["Ctrl", "Shift", "Tab"], "Previous tab"],
      [["Ctrl", "1–8"], "Go to tab"],
      [["Ctrl", "9"], "Last tab"],
      [["Ctrl", "\\"], "Dual pane on / off"],
      [["Tab"], "Switch pane"],
    ],
  },
  {
    title: "Files",
    items: [
      [["↑", "↓"], "Move in the list"],
      [["Home", "End"], "First / last"],
      [["PgUp", "PgDn"], "Page up / down"],
      [["Shift", "Move"], "Extend selection"],
      [["Enter"], "Open folder / play"],
      [["Backspace"], "Parent folder"],
      [["Ctrl", "A"], "Select all"],
      [["Ctrl", "C"], "Copy"],
      [["Ctrl", "X"], "Cut"],
      [["Ctrl", "V"], "Paste"],
      [["Ctrl", "Z"], "Undo rename"],
      [["Ctrl", "Shift", "N"], "New folder"],
      [["Del"], "Move to Recycle Bin"],
      [["Ctrl", "Drop"], "Copy here"],
      [["Shift", "Drop"], "Move here"],
    ],
  },
  {
    title: "Tagging",
    items: [
      [["F2"], "Edit tags"],
      [["Alt", "F1"], "Tags from file name"],
      [["Alt", "F2"], "File name from tags"],
      [["Alt", "F3"], "Find metadata"],
      [["Alt", "1–5"], "Rate"],
      [["Alt", "0"], "Clear rating"],
      [["Slow double-click"], "Edit cell in place"],
      [["Enter"], "Save cell"],
      [["Esc"], "Cancel edit"],
      [["Ctrl", "Enter"], "Save in tag editor"],
    ],
  },
  {
    title: "Playlists",
    items: [
      [["P"], "Add to / remove from current playlist"],
      [["Enter"], "Play (in Playlists)"],
      [["Del"], "Remove from playlist"],
      [["Alt", "↑ ↓"], "Move in playlist"],
      [["F2"], "Rename playlist"],
    ],
  },
  {
    title: "Quick tag",
    items: [
      [["T"], "Open / close"],
      [["1–9"], "First group"],
      [["Q–O"], "Second group"],
      [["A–L"], "Third group"],
      [["Shift", "1–5"], "Rate"],
    ],
  },
  {
    title: "Global",
    items: [
      [["F1"], "Shortcuts"],
      [["?"], "Shortcuts"],
      [["Esc"], "Close dialog"],
      [["Right-click", "Toolbar"], "Customize toolbar"],
    ],
  },
];

export default function Shortcuts(props: { onClose: () => void }) {
  const [query, setQuery] = createSignal("");

  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" && e.key !== "F1") return;
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  const hit = ([keys, does]: Shortcut) => {
    const q = query().trim().toLowerCase();
    return !q || does.toLowerCase().includes(q) || keys.join(" ").toLowerCase().includes(q);
  };

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer shortcuts" role="dialog" aria-label="Keyboard shortcuts">
        <header class="sethead">
          <span class="settitle">Keyboard shortcuts</span>
          <input
            class="syntaxsearch"
            type="text"
            placeholder="Search…"
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            ref={(el) => queueMicrotask(() => el.focus())}
          />
          <button class="setclose" onClick={props.onClose} aria-label="Close">
            <CloseIcon />
          </button>
        </header>

        <div class="renamebody shortcutbody">
          <For each={GROUPS.filter((g) => g.items.some(hit))}>
            {(group) => (
              <section class="shortcutgroup">
                <h3>{group.title}</h3>
                <For each={group.items.filter(hit)}>
                  {([keys, does]) => (
                    <div class="shortcut">
                      <span class="shortcutdoes">{does}</span>
                      <span class="shortcutkeys">
                        <For each={keys}>
                          {(k, i) => (
                            <>
                              <Show when={i() > 0}>
                                <span class="shortcutplus">+</span>
                              </Show>
                              <kbd>{k}</kbd>
                            </>
                          )}
                        </For>
                      </span>
                    </div>
                  )}
                </For>
              </section>
            )}
          </For>
          <Show when={!GROUPS.some((g) => g.items.some(hit))}>
            <p class="shortcutnone">No results</p>
          </Show>
        </div>
      </div>
    </>
  );
}
