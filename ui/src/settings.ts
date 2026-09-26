import type { WaveColour, WaveShape } from "./Waveform";
import { CUSTOM, DEFAULT_FONT, UI_FONTS, cleanFamily } from "./fonts";
import { DEFAULT_THEME, isTheme, type ThemeName } from "./themes";
import type { MeterMode } from "./Meters";

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
/**
 * One value, and the colour it is shown in.
 *
 * The colour belongs to the value rather than to the row. A row painted one
 * colour makes six moods into six identical buttons, which is the opposite of
 * the point — and which colour means "dark" is a judgement only the person
 * tagging can make.
 */
export type QuickValue = {
  value: string;
  colour?: ColourName;
};

export type QuickGroup = {
  label: string;
  /** Tag field written to: `ENERGY`, `MOOD`, `GENRE`, or anything else. */
  field: string;
  values: QuickValue[];
  /** Whether a track can carry several of these at once. */
  multi: boolean;
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
 * The colour one value is shown in.
 *
 * Falls back to the palette position rather than to a single default, so values
 * nobody has chosen a colour for still differ from each other — which is what
 * makes the feature useful before it has been configured.
 */
export function valueColour(value: QuickValue, index: number) {
  return (
    TAG_COLOURS.find((c) => c.name === value.colour) ??
    TAG_COLOURS[index % TAG_COLOURS.length]
  );
}

/**
 * Rebuilds a row's values from the comma-separated box, keeping colours.
 *
 * Matched by name, so renaming a value loses its colour and reordering or
 * inserting one does not. Typing the list is far quicker than adding values one
 * at a time; having that discard every colour in the row would not be.
 */
export function valuesFrom(text: string, previous: QuickValue[]): QuickValue[] {
  const kept = new Map(previous.map((v) => [v.value.toLowerCase(), v.colour]));

  return text
    .split(",")
    .map((v) => v.trim())
    .filter(Boolean)
    .map((value, index) => ({
      value,
      colour:
        kept.get(value.toLowerCase()) ??
        TAG_COLOURS[index % TAG_COLOURS.length].name,
    }));
}

export type Settings = {
  wave: { shape: WaveShape; colour: WaveColour };
  quick: QuickGroup[];
  /** Whether the quick-tag panel offers a star rating row. */
  quickRating: boolean;
  /**
   * Whether listings hide files that are not audio.
   *
   * Folders always survive it — see `Show` in `api.ts`. Off by default: a
   * browser that silently omits files is lying about what is on the disk, and
   * that is a thing to opt into rather than to discover.
   */
  tracksOnly: boolean;
  /** Name of the interface face — see `UI_FONTS` in `fonts.ts`. */
  font: string;
  /** Family typed by hand, used only while `font` is `custom`. */
  fontCustom: string;
  /**
   * Whether a folder holding no audio is drawn as icons rather than as a list.
   *
   * On by default: every column but the name is empty for a directory, so a
   * folder of folders spends a whole screen's width saying nothing.
   */
  folderGrid: boolean;
  /** Colour scheme - see `THEMES` in `themes.ts`. */
  theme: ThemeName;
  /** What the meter box in the transport shows, or `off` for no box. */
  meter: MeterMode;
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
    // Cool to warm, because energy has a direction and the colours should read
    // in the same one.
    values: [
      { value: "1", colour: "blue" },
      { value: "2", colour: "teal" },
      { value: "3", colour: "green" },
      { value: "4", colour: "amber" },
      { value: "5", colour: "red" },
    ],
    multi: false,
  },
  {
    label: "Mood",
    field: "MOOD",
    values: [
      { value: "Dark", colour: "violet" },
      { value: "Rolling", colour: "blue" },
      { value: "Peak", colour: "red" },
      { value: "Deep", colour: "teal" },
      { value: "Uplifting", colour: "amber" },
      { value: "Weird", colour: "green" },
    ],
    multi: true,
  },
];

export const DEFAULTS: Settings = {
  wave: { shape: "filled", colour: "rgb" },
  quick: DEFAULT_QUICK,
  quickRating: true,
  tracksOnly: false,
  font: DEFAULT_FONT.name,
  fontCustom: "",
  folderGrid: true,
  theme: DEFAULT_THEME,
  meter: "vu",
};

const KEY = "noceur.settings";

const SHAPES: WaveShape[] = ["filled", "bars"];
const COLOURS: WaveColour[] = ["rgb", "threeband", "blue", "accent"];
const METERS: MeterMode[] = ["off", "vu", "needle", "scope", "vector", "spectrum"];

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
        ? parsed.quick.filter(isGroup).map(readGroup)
        : DEFAULT_QUICK,
      quickRating: parsed.quickRating !== false,
      tracksOnly: parsed.tracksOnly === true,
      // A name no build knows about falls back rather than leaving the app in
      // a face it cannot show in the picker.
      font: known(parsed.font) ? parsed.font : DEFAULTS.font,
      fontCustom:
        typeof parsed.fontCustom === "string" ? cleanFamily(parsed.fontCustom) : "",
      folderGrid: parsed.folderGrid !== false,
      theme: isTheme(parsed.theme) ? parsed.theme : DEFAULTS.theme,
      meter: parsed.meter && METERS.includes(parsed.meter) ? parsed.meter : DEFAULTS.meter,
    };
  } catch {
    return DEFAULTS;
  }
}

/** Whether a stored font name is one this build offers. */
function known(name: unknown): name is string {
  return name === CUSTOM || UI_FONTS.some((f) => f.name === name);
}

/** Guards a stored group, since this file is editable by hand. */
function isGroup(g: unknown): g is QuickGroup {
  const q = g as { label?: unknown; field?: unknown; values?: unknown };
  return (
    Boolean(q) &&
    typeof q.label === "string" &&
    typeof q.field === "string" &&
    Array.isArray(q.values)
  );
}

/**
 * Brings a stored row up to date.
 *
 * Values used to be plain strings and are now values with colours, so settings
 * written by an earlier build arrive as an array of strings. They are converted
 * rather than discarded: losing somebody's tag vocabulary to a change in how it
 * is stored would be the worst possible reason to lose it.
 *
 * A colour naming nothing is dropped and the value keeps its place.
 */
function readGroup(g: QuickGroup): QuickGroup {
  const values: QuickValue[] = (g.values as unknown[])
    .map((v, index) => {
      if (typeof v === "string") {
        return { value: v, colour: TAG_COLOURS[index % TAG_COLOURS.length].name };
      }
      const entry = v as QuickValue;
      if (!entry || typeof entry.value !== "string") return null;
      return TAG_COLOURS.some((c) => c.name === entry.colour)
        ? entry
        : { value: entry.value };
    })
    .filter((v): v is QuickValue => v !== null);

  return { ...g, values };
}

export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(settings));
  } catch {
    // A full or disabled store costs the preference, not the session.
  }
}
