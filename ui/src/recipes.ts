import type { ActionKind } from "./api";

/**
 * Ready-made buttons, so nobody has to write an expression to get an ordinary
 * one.
 *
 * The expression language underneath is still foobar2000's — anything already
 * written for foobar, Mp3tag or OneTagger keeps working, which is most of why
 * it was chosen. But almost every button anybody actually wants is one of a
 * handful of named operations with a field and an argument or two, and asking
 * for
 *
 *     $ifgreater($strchr(%title%,'('),0,$trim($left(%title%,$sub($strchr(%title%,'('),1))),%title%)
 *
 * when what is meant is "drop everything from the bracket onwards" is the
 * interface failing, not the user. A recipe is that operation named, with the
 * expression generated from it. The generated text stays visible and editable,
 * so a recipe is a starting point rather than a cage.
 */

/** One plain-text box a recipe asks for. */
export type Slot = {
  label: string;
  placeholder: string;
  /** Prefilled when the recipe is chosen. */
  value?: string;
};

export type Recipe = {
  /** Stored on the action, so reopening settings shows the recipe again. */
  id: string;
  label: string;
  kind: ActionKind;
  /** One line, shown under the picker. Says what it does to a file. */
  hint: string;
  /** Whether it works on a named tag field, which the user picks. */
  field?: boolean;
  slots?: Slot[];
  /** Prefills the target box — a program, or a folder. */
  target?: string;
  /**
   * Builds the expression. `f` is the chosen tag field, `v` the slot values.
   *
   * Values are dropped in as foobar2000 literals: single-quoted, with any quote
   * of their own doubled. Without that, a comma typed into "replace this" would
   * end the argument and the expression would mean something else entirely —
   * which is exactly the class of mistake recipes exist to prevent.
   */
  build: (f: string, v: string[]) => string;
};

/** A user value as a foobar2000 literal. */
export function lit(text: string): string {
  return `'${text.replace(/'/g, "''")}'`;
}

/**
 * Everything up to the first `mark`, trimmed — or the whole value if it is not
 * there.
 *
 * The shape of this is forced by the language: there is no "index of, or the
 * whole string" so the position has to be tested before it is used, and
 * `$strchr` returns 1-based, hence the `$sub`.
 */
function before(field: string, mark: string): string {
  const at = `$strchr(%${field}%,${lit(mark)})`;
  return `$ifgreater(${at},0,$trim($left(%${field}%,$sub(${at},1))),%${field}%)`;
}

