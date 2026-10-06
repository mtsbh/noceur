import { createEffect, createSignal, onCleanup, For, Show, type JSX } from "solid-js";

/** One line of a menu: an action, or a rule between groups. */
export type MenuItem =
  | { kind: "separator" }
  /** A rule with a small name on it, for a menu sorted into the bar's groups. */
  | { kind: "header"; label: string }
  | {
      kind: "item";
      label: string;
      /** Shown right-aligned and dimmed — the key that does the same thing. */
      hint?: string;
      /** Rendered in the accent, for anything that destroys or leaves the app. */
      danger?: boolean;
      disabled?: boolean;
      /** Draws a tick, for entries that toggle something on and off. */
      checked?: boolean;
      /** Drawn before the label. */
      icon?: () => JSX.Element;
      /** A submenu, opened by hovering. `run` is then ignored. */
      children?: MenuItem[];
      run: () => void;
    };

/**
 * A context menu in the app's own theme.
 *
 * Windows' own menu is available beside this one for the times a shell extension
 * is what you want; this is for the things a music browser knows about and the
 * shell does not, drawn to match the rest of the window rather than the system.
 */
export default function Menu(props: {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}) {
  let box: HTMLDivElement | undefined;
  const iconned = () => props.items.some((i) => i.kind === "item" && i.icon);

  // Escape closes, and so does any key that is not a menu key: a menu left open
  // while the user starts typing is a menu in the way.
  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  /**
   * Nudges the menu back inside the window once its real size is known.
   *
   * Measured after mount rather than guessed from an item count, because the
   * height depends on how many separators and how long the labels are.
   */
  createEffect(() => {
    if (!box) return;
    const rect = box.getBoundingClientRect();
    const overflowX = rect.right - window.innerWidth + 6;
    const overflowY = rect.bottom - window.innerHeight + 6;
    if (overflowX > 0) box.style.left = `${Math.max(6, props.x - overflowX)}px`;
    if (overflowY > 0) box.style.top = `${Math.max(6, props.y - overflowY)}px`;
  });

  return (
    <>
      {/* Pointer-down rather than click, so the menu is gone before whatever is
          underneath reacts to the same gesture. */}
      <div class="scrim bare" onPointerDown={() => props.onClose()} onContextMenu={(e) => {
        e.preventDefault();
        props.onClose();
      }} />
      <div
        class="ctxmenu"
        ref={(el) => (box = el)}
        style={{ left: `${props.x}px`, top: `${props.y}px` }}
      >
        <Items items={props.items} iconned={iconned()} onClose={props.onClose} />
      </div>
    </>
  );
}

/** The entries of one menu box, and the submenu hovering one of them opens. */
function Items(props: { items: MenuItem[]; iconned: boolean; onClose: () => void }) {
  const [sub, setSub] = createSignal<{ items: MenuItem[]; x: number; y: number; left: number } | null>(null);
  let subBox: HTMLDivElement | undefined;

  // Kept on screen: flips to the left of its parent when it would run off the right.
  createEffect(() => {
    const open = sub();
    if (!open || !subBox) return;
    const rect = subBox.getBoundingClientRect();
    if (rect.right > window.innerWidth - 6) {
      subBox.style.left = `${Math.max(6, open.left - rect.width + 2)}px`;
    }
    if (rect.bottom > window.innerHeight - 6) {
      subBox.style.top = `${Math.max(6, window.innerHeight - rect.height - 6)}px`;
    }
  });

  return (
    <>
      <For each={props.items}>
        {(item) => (
          <Show
            when={item.kind === "item" ? item : null}
            fallback={
              item.kind === "header" ? (
                <div class="ctxhead">{item.label}</div>
              ) : (
                <div class="ctxsep" />
              )
            }
          >
            {(entry) => (
              <div
                class={`ctxitem ${entry().danger ? "danger" : ""} ${
                  entry().disabled ? "off" : ""
                } ${entry().children ? "parent" : ""}`}
                onPointerEnter={(e) => {
                  const kids = entry().children;
                  if (!kids || entry().disabled) return setSub(null);
                  const r = e.currentTarget.getBoundingClientRect();
                  setSub({ items: kids, x: r.right - 2, y: r.top - 5, left: r.left });
                }}
                onClick={() => {
                  if (entry().disabled || entry().children) return;
                  entry().run();
                  props.onClose();
                }}
              >
                <Show when={entry().checked !== undefined}>
                  <span class={`ctxtick ${entry().checked ? "on" : ""}`}>✓</span>
                </Show>
                {/* A blank of the same width when others in this menu have
                    an icon, so every label starts on one line. */}
                <Show when={entry().icon || props.iconned}>
                  <span class="ctxicon">{entry().icon?.()}</span>
                </Show>
                <span class="ctxlabel">{entry().label}</span>
                <Show when={entry().hint}>
                  <span class="ctxhint">{entry().hint}</span>
                </Show>
                <Show when={entry().children}>
                  <span class="ctxmore">›</span>
                </Show>
              </div>
            )}
          </Show>
        )}
      </For>
      <Show when={sub()}>
        {(open) => (
          <div
            class="ctxmenu sub"
            ref={(el) => (subBox = el)}
            style={{ left: `${open().x}px`, top: `${open().y}px` }}
          >
            <Items
              items={open().items}
              iconned={open().items.some((i) => i.kind === "item" && i.icon)}
              onClose={props.onClose}
            />
          </div>
        )}
      </Show>
    </>
  );
}
