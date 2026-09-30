import { For, Show, createSignal, onCleanup, type JSX } from "solid-js";
import { BAR_ICONS, BoltIcon, CloseIcon, barIcon } from "./icons";
import type { BarLayout } from "./settings";

/** A thing the toolbar can do to the selection. */
export type ActionId =
  | "tags"
  | "match"
  | "art"
  | "rename"
  | "clean"
  | "quality"
  | "bpm"
  | "copies"
  | "quick"
  | "convert"
  | "fromname";

/** One thing that can sit in the top bar. */
export type BarItem = {
  id: string;
  label: string;
  icon?: () => JSX.Element;
  /** A widget drawn instead of a single button — the art-size switch. */
  render?: () => JSX.Element;
  on?: () => boolean;
  /** Unavailable right now (nothing selected), but still in its place. */
  off?: () => boolean;
  /** Not drawn right now at all — the split direction with only one pane. */
  absent?: () => boolean;
  /** Put in the bar by itself the first time it exists. Defaults to true. */
  auto?: boolean;
  /**
   * Whether it is in the bar, decided elsewhere — an "Open with" program's own
   * toolbar switch. Overrides `auto` and the removed list either way.
   */
  forced?: boolean;
  /** Told when the bar adds or removes it, so that switch can follow. */
  onToggle?: (inBar: boolean) => void;
  run?: (e: MouseEvent & { currentTarget: HTMLElement }) => void;
};

type SpacerKind = "sep" | "space" | "flex";

const SPACERS: { kind: SpacerKind; label: string }[] = [
  { kind: "sep", label: "Separator" },
  { kind: "space", label: "Space" },
  { kind: "flex", label: "Flexible space" },
];

export const isSpacer = (id: string) => /^(sep|space|flex):/.test(id);
const spacerKind = (id: string) => id.split(":")[0] as SpacerKind;
export const newSpacer = (kind: SpacerKind) =>
  `${kind}:${Math.random().toString(36).slice(2, 8)}`;

/**
 * The bar as it should be drawn: the stored order with anything unknown
 * dropped, and anything new — a tool from a later build, a button you just gave
 * an icon — appended unless you took it off before.
 */
export function resolveBar(
  stored: BarLayout | null,
  items: BarItem[],
  defaults: string[],
): BarLayout {
  const known = new Map(items.map((i) => [i.id, i]));
  const order = (stored?.order ?? defaults).filter(
    (id) => isSpacer(id) || (known.has(id) && known.get(id)!.forced !== false),
  );
  const hidden = stored?.hidden ?? [];
  for (const item of items) {
    if (order.includes(item.id)) continue;
    const wanted = item.forced ?? (item.auto !== false && !hidden.includes(item.id));
    if (wanted) order.push(item.id);
  }
  return { order, hidden, icons: stored?.icons ?? {} };
}

/** Where a drag is and what it would do if let go now. */
type Drag = {
  id: string;
  fromBar: boolean;
  x: number;
  y: number;
  /** Index in the bar it would land at, or `null` when outside the bar. */
  into: number | null;
  moved: boolean;
};

/**
 * The top bar: every button in it, in whatever order you put them.
 *
 * Right-click the bar to customise it. While customising, buttons are dragged
 * to reorder, dragged off (or ×) to remove, clicked to change their icon, and
 * the tray underneath holds what is not in the bar plus separators and spaces.
 */
