import { ICON_NAMES, type IconName } from "./icons";
import type { WaveColour, WaveShape } from "./Waveform";

/**
 * Everything the app remembers about how it should look.
 *
 * One record in one key rather than a key per preference: these are read
 * together at startup and written together whenever the panel changes, and a
 * dozen separate entries would make it that much harder to see what the app has
 * stored about you — or to clear it.
 *
 * Deliberately not in the index. The database is a cache of what is on disk and
 * is safe to delete; preferences are not derived from anything and would be lost
 * with it.
 */
/**
 * One row of the quick-tag panel.
 *
 * A field to write, and the values that can go in it. The keys are positional —
 * the first value takes the first key of the group's row — so adding a value
 * never silently rebinds the others past it.
 */
export type QuickGroup = {
  label: string;
  /** Tag field written to: `ENERGY`, `MOOD`, `GENRE`, or anything else. */
  field: string;
  values: string[];
  /** Whether a track can carry several of these at once. */
  multi: boolean;
};

/**
 * A thing the toolbar can do to the selection.
 *
 * The same set the right-click menu offers. Nothing here is exclusive to the
 * bar — a button is a shortcut to a menu entry, so removing one from the bar
 * never removes the ability.
 */
export type ActionId = "tags" | "match" | "art" | "rename" | "clean" | "quick";

/** One button on the toolbar: what it does, what it looks like, whether it is there. */
export type ToolbarButton = { id: ActionId; icon: IconName; on: boolean };

/** What each action is called, and what the button's tooltip says. */
export const ACTIONS: Record<ActionId, { label: string; hint: string }> = {
  tags: { label: "Edit metadata", hint: "Edit the tags of the selected tracks" },
  match: { label: "Find metadata", hint: "Look the selected track up online" },
  art: { label: "Find album art", hint: "Search for cover art for the selection" },
  rename: { label: "Rename", hint: "Rename the selection from a template" },
  clean: { label: "Clean up", hint: "Fix names and tags with a set of rules" },
  quick: { label: "Quick tag", hint: "Tag the playing track by keyboard" },
};

export const ACTION_IDS = Object.keys(ACTIONS) as ActionId[];

/**
 * What the toolbar starts as.
 *
 * All of them on, because a bar that starts empty is a feature nobody finds.
 * The icons are the obvious pairing; every one of them can be swapped in
 * settings, which is the point of storing the icon rather than hard-coding it.
 */
export const DEFAULT_TOOLBAR: ToolbarButton[] = [
  { id: "tags", icon: "tag", on: true },
  { id: "match", icon: "globe", on: true },
  { id: "art", icon: "image", on: true },
  { id: "rename", icon: "rename", on: true },
  { id: "clean", icon: "broom", on: true },
  { id: "quick", icon: "star", on: true },
];

export type Settings = {
  wave: { shape: WaveShape; colour: WaveColour };
  quick: QuickGroup[];
  /** Whether the quick-tag panel offers a star rating row. */
  quickRating: boolean;
  /** The shortcut buttons in the path bar, in the order they appear. */
  toolbar: ToolbarButton[];
};

/** A group added by the "add" button, before it is named. */
export const NEW_QUICK_GROUP: QuickGroup = {
  label: "New",
  field: "",
  values: [],
  multi: false,
};

/**
 * The keys each group's values bind to, in order.
 *
 * Two rows of a keyboard rather than anything clever: the point of quick tag is
 * that a hand stays put while a folder goes past, and the digits and the top
 * letter row are where a hand already is.
 */
export const QUICK_KEYS: string[][] = [
  ["1", "2", "3", "4", "5", "6", "7", "8", "9"],
  ["q", "w", "e", "r", "t", "y", "u", "i", "o"],
  ["a", "s", "d", "f", "g", "h", "j", "k", "l"],
];

/**
 * What quick tag starts as.
 *
 * Energy and mood, because those are the two things a DJ decides about a track
 * while listening to it and cannot get from any database. Both are editable —
 * the fields as much as the values, since every library has its own vocabulary.
 */
export const DEFAULT_QUICK: QuickGroup[] = [
  { label: "Energy", field: "ENERGY", values: ["1", "2", "3", "4", "5"], multi: false },
  {
    label: "Mood",
    field: "MOOD",
    values: ["Dark", "Rolling", "Peak", "Deep", "Uplifting", "Weird"],
    multi: true,
  },
];

export const DEFAULTS: Settings = {
  wave: { shape: "filled", colour: "rgb" },
  quick: DEFAULT_QUICK,
  quickRating: true,
  toolbar: DEFAULT_TOOLBAR,
};

const KEY = "noceur.settings";

const SHAPES: WaveShape[] = ["filled", "bars"];
const COLOURS: WaveColour[] = ["rgb", "threeband", "blue", "accent"];

/**
 * Reads stored settings, falling back field by field.
 *
 * Merged against the defaults rather than replaced by them, so a build that adds
 * a preference does not discard the ones already set — and a corrupt or
 * half-written entry costs one setting rather than all of them.
 */
export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS;
    const parsed = JSON.parse(raw) as Partial<Settings>;
    const shape = parsed.wave?.shape;
    const colour = parsed.wave?.colour;
    return {
      wave: {
        shape: shape && SHAPES.includes(shape) ? shape : DEFAULTS.wave.shape,
        colour: colour && COLOURS.includes(colour) ? colour : DEFAULTS.wave.colour,
      },
      quick: Array.isArray(parsed.quick) ? parsed.quick.filter(isGroup) : DEFAULT_QUICK,
      quickRating: parsed.quickRating !== false,
      toolbar: mergeToolbar(parsed.toolbar),
    };
  } catch {
    return DEFAULTS;
  }
}

/**
 * Reconciles a stored toolbar with the actions this build has.
 *
 * Kept in the stored order, with anything unrecognised dropped and anything new
 * appended — so a build that adds an action shows it rather than hiding it
 * behind a preference written before it existed, and one that removes an action
 * does not leave a button that does nothing.
 */
function mergeToolbar(stored: unknown): ToolbarButton[] {
  if (!Array.isArray(stored)) return DEFAULT_TOOLBAR;

  const seen = new Set<ActionId>();
  const out: ToolbarButton[] = [];

  for (const entry of stored) {
    const b = entry as ToolbarButton;
    if (!b || !ACTION_IDS.includes(b.id) || seen.has(b.id)) continue;
    seen.add(b.id);
    out.push({
      id: b.id,
      icon: ICON_NAMES.includes(b.icon) ? b.icon : defaultIcon(b.id),
      on: b.on !== false,
    });
  }

  for (const fallback of DEFAULT_TOOLBAR) {
    if (!seen.has(fallback.id)) out.push(fallback);
  }
  return out;
}

function defaultIcon(id: ActionId): IconName {
  return DEFAULT_TOOLBAR.find((b) => b.id === id)?.icon ?? "tag";
}

/** Guards a stored group, since this file is editable by hand. */
function isGroup(g: unknown): g is QuickGroup {
  const q = g as QuickGroup;
  return (
    Boolean(q) &&
    typeof q.label === "string" &&
    typeof q.field === "string" &&
    Array.isArray(q.values) &&
    q.values.every((v) => typeof v === "string")
  );
}

export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(settings));
  } catch {
    // A full or disabled store costs the preference, not the session.
  }
}
