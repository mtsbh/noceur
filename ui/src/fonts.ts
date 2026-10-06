/**
 * The faces the interface can be set in.
 *
 * Everything here except Inter ships with Windows, so the list is not a list of
 * things to go and install — it is a list of what is already there. Inter is
 * kept because it is the face this theme was drawn against and plenty of people
 * have it; if it is missing the picker says so rather than silently showing
 * Segoe UI under Inter's name.
 *
 * No bundled font file. One woff2 is a few hundred kilobytes inside a binary
 * that is meant to be copied onto a USB stick, and it would buy a face the
 * machine can very likely already draw.
 */
export type FontOption = {
  /**
   * What settings store. Stable for the life of the preference — renaming one
   * would silently reset the choice of everybody who had picked it.
   */
  name: string;
  label: string;
  /**
   * The full CSS stack, fallbacks included, so a face that turns out not to be
   * installed lands on something sane rather than on the browser's default
   * serif.
   */
  stack: string;
};

/**
 * What every stack ends with.
 *
 * `system-ui` before the generic, so a machine that is not Windows gets its own
 * interface font rather than Arial.
 */
const FALLBACK = `"Segoe UI", system-ui, sans-serif`;

/**
 * The choices, in the order they are shown.
 *
 * "System" leads with Segoe UI Variable Text, which is Windows 11's own
 * interface face and is hinted for exactly the 11–12px this list is drawn at —
 * visibly cleaner than plain Segoe UI at that size, and identical to it on
 * Windows 10, where it does not exist and the stack falls through.
 */
export const UI_FONTS: FontOption[] = [
  { name: "system", label: "System", stack: `"Segoe UI Variable Text", ${FALLBACK}` },
  { name: "segoe", label: "Segoe UI", stack: `"Segoe UI", ${FALLBACK}` },
  { name: "inter", label: "Inter", stack: `Inter, ${FALLBACK}` },
  // Windows' own DIN: narrow and engineered, it suits a dense track list.
  { name: "bahnschrift", label: "Bahnschrift", stack: `Bahnschrift, ${FALLBACK}` },
  // Office's face since 2023; on most machines with Microsoft 365.
  { name: "aptos", label: "Aptos", stack: `Aptos, ${FALLBACK}` },
  { name: "tahoma", label: "Tahoma", stack: `Tahoma, ${FALLBACK}` },
  { name: "verdana", label: "Verdana", stack: `Verdana, ${FALLBACK}` },
  { name: "calibri", label: "Calibri", stack: `Calibri, ${FALLBACK}` },
  { name: "cascadia", label: "Cascadia", stack: `"Cascadia Mono", Consolas, monospace` },
  { name: "consolas", label: "Consolas", stack: `Consolas, ui-monospace, monospace` },
];

/** The option name meaning "whatever family is typed in the box". */
export const CUSTOM = "custom";

/** What the interface uses when nothing has been chosen. */
export const DEFAULT_FONT = UI_FONTS[0];

/**
 * The first family named in a stack, for probing and for previewing.
 *
 * A stack cannot be probed: it always resolves to something, which is the
 * point of it.
 */
export function head(stack: string): string {
  return (stack.split(",")[0] ?? "").trim().replace(/^["']|["']$/g, "");
}

/**
 * Strips what must not reach a CSS declaration.
 *
 * This value is typed by hand and then written into a custom property, so it is
 * the one string in the interface that becomes CSS rather than being drawn as
 * text. Quotes, semicolons and braces would end the declaration early and start
 * another one; the length cap is there because a font name is a font name.
 */
export function cleanFamily(text: string): string {
  return text
    .replace(/["';{}()<>\\]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 64);
}

/** The CSS stack for a stored choice. */
export function stackFor(name: string, custom: string): string {
  if (name === CUSTOM) {
    const family = cleanFamily(custom);
    return family ? `"${family}", ${FALLBACK}` : DEFAULT_FONT.stack;
  }
  return (UI_FONTS.find((f) => f.name === name) ?? DEFAULT_FONT).stack;
}

/**
 * A string that renders at a different width in almost any two faces.
 *
 * Round letters, narrow letters, wide letters and digits, because two faces
 * that happen to agree on the width of one of those rarely agree on all four.
 */
const PROBE = "mmmwwwiiilll0123456789MW@";

/**
 * Bases to measure against.
 *
 * Three, because the test is "did the browser use my family or fall through to
 * the base", and a family that happens to have the same metrics as one generic
 * — a monospace face measured against `monospace` — would look like a miss.
 * Disagreeing with any base is enough to prove the family was used.
 */
const BASES = ["monospace", "sans-serif", "serif"];

/**
 * Whether a family is actually on this machine.
 *
 * Measured rather than asked: the API that would answer this directly
 * (`queryLocalFonts`) needs a permission prompt, and a picker that opens a
 * permission dialog before it will show you a list of fonts is worse than one
 * that quietly works it out.
 *
 * Errs towards saying yes. A false "missing" label on a font that is right
 * there would be actively misleading; the cost of a false "installed" is that
 * one entry in a list of eight draws in the fallback, which is what would have
 * happened anyway.
 */
export function installed(family: string): boolean {
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) return true;

  return BASES.some((base) => {
    ctx.font = `32px ${base}`;
    const control = ctx.measureText(PROBE).width;
    ctx.font = `32px "${family}", ${base}`;
    return Math.abs(ctx.measureText(PROBE).width - control) > 0.5;
  });
}