export default function Toolbar(props: {
  items: BarItem[];
  layout: BarLayout;
  editing: boolean;
  onLayout: (next: BarLayout) => void;
  onReset: () => void;
  onDone: () => void;
}) {
  const [drag, setDrag] = createSignal<Drag | null>(null);
  const [picker, setPicker] = createSignal<{ id: string; x: number; y: number } | null>(null);
  let bar: HTMLDivElement | undefined;
  const cells = new Map<string, HTMLElement>();

  const byId = (id: string) => props.items.find((i) => i.id === id);
  const iconOf = (item: BarItem) => barIcon(props.layout.icons[item.id]) ?? item.icon ?? BoltIcon;

  /** The order on screen, with a drag in progress shown where it would land. */
  const shown = () => {
    const d = drag();
    if (!d || !d.moved) return props.layout.order;
    const rest = props.layout.order.filter((id) => id !== d.id);
    if (d.into === null) return rest;
    return [...rest.slice(0, d.into), d.id, ...rest.slice(d.into)];
  };

  /** What can be added: everything not already in the bar. */
  const spare = () => props.items.filter((i) => !props.layout.order.includes(i.id));

  /** Which slot a pointer at `x, y` points into, or `null` when off the bar. */
  function slotAt(x: number, y: number, id: string): number | null {
    if (!bar) return null;
    const box = bar.getBoundingClientRect();
    if (y < box.top - 24 || y > box.bottom + 24 || x < box.left - 40 || x > box.right + 40) {
      return null;
    }
    const others = props.layout.order.filter((o) => o !== id);
    let into = 0;
    for (const other of others) {
      const el = cells.get(other);
      if (!el || !el.isConnected) continue;
      const r = el.getBoundingClientRect();
      if (x > r.left + r.width / 2) into = others.indexOf(other) + 1;
    }
    return into;
  }

  function begin(e: PointerEvent, id: string, fromBar: boolean) {
    if (e.button !== 0) return;
    e.preventDefault();
    const startX = e.clientX;
    const startY = e.clientY;
    setPicker(null);
    setDrag({ id, fromBar, x: startX, y: startY, into: null, moved: false });

    const move = (ev: PointerEvent) => {
      const d = drag();
      if (!d) return;
      const moved = d.moved || Math.hypot(ev.clientX - startX, ev.clientY - startY) > 4;
      setDrag({ ...d, x: ev.clientX, y: ev.clientY, moved, into: moved ? slotAt(ev.clientX, ev.clientY, id) : null });
    };
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      const d = drag();
      setDrag(null);
      if (!d) return;
      if (!d.moved) return click(id, fromBar, ev);
      const hidden = props.layout.hidden.filter((h) => h !== id);
      if (d.into !== null) {
        const rest = props.layout.order.filter((o) => o !== id);
        const order = [...rest.slice(0, d.into), id, ...rest.slice(d.into)];
        props.onLayout({ ...props.layout, order, hidden });
        if (!fromBar) byId(id)?.onToggle?.(true);
      } else if (fromBar) {
        remove(id);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  /** A press without a drag: add from the tray, or pick an icon in the bar. */
  function click(id: string, fromBar: boolean, e: PointerEvent) {
    if (!fromBar) {
      props.onLayout({
        ...props.layout,
        order: [...props.layout.order, id],
        hidden: props.layout.hidden.filter((h) => h !== id),
      });
      byId(id)?.onToggle?.(true);
      return;
    }
    const item = byId(id);
    if (!item || item.render) return;
    const el = cells.get(id);
    const r = el?.getBoundingClientRect();
    // Kept inside the window: the picker is eight icons wide.
    const x = Math.min(r ? r.left : e.clientX, window.innerWidth - 250);
    setPicker({ id, x, y: r ? r.bottom + 6 : e.clientY });
  }

  function remove(id: string) {
    props.onLayout({
      ...props.layout,
      order: props.layout.order.filter((o) => o !== id),
      hidden: isSpacer(id) || props.layout.hidden.includes(id)
        ? props.layout.hidden
        : [...props.layout.hidden, id],
    });
    byId(id)?.onToggle?.(false);
  }

  function chooseIcon(id: string, name: string | null) {
    const icons = { ...props.layout.icons };
    if (name) icons[id] = name;
    else delete icons[id];
    props.onLayout({ ...props.layout, icons });
    setPicker(null);
  }

  // Escape leaves customising, the way it leaves every other mode.
  const onKey = (e: KeyboardEvent) => {
    if (!props.editing || e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    if (picker()) setPicker(null);
    else props.onDone();
  };
  window.addEventListener("keydown", onKey, true);
  onCleanup(() => window.removeEventListener("keydown", onKey, true));

  /**
   * Tooltips near the window's right edge open leftwards, so they stay on
   * screen wherever a button has been moved to.
   */
  const placeTip = (e: PointerEvent) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-tip]");
    if (!el) return;
    const r = el.getBoundingClientRect();
    if (r.right > window.innerWidth - 90) el.setAttribute("data-tip-at", "end");
  };

  const Cell = (p: { id: string }) => {
    const dragging = () => drag()?.moved && drag()?.id === p.id;
    return (
      <Show
        when={!isSpacer(p.id)}
        fallback={
          <div
            ref={(el) => cells.set(p.id, el)}
            class={`baritem tspacer ${spacerKind(p.id)} ${dragging() ? "dragging" : ""}`}
            onPointerDown={(e) => props.editing && begin(e, p.id, true)}
          >
            <Show when={props.editing}>
              <button class="barx" aria-label="Remove" onPointerDown={(e) => e.stopPropagation()} onClick={() => remove(p.id)}>
                <CloseIcon />
              </button>
            </Show>
          </div>
        }
      >
        {(() => {
          const item = byId(p.id);
          if (!item) return null;
          const Glyph = () => {
            const I = iconOf(item);
            return <I />;
          };
          return (
            <Show when={props.editing || !item.absent?.()}>
              <div
                ref={(el) => cells.set(p.id, el)}
                class={`baritem ${dragging() ? "dragging" : ""} ${item.absent?.() ? "absent" : ""}`}
                onPointerDown={(e) => props.editing && begin(e, p.id, true)}
                onPointerOver={placeTip}
              >
                <Show
                  when={item.render}
                  fallback={
                    <button
                      class={`toolbtn ${item.off?.() ? "off" : ""} ${item.on?.() ? "on" : ""}`}
                      // `data-tip` rather than `title`: the browser's own tooltip
                      // is slow, OS-coloured, and sits under a window that draws
                      // its own title bar.
                      data-tip={item.label}
                      aria-label={item.label}
                      // Marked rather than `disabled`: a disabled button gets no
                      // mouse events here and would lose its tooltip.
                      aria-disabled={item.off?.() ?? false}
                      onClick={(e) => {
                        if (props.editing || item.off?.()) return;
                        item.run?.(e);
                      }}
                    >
                      <Glyph />
                    </button>
                  }
                >
                  {item.render!()}
                </Show>
                <Show when={props.editing}>
                  <button class="barx" aria-label={`Remove ${item.label}`} onPointerDown={(e) => e.stopPropagation()} onClick={() => remove(p.id)}>
                    <CloseIcon />
                  </button>
                </Show>
              </div>
            </Show>
          );
        })()}
      </Show>
    );
  };

  /** What follows the pointer while dragging. */
  const Ghost = () => {
    const d = drag()!;
    const item = byId(d.id);
    const I = item ? iconOf(item) : null;
    return (
      <div class="barghost" style={{ left: `${d.x + 10}px`, top: `${d.y + 12}px` }}>
        <Show when={I} fallback={SPACERS.find((s) => s.kind === spacerKind(d.id))?.label}>
          {I && <I />}
        </Show>
        <Show when={d.fromBar && d.into === null}>
          <span class="barghostx">remove</span>
        </Show>
      </div>
    );
  };

  return (
    <>
      <div class={`toolbar ${props.editing ? "editing" : ""}`} ref={(el) => (bar = el)}>
        <For each={shown()}>{(id) => <Cell id={id} />}</For>
      </div>

      <Show when={props.editing}>
        <div class="bartray" onContextMenu={(e) => e.preventDefault()}>
          <div class="bartrayitems">
            <For each={SPACERS}>
              {(s) => (
                <div class="barchip" onPointerDown={(e) => begin(e, newSpacer(s.kind), false)}>
                  <span class={`chipspacer ${s.kind}`} />
                  {s.label}
                </div>
              )}
            </For>
            <For each={spare()}>
              {(item) => {
                const I = iconOf(item);
                return (
                  <div class="barchip" onPointerDown={(e) => begin(e, item.id, false)}>
                    <Show when={!item.render} fallback={<span class="chipword">S M L</span>}>
                      <I />
                    </Show>
                    {item.label}
                  </div>
                );
              }}
            </For>
          </div>
          <div class="bartrayfoot">
            <span class="barhint">Drag to arrange · click to change icon</span>
            <button class="btn" onClick={props.onReset}>Reset</button>
            <button class="btn" onClick={props.onDone}>Done</button>
          </div>
        </div>
      </Show>

      <Show when={drag()?.moved}>
        <Ghost />
      </Show>

      <Show when={picker()}>
        {(pk) => (
          <>
            <div class="scrim bare" onPointerDown={() => setPicker(null)} />
            <div class="baricons" style={{ left: `${pk().x}px`, top: `${pk().y}px` }}>
              <For each={BAR_ICONS}>
                {(ic) => (
                  <button
                    class={`bariconbtn ${props.layout.icons[pk().id] === ic.name ? "on" : ""}`}
                    data-tip={ic.label}
                    aria-label={ic.label}
                    onClick={() => chooseIcon(pk().id, ic.name)}
                  >
                    <ic.icon />
                  </button>
                )}
              </For>
              <button class="bariconreset" onClick={() => chooseIcon(pk().id, null)}>
                Default
              </button>
            </div>
          </>
        )}
      </Show>
    </>
  );
}
