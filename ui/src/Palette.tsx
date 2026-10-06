import { createMemo, createSignal, For, onMount, Show, type JSX } from "solid-js";
import { GROUPS, type BarItem } from "./Toolbar";
import { BoltIcon } from "./icons";

type Entry = {
  id: string;
  label: string;
  icon?: () => JSX.Element;
  section: string;
  off?: () => boolean;
  run: () => void;
};

/**
 * Ctrl+K: every action in the toolbar, and a few that are not, by name.
 *
 * Built from the same items as the bar, so anything the bar can do — your own
 * buttons, programs and sites included — is here without being listed twice.
 * Words match anywhere and in any order: "bpm det" finds Detect BPM & key.
 */
export default function Palette(props: {
  items: BarItem[];
  extra: { id: string; label: string; icon?: () => JSX.Element; run: () => void }[];
  onClose: () => void;
}) {
  const [query, setQuery] = createSignal("");
  const [at, setAt] = createSignal(0);
  let input: HTMLInputElement | undefined;
  let list: HTMLDivElement | undefined;

  const all = createMemo<Entry[]>(() => {
    const section = (i: BarItem) =>
      GROUPS.find((g) => g.id === i.group)?.label ?? "View";
    const fromBar = props.items
      .filter((i) => i.run && !i.members && !i.render && !i.absent?.())
      .map<Entry>((i) => ({
        id: i.id,
        label: i.label,
        icon: i.icon,
        section: section(i),
        off: i.off,
        run: () => i.run!(new MouseEvent("click") as MouseEvent & { currentTarget: HTMLElement }),
      }));
    // Grouped first, in bar order, then the rest.
    const order = [...GROUPS.map((g) => g.label), "View", "App"];
    return [...fromBar, ...props.extra.map((x) => ({ ...x, section: "App" }))].sort(
      (a, b) => order.indexOf(a.section) - order.indexOf(b.section),
    );
  });

  const shown = createMemo(() => {
    const words = query().toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return all();
    return all().filter((e) => {
      const hay = `${e.label} ${e.section}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
  });

  onMount(() => input?.focus());

  function run(e: Entry | undefined) {
    if (!e || e.off?.()) return;
    props.onClose();
    e.run();
  }

  function move(by: number) {
    const n = shown().length;
    if (!n) return;
    const next = (at() + by + n) % n;
    setAt(next);
    list?.querySelectorAll(".palitem")[next]?.scrollIntoView({ block: "nearest" });
  }

  function key(e: KeyboardEvent) {
    if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Enter") run(shown()[at()]);
    else if (e.key === "Escape") props.onClose();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="palette" role="dialog" aria-label="Command palette" onKeyDown={key}>
        <input
          ref={(el) => (input = el)}
          class="palinput"
          placeholder="Type an action"
          value={query()}
          spellcheck={false}
          onInput={(e) => {
            setQuery(e.currentTarget.value);
            setAt(0);
          }}
        />
        <div class="pallist" ref={(el) => (list = el)}>
          <For each={shown()}>
            {(e, i) => {
              const I = e.icon ?? BoltIcon;
              return (
                <>
                  <Show when={i() === 0 || shown()[i() - 1].section !== e.section}>
                    <div class="palsection">{e.section}</div>
                  </Show>
                  <div
                    class={`palitem ${i() === at() ? "at" : ""} ${e.off?.() ? "off" : ""}`}
                    onPointerMove={() => setAt(i())}
                    onClick={() => run(e)}
                  >
                    <span class="palicon">
                      <I />
                    </span>
                    {e.label}
                    <Show when={e.off?.()}>
                      <span class="palnote">select tracks</span>
                    </Show>
                  </div>
                </>
              );
            }}
          </For>
          <Show when={!shown().length}>
            <div class="palempty">No action matches</div>
          </Show>
        </div>
      </div>
    </>
  );
}
