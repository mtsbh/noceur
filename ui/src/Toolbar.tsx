import { For, Show, createSignal, onCleanup, type JSX } from "solid-js";
import { BAR_ICONS, BoltIcon, CloseIcon, barIcon } from "./icons";
import type { BarLayout } from "./settings";

/** A thing the toolbar can do to the selection. */
export type ActionId =
  | "tags"
  | "match"
  | "identify"
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
  /** The group it lives in. Grouped items are reached through their group. */
  group?: GroupId;
  /** A group cell: everything in it, in the order its list shows them. */
  members?: () => BarItem[];
  /** A group cell: the members drawn as icons beside its arrow. */
  lead?: () => string[];
  /** A group cell: shows or hides one member as an icon. */
  onLead?: (id: string) => void;
};

export type GroupId = "tags" | "fix" | "analyse" | "files" | "send";

/**
 * The five groups the tools are sorted into, in bar order. What a track says,
 * what is done to its name, what is measured, what happens to the file, and
 * where it goes next.
 */
export const GROUPS: { id: GroupId; label: string }[] = [
  { id: "tags", label: "Tags" },
  { id: "fix", label: "Fix" },
  { id: "analyse", label: "Analyse" },
  { id: "files", label: "Files" },
  { id: "send", label: "Send" },
];

/** What each group shows as icons before anyone has chosen. */
export const DEFAULT_LEAD: Record<GroupId, string[]> = {
  tags: ["tags", "quick"],
  fix: ["rename"],
  analyse: ["quality", "bpm"],
  files: ["convert"],
  send: [],
};

/** Tools that start in the tray: they belong in the right-click menu. */
export const DEFAULT_OUT = ["paste", "newfolder"];

