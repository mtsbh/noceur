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
  /**
   * Where this row's colours start.
   *
   * Each value takes the next colour along, so a row of six values is six
   * different colours rather than six of the same one — telling them apart at a
   * glance is the entire reason for colouring them. This picks which colour the
   * row begins at, so two rows need not look alike.
   */
  colour?: ColourName;
};

export type ColourName =
  | "amber"
  | "red"
  | "violet"
  | "blue"
  | "teal"
  | "green"
  | "grey";

/**
 * The colours a quick-tag row can be marked in.
 *
 * Seven, and no colour picker. A palette chosen against this theme stays
 * readable on it; a free choice produces the one row nobody can read, and the
 * point of colouring a row is telling it apart at a glance mid-set.
 *
 * Each carries its own fill rather than deriving one, so the buttons do not
 * depend on `color-mix` being available in whatever web view is hosting this.
 */
export const TAG_COLOURS: {
  name: ColourName;
  label: string;
  line: string;
  fill: string;
}[] = [
  { name: "amber", label: "Amber", line: "#e8a55f", fill: "rgba(232, 165, 95, 0.16)" },
  { name: "red", label: "Red", line: "#e2685e", fill: "rgba(226, 104, 94, 0.16)" },
  { name: "violet", label: "Violet", line: "#a98cf0", fill: "rgba(169, 140, 240, 0.16)" },
  { name: "blue", label: "Blue", line: "#6aa9e8", fill: "rgba(106, 169, 232, 0.16)" },
  { name: "teal", label: "Teal", line: "#5fc6b8", fill: "rgba(95, 198, 184, 0.16)" },
  { name: "green", label: "Green", line: "#86c46a", fill: "rgba(134, 196, 106, 0.16)" },
  { name: "grey", label: "Grey", line: "#9aa0a6", fill: "rgba(154, 160, 166, 0.16)" },
];

/** The palette entry for a name, or the first when there is none. */
export function colourOf(name?: ColourName) {
  return TAG_COLOURS.find((c) => c.name === name) ?? TAG_COLOURS[0];
}

/**
 * The colour of one value in a row.
 *
 * Stepped from the row's own starting colour and wrapped, so every value in a
 * row differs from its neighbours and a row with more values than the palette
 * has repeats rather than running out.
 */
export function valueColour(group: QuickGroup, index: number) {
  const start = Math.max(
    0,
    TAG_COLOURS.findIndex((c) => c.name === group.colour),
  );
  return TAG_COLOURS[(start + index) % TAG_COLOURS.length];
}

export type Settings = {
  wave: { shape: WaveShape; colour: WaveColour };
  quick: QuickGroup[];
  /** Whether the quick-tag panel offers a star rating row. */
  quickRating: boolean;
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
  {
    label: "Energy",
    field: "ENERGY",
    values: ["1", "2", "3", "4", "5"],
    multi: false,
    colour: "amber",
  },
  {
    label: "Mood",
    field: "MOOD",
    values: ["Dark", "Rolling", "Peak", "Deep", "Uplifting", "Weird"],
    multi: true,
    colour: "violet",
  },
];

export const DEFAULTS: Settings = {
  wave: { shape: "filled", colour: "rgb" },
  quick: DEFAULT_QUICK,
  quickRating: true,
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
      quick: Array.isArray(parsed.quick)
        ? parsed.quick.filter(isGroup).map(withKnownColour)
        : DEFAULT_QUICK,
      quickRating: parsed.quickRating !== false,
    };
  } catch {
    return DEFAULTS;
  }
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

/**
 * Drops a colour that names nothing.
 *
 * The row keeps its field and its values and simply loses the marking — a
 * palette that shrank between builds must not take somebody's tag vocabulary
 * with it.
 */
function withKnownColour(g: QuickGroup): QuickGroup {
  if (g.colour && !TAG_COLOURS.some((c) => c.name === g.colour)) {
    const { colour: _dropped, ...rest } = g;
    return rest;
  }
  return g;
}

export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(settings));
  } catch {
    // A full or disabled store costs the preference, not the session.
  }
}
