import { createSignal, For, onCleanup, Show, type JSX } from "solid-js";

export type PickOption = {
  id: string;
  label: string;
  /** A short word after the label, in grey. */
  note?: string;
  /** Drawn before the label: a theme's swatch. */
  lead?: () => JSX.Element;
  /** Style on the label: a font drawn in its own face. */
  face?: string;
};

/**
 * A dropdown that tries each choice on the whole window as you go.
 *
 * Hovering or arrowing through the list calls `onPreview`, so a theme or a font
 * is chosen by looking at the app rather than at a sample. Only a click or
 * Enter keeps it; Escape or a click elsewhere calls `onRevert`, which puts the
 * saved one back.
 */
export default function LivePicker(props: {
  options: PickOption[];
  value: string;
  onPreview: (id: string) => void;
  onRevert: () => void;
  onChange: (id: string) => void;
  label: string;
}) {
  const [open, setOpen] = createSignal(false);
  const [at, setAt] = createSignal(0);
  let root: HTMLDivElement | undefined;
  let list: HTMLDivElement | undefined;

  const current = () => props.options.find((o) => o.id === props.value) ?? props.options[0];

  function preview(i: number) {
    setAt(i);
    props.onPreview(props.options[i].id);
    list?.children[i]?.scrollIntoView({ block: "nearest" });
  }

  function close(keep: boolean) {
    if (!open()) return;
    setOpen(false);
    window.removeEventListener("pointerdown", outside, true);
    if (keep) props.onChange(props.options[at()].id);
    else props.onRevert();
  }

  function outside(e: PointerEvent) {
    if (root && !root.contains(e.target as Node)) close(false);
  }

  function show() {
    setAt(Math.max(0, props.options.findIndex((o) => o.id === props.value)));
    setOpen(true);
    window.addEventListener("pointerdown", outside, true);
  }

  onCleanup(() => {
    window.removeEventListener("pointerdown", outside, true);
    if (open()) props.onRevert();
  });

  function key(e: KeyboardEvent) {
    if (!open()) {
      if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown") {
        e.preventDefault();
        show();
      }
      return;
    }
    const n = props.options.length;
    if (e.key === "ArrowDown") preview((at() + 1) % n);
    else if (e.key === "ArrowUp") preview((at() - 1 + n) % n);
    else if (e.key === "Enter" || e.key === " ") close(true);
    else if (e.key === "Escape") close(false);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  const Row = (o: PickOption) => (
    <>
      {o.lead?.()}
      <span class="pickname" style={o.face ? { "font-family": o.face } : undefined}>
        {o.label}
      </span>
      <Show when={o.note}>
        <span class="picknote">{o.note}</span>
      </Show>
    </>
  );

  return (
    <div class="pick" ref={(el) => (root = el)}>
      <button
        class={`pickcur ${open() ? "open" : ""}`}
        onClick={() => (open() ? close(false) : show())}
        onKeyDown={key}
        aria-haspopup="listbox"
        aria-expanded={open()}
        aria-label={props.label}
      >
        {Row(current())}
        <svg class="pickcaret" viewBox="0 0 10 6" aria-hidden="true">
          <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.4" />
        </svg>
      </button>
      <Show when={open()}>
        <div class="picklist" role="listbox" ref={(el) => (list = el)}>
          <For each={props.options}>
            {(o, i) => (
              <div
                role="option"
                aria-selected={i() === at()}
                class={`pickopt ${i() === at() ? "at" : ""}`}
                onPointerEnter={() => preview(i())}
                onClick={() => {
                  setAt(i());
                  close(true);
                }}
              >
                {Row(o)}
                <span class={`picktick ${o.id === props.value ? "on" : ""}`}>✓</span>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