/** Bumped when the default arrangement changes enough that old ones are dropped. */
export const BAR_VERSION = 2;
export const isGroup = (id: string) => id.startsWith("group:");

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
  // An arrangement from before the groups is put back to the default one: its
  // order is a list of single tools that now live inside groups.
  const current = stored && stored.v === BAR_VERSION ? stored : null;
  const order = (current?.order ?? defaults).filter(
    (id) => isSpacer(id) || (known.has(id) && known.get(id)!.forced !== false),
  );
  const hidden = current?.hidden ?? [];
  for (const item of items) {
    // Grouped tools are reached through their group, never appended alone.
    if (order.includes(item.id) || item.group) continue;
    const wanted = item.forced ?? (item.auto !== false && !hidden.includes(item.id));
    if (wanted) order.push(item.id);
  }
  return {
    v: BAR_VERSION,
    order,
    hidden,
    icons: stored?.icons ?? {},
    lead: current?.lead ?? {},
    out: current?.out ?? DEFAULT_OUT,
  };
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

  /** A group's tools shown as icons, less any that stand on the bar alone. */
  const leadsOf = (g: BarItem) => {
    const chosen = g.lead?.() ?? [];
    return inGroup(g).filter((m) => chosen.includes(m.id) && !props.layout.order.includes(m.id));
  };

  /**
   * A group's tools, less those taken out to the tray or standing on the bar
   * alone: every tool is in one place at a time, as in Firefox's customising.
   */
  const inGroup = (g: BarItem) => {
    const out = props.layout.out ?? [];
    return (g.members?.() ?? []).filter(
      (m) => !m.absent?.() && !out.includes(m.id) && !props.layout.order.includes(m.id),
    );
  };

  /**
   * The tray: what is in neither the bar nor a group. A tool still in its group
   * lives there, not here.
   */
  const spare = () => {
    const out = props.layout.out ?? [];
    return props.items.filter(
      (i) => !props.layout.order.includes(i.id) && (!i.group || out.includes(i.id)),
    );
  };

  /** Every group swapped for all of its tools, side by side: no groups at all. */
  function flatten() {
    const order = props.layout.order.flatMap((id) => {
      const g = isGroup(id) ? byId(id) : undefined;
      if (!g?.members) return [id];
      return inGroup(g).map((m) => m.id);
    });
    props.onLayout({ ...props.layout, order });
  }

  /* ── Group dropdowns ─────────────────────────────────────────────────── */

  const [openGroup, setOpenGroup] = createSignal<{ id: string; x: number; y: number } | null>(null);
  let openTimer: number | undefined;
  let closeTimer: number | undefined;

  /** Opens a group's list under its arrow; hover waits a beat so passing over does not. */
  function openAt(id: string, el: HTMLElement, delay: number) {
    clearTimeout(closeTimer);
    clearTimeout(openTimer);
    if (props.editing) return;
    openTimer = window.setTimeout(() => {
      const r = el.closest(".bargroup")!.getBoundingClientRect();
      setOpenGroup({ id, x: Math.min(r.left, window.innerWidth - 250), y: r.bottom + 4 });
    }, delay);
  }
  function closeSoon() {
    clearTimeout(openTimer);
    closeTimer = window.setTimeout(() => setOpenGroup(null), 220);
  }
  function stay() {
    clearTimeout(closeTimer);
  }
  onCleanup(() => {
    clearTimeout(openTimer);
    clearTimeout(closeTimer);
  });

  /** The group cell: its lead icons, then the arrow that opens the rest. */
  const Group = (p: { item: BarItem }) => {
    const leads = () => leadsOf(p.item);
    const open = () => openGroup()?.id === p.item.id;
    const Glyph = () => {
      const I = iconOf(p.item);
      return <I />;
    };
    return (
      <div class={`bargroup ${open() ? "open" : ""}`} data-group={p.item.id}>
        <For each={leads()}>
          {(m) => {
            const I = iconOf(m);
            return (
              <span
                class="bargroupicon"
                onPointerDown={(e) => {
                  if (!props.editing) return;
                  e.stopPropagation();
                  begin(e, m.id, true);
                }}
              >
                <button
                  class={`toolbtn ${m.off?.() ? "off" : ""} ${m.on?.() ? "on" : ""}`}
                  data-tip={m.label}
                  aria-label={m.label}
                  aria-disabled={m.off?.() ?? false}
                  onClick={(e) => {
                    if (props.editing || m.off?.()) return;
                    m.run?.(e);
                  }}
                >
                  <I />
                </button>
                {/* While customising, each icon comes off on its own; the
                    group's own × takes the whole group. */}
                <Show when={props.editing}>
                  <button
                    class="barx one"
                    aria-label={`Remove ${m.label} icon`}
                    onPointerDown={(e) => e.stopPropagation()}
                    onClick={() => remove(m.id)}
                  >
                    <CloseIcon />
                  </button>
                </Show>
              </span>
            );
          }}
        </For>
        {/* With no icon chosen the group shows its own, so it is never just
            an arrow on its own. */}
        <button
          class={`toolbtn bargroupbtn ${leads().length ? "caretonly" : ""}`}
          data-tip={open() ? undefined : p.item.label}
          aria-label={`${p.item.label} actions`}
          aria-haspopup="menu"
          aria-expanded={open()}
          onPointerEnter={(e) => openAt(p.item.id, e.currentTarget, 140)}
          onPointerLeave={closeSoon}
          onClick={(e) => {
            if (props.editing) return;
            if (open()) setOpenGroup(null);
            else openAt(p.item.id, e.currentTarget, 0);
          }}
        >
          <Show when={!leads().length}>
            <Glyph />
          </Show>
          <svg class="bargroupcaret" viewBox="0 0 10 6" aria-hidden="true">
            <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.4" />
          </svg>
        </button>
      </div>
    );
  };

  /** The open group's list: every tool in it, with a tick for "show as icon". */
  const Drop = (p: { at: { id: string; x: number; y: number } }) => {
    const group = () => byId(p.at.id);
    const lead = () => group()?.lead?.() ?? [];
    return (
      <div
        class="bardrop"
        role="menu"
        style={{ left: `${p.at.x}px`, top: `${p.at.y}px` }}
        onPointerEnter={stay}
        onPointerLeave={closeSoon}
      >
        <div class="bardrophead">{group()?.label}</div>
        <For each={group() ? inGroup(group()!) : []}>
          {(m) => {
            const I = iconOf(m);
            return (
              <div
                class={`bardropitem ${m.off?.() ? "off" : ""}`}
                role="menuitem"
                onClick={(e) => {
                  if (m.off?.()) return;
                  setOpenGroup(null);
                  m.run?.(e as MouseEvent & { currentTarget: HTMLElement });
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  group()?.onLead?.(m.id);
                }}
              >
                <span class="bardropicon">
                  <I />
                </span>
                <span class="bardroplabel">{m.label}</span>
                <button
                  class={`bardroppin ${lead().includes(m.id) ? "on" : ""}`}
                  data-tip={lead().includes(m.id) ? "Hide icon" : "Show as icon"}
                  data-tip-at="end"
                  aria-label={lead().includes(m.id) ? `Hide ${m.label} icon` : `Show ${m.label} as icon`}
                  aria-pressed={lead().includes(m.id)}
                  onClick={(e) => {
                    e.stopPropagation();
                    group()?.onLead?.(m.id);
                  }}
                >
                  ✓
                </button>
                <button
                  class="bardroppin bardropout"
                  data-tip="Remove from this list"
                  data-tip-at="end"
                  aria-label={`Remove ${m.label} from this list`}
                  onClick={(e) => {
                    e.stopPropagation();
                    remove(m.id);
                  }}
                >
                  <CloseIcon />
                </button>
              </div>
            );
          }}
        </For>
        <Show when={group() && !inGroup(group()!).length}>
          <div class="bardropempty">Empty. Customize the toolbar to drag tools back in.</div>
        </Show>
      </div>
    );
  };

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

  /** The group cell under the pointer, when it is the dragged tool's own. */
  function groupAt(x: number, y: number, id: string): BarItem | undefined {
    const tool = byId(id);
    if (!tool?.group) return undefined;
    const g = byId(`group:${tool.group}`);
    const el = g && cells.get(g.id);
    if (!el?.isConnected || !props.layout.order.includes(g!.id)) return undefined;
    const r = el.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top - 6 && y <= r.bottom + 6 ? g : undefined;
  }

  /** Puts a tool back in its group, as one of its icons. */
  function backInto(g: BarItem, id: string) {
    props.onLayout({
      ...props.layout,
      order: props.layout.order.filter((o) => o !== id),
      hidden: props.layout.hidden.filter((h) => h !== id),
      out: (props.layout.out ?? []).filter((o) => o !== id),
    });
    if (!(g.lead?.() ?? []).includes(id)) g.onLead?.(id);
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
      // A press on a group's icon is not a click on a bar cell.
      if (!d.moved) return fromBar && !props.layout.order.includes(id) ? undefined : click(id, fromBar, ev);
      const hidden = props.layout.hidden.filter((h) => h !== id);
      const home = groupAt(ev.clientX, ev.clientY, id);
      if (home) return backInto(home, id);
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
    if (!item || item.render || item.members) return;
    const el = cells.get(id);
    const r = el?.getBoundingClientRect();
    // Kept inside the window: the picker is eight icons wide.
    const x = Math.min(r ? r.left : e.clientX, window.innerWidth - 250);
    setPicker({ id, x, y: r ? r.bottom + 6 : e.clientY });
  }

  function remove(id: string) {
    const out = props.layout.out ?? [];
    props.onLayout({
      ...props.layout,
      // A grouped tool taken off goes to the tray, not back into its group.
      out: byId(id)?.group && !out.includes(id) ? [...out, id] : out,
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

  // Escape closes an open group, and leaves customising, the way it leaves
  // every other mode.
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape" && openGroup()) {
      e.preventDefault();
      e.stopPropagation();
      setOpenGroup(null);
      return;
    }
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
                  when={!item.members}
                  fallback={<Group item={item} />}
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
            <span class="barhint">Drag anything to or from the tray · drop a tool on its group to put it back · click to change icon</span>
            <button
              class="btn"
              onClick={flatten}
              data-tip="Every tool as its own icon, no groups"
            >
              All icons
            </button>
            <button class="btn" onClick={props.onReset} data-tip="Back to the groups">
              Reset
            </button>
            <button class="btn" onClick={props.onDone}>Done</button>
          </div>
        </div>
      </Show>

      <Show when={drag()?.moved}>
        <Ghost />
      </Show>

      <Show when={!props.editing && openGroup()}>{(at) => <Drop at={at()} />}</Show>

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
