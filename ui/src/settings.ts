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
  /** The top bar as arranged by hand, or `null` for the default arrangement. */
  bar: BarLayout | null;
  /** Programs offered under "Open with". */
  openWith: Launcher[];
  /** Sites offered under "Search in", and in the sidebar where ticked. */
  searchIn: Launcher[];
  /** Which round of default sites `searchIn` has been given; see `SEARCH_ROUND`. */
  searchRound: number;
};

/**
 * One entry of "Open with" or "Search in".
 *
 * `target` is a program's path for the first, and a web address for the
 * second — with `{artist}`, `{title}`, `{album}` or `{query}` where the track
 * goes, or nothing, in which case artist and title are added to the end.
 */
export type Launcher = {
  id: string;
  name: string;
  target: string;
  /** Whether it also has a button in the top bar. Off until asked for. */
  inBar: boolean;
  /** Search sites only: whether the track sidebar offers it. On unless taken off. */
  inSide: boolean;
};

export const newLauncherId = () => Math.random().toString(36).slice(2, 10);

const site = (id: string, name: string, target: string): Launcher => ({
  id,
  name,
  target,
  inBar: false,
  inSide: true,
});

export const DEFAULT_SEARCH: Launcher[] = [
  site("google", "Google", "https://www.google.com/search?q={query}"),
  site("youtube", "YouTube", "https://www.youtube.com/results?search_query={query}"),
  site("discogs", "Discogs", "https://www.discogs.com/search/?q={query}&type=all"),
  site("beatport", "Beatport", "https://www.beatport.com/search?q={query}"),
  site("bandcamp", "Bandcamp", "https://bandcamp.com/search?q={query}"),
  site("soundcloud", "SoundCloud", "https://soundcloud.com/search?q={query}"),
  site("juno", "Juno", "https://www.junodownload.com/search/?q%5Ball%5D%5B%5D={query}"),
];

/**
 * Which round of default sites a stored list has been given.
 *
 * A stored list replaces the defaults, so sites added to them later would
 * never reach anyone who already had one. Each round is offered once; a site
 * removed after that stays removed.
 */
const SEARCH_ROUND = 2;

/**
 * The top bar's arrangement.
 *
 * `order` holds item ids and spacers (`sep:…`, `space:…`, `flex:…`, each with a
 * unique suffix so the same kind can appear twice). `hidden` remembers what was
 * taken off, so a tool added by a later build — or a new button of your own —
 * turns up by itself without bringing back the ones you removed.
 */
export type BarLayout = {
  order: string[];
  hidden: string[];
  /** Icon chosen per item, by name from `BAR_ICONS`. */
  icons: Record<string, string>;
  /** Which arrangement this is; one from before the groups is replaced. */
  v?: number;
  /** Per group, the tools shown as icons beside its arrow. Absent = default. */
  lead?: Record<string, string[]>;
  /** Tools taken out of their group: no icon, not in its list. The right-click
   * menu on the files still has them. */
  out?: string[];
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
  bar: null,
  openWith: [],
  searchIn: DEFAULT_SEARCH,
  searchRound: SEARCH_ROUND,
};

const KEY = "noceur.settings";

const SHAPES: WaveShape[] = ["filled", "bars", "slider"];
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
      bar: readBar(parsed.bar),
      openWith: readLaunchers(parsed.openWith) ?? DEFAULTS.openWith,
      searchIn: seedSearch(readLaunchers(parsed.searchIn), parsed.searchRound),
      searchRound: SEARCH_ROUND,
    };
  } catch {
    return DEFAULTS;
  }
}

/** Guards a stored launcher list; `undefined` when there is none to read. */
function readLaunchers(raw: unknown): Launcher[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  return raw
    .filter((l) => l && typeof l.name === "string" && typeof l.target === "string")
    .map((l) => ({
      id: typeof l.id === "string" && l.id ? l.id : newLauncherId(),
      name: l.name,
      target: l.target,
      inBar: l.inBar === true,
      inSide: l.inSide !== false,
    }));
}

/** A stored search list, with any default sites from a newer round added once. */
function seedSearch(stored: Launcher[] | undefined, round: unknown): Launcher[] {
  if (!stored) return DEFAULT_SEARCH;
  if (typeof round === "number" && round >= SEARCH_ROUND) return stored;
  const have = new Set(stored.map((l) => l.id));
  return [...stored, ...DEFAULT_SEARCH.filter((l) => !have.has(l.id))];
}

/**
 * The address a search opens, for one track.
 *
 * Every value is URL-encoded on its way in, so an artist called "Simon & Garfunkel"
 * searches for that rather than for "Simon" and a stray parameter.
 */
export function searchUrl(
  template: string,
  track: { artist: string; title: string; album: string; name: string },
): string {
  const stem = track.name.replace(/\.[^.]+$/, "");
  const title = track.title || (track.artist ? "" : stem);
  const query = [track.artist, title].filter(Boolean).join(" ") || stem;
  const enc = encodeURIComponent;
  const values: Record<string, string> = {
    artist: enc(track.artist),
    title: enc(title),
    album: enc(track.album),
    query: enc(query),
  };
  const filled = template.replace(/\{(artist|title|album|query)\}/gi, (_, k: string) => values[k.toLowerCase()]);
  return filled === template ? template + enc(query) : filled;
}

/** Guards a stored bar layout, since this file is editable by hand. */
function readBar(raw: unknown): BarLayout | null {
  const b = raw as Partial<BarLayout> | null | undefined;
  if (!b || !Array.isArray(b.order)) return null;
  const strings = (v: unknown) =>
    Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  const icons: Record<string, string> = {};
  if (b.icons && typeof b.icons === "object") {
    for (const [k, v] of Object.entries(b.icons)) if (typeof v === "string") icons[k] = v;
  }
  const lead: Record<string, string[]> = {};
  if (b.lead && typeof b.lead === "object") {
    for (const [k, v] of Object.entries(b.lead)) lead[k] = strings(v);
  }
  return {
    order: strings(b.order),
    hidden: strings(b.hidden),
    icons,
    v: typeof b.v === "number" ? b.v : undefined,
    lead,
    out: Array.isArray(b.out) ? strings(b.out) : undefined,
  };
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