export const RECIPES: Recipe[] = [
  // ── Tags ──────────────────────────────────────────────────────────────────
  {
    id: "tag.titlecase",
    label: "Title Case",
    kind: "tag",
    hint: "the beatles → The Beatles",
    field: true,
    build: (f) => `$caps2(%${f}%)`,
  },
  {
    id: "tag.upper",
    label: "UPPERCASE",
    kind: "tag",
    hint: "The Beatles → THE BEATLES",
    field: true,
    build: (f) => `$upper(%${f}%)`,
  },
  {
    id: "tag.lower",
    label: "lowercase",
    kind: "tag",
    hint: "The Beatles → the beatles",
    field: true,
    build: (f) => `$lower(%${f}%)`,
  },
  {
    id: "tag.trim",
    label: "Trim spaces",
    kind: "tag",
    hint: "drops spaces from both ends",
    field: true,
    build: (f) => `$trim(%${f}%)`,
  },
  {
    id: "tag.brackets",
    label: "Remove text in brackets",
    kind: "tag",
    hint: "Track (Original Mix) → Track",
    field: true,
    slots: [{ label: "From", placeholder: "(", value: "(" }],
    build: (f, v) => before(f, v[0] || "("),
  },
  {
    id: "tag.year",
    label: "Year only",
    kind: "tag",
    hint: "2019-04-13 → 2019",
    field: true,
    build: (f) => `$left(%${f}%,4)`,
  },
  {
    id: "tag.first",
    label: "Keep the first N characters",
    kind: "tag",
    hint: "cuts the value to a length",
    field: true,
    slots: [{ label: "Keep", placeholder: "4", value: "4" }],
    build: (f, v) => `$left(%${f}%,${Number(v[0]) || 4})`,
  },
  {
    id: "tag.replace",
    label: "Replace text",
    kind: "tag",
    hint: "swaps one piece of text for another, everywhere it appears",
    field: true,
    slots: [
      { label: "Find", placeholder: "feat." },
      { label: "With", placeholder: "ft." },
    ],
    build: (f, v) => `$replace(%${f}%,${lit(v[0] ?? "")},${lit(v[1] ?? "")})`,
  },
  {
    id: "tag.separators",
    label: "One separator between artists",
    kind: "tag",
    hint: "A, B & C and D → A;B;C;D",
    field: true,
    slots: [{ label: "Use", placeholder: ";", value: ";" }],
    build: (f, v) => {
      const to = lit(v[0] || ";");
      return `$replace(%${f}%,',',${to},' & ',${to},'&',${to},' and ',${to})`;
    },
  },
  {
    id: "tag.merge",
    label: "Artist - Title into a field",
    kind: "tag",
    hint: "writes “Artist - Title” to the field you pick",
    field: true,
    build: () => `%artist% - %title%`,
  },

  // ── Renaming ──────────────────────────────────────────────────────────────
  {
    id: "rename.artist-title",
    label: "Artist - Title",
    kind: "rename",
    hint: "Bicep - Glue.wav",
    build: () => `%artist% - %title%`,
  },
  {
    id: "rename.track",
    label: "01. Artist - Title",
    kind: "rename",
    hint: "keeps an album in its own order",
    build: () => `$num(%tracknumber%,2). %artist% - %title%`,
  },
  {
    id: "rename.title",
    label: "Title only",
    kind: "rename",
    hint: "Glue.wav",
    build: () => `%title%`,
  },

  // ── Opening ───────────────────────────────────────────────────────────────
  {
    id: "open.program",
    label: "Open in a program",
    kind: "open",
    hint: "passes the selected files to it",
    target: "",
    build: () => `"%_path%"`,
  },
  {
    id: "open.telegram",
    label: "Send with Telegram",
    kind: "open",
    hint: "hands the file to Telegram Desktop",
    target: "Telegram.exe",
    build: () => `-sendpath "%_path%"`,
  },

  // ── Looking it up ─────────────────────────────────────────────────────────
  {
    id: "url.google",
    label: "Search Google",
    kind: "url",
    hint: "opens a search for artist and title",
    build: () => `https://www.google.com/search?q=%artist%+%title%`,
  },
  {
    id: "url.youtube",
    label: "Search YouTube",
    kind: "url",
    hint: "opens a search for artist and title",
    build: () => `https://www.youtube.com/results?search_query=%artist%+%title%`,
  },
  {
    id: "url.discogs",
    label: "Search Discogs",
    kind: "url",
    hint: "opens a search for artist and title",
    build: () => `https://www.discogs.com/search/?q=%artist%+%title%`,
  },
  {
    id: "url.site",
    label: "Search a site",
    kind: "url",
    hint: "your own search address, with the track dropped into it",
    slots: [
      {
        label: "Address",
        placeholder: "http://example.com/?search=",
        value: "http://example.com/?search=",
      },
    ],
    build: (_f, v) => `${v[0] ?? ""}%artist%+%title%`,
  },

  // ── Filing ────────────────────────────────────────────────────────────────
  {
    id: "copy.folder",
    label: "Copy into a folder",
    kind: "copy",
    hint: "leaves the original where it is",
    target: "",
    build: () => "",
  },
  {
    id: "move.folder",
    label: "Move into a folder",
    kind: "move",
    hint: "the file lands under the name it already has",
    target: "",
    build: () => "",
  },
  {
    id: "move.release",
    label: "Move into a release folder",
    kind: "move",
    hint: "Bicep - Isles (2021)\\Glue.wav — the folder is made if it is missing",
    target: "",
    build: () => `%artist% - %album% '('%year%')'\\%filename%`,
  },
];

/** Marks an action written by hand rather than from a recipe. */
export const CUSTOM_RECIPE = "custom";

export function recipesFor(kind: ActionKind): Recipe[] {
  return RECIPES.filter((r) => r.kind === kind);
}

export function recipeById(id: string): Recipe | undefined {
  return RECIPES.find((r) => r.id === id);
}

/**
 * The tag fields offered to a recipe that writes one.
 *
 * The ones a DJ edits, not every field that exists — a list of forty would be
 * the same problem as an expression box. Anything else can still be typed into
 * the field box, which stays free text.
 */
export const TAG_FIELDS = [
  "title",
  "artist",
  "album",
  "albumartist",
  "genre",
  "date",
  "comment",
  "tracknumber",
  "label",
  "catalognumber",
];
