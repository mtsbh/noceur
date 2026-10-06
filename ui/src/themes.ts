import { createSignal } from "solid-js";

/**
 * The themes on offer. Each is a block of token overrides in `theme.css`,
 * keyed by `data-theme` on the root element; `swatch` is only what the picker
 * draws - background, panel, text, accent - so the choice can be made by eye.
 */
export const THEMES = [
  { name: "sodium", label: "Sodium", note: "amber", swatch: ["#0a0a0c", "#131317", "#eeeae4", "#e8a55f"] },
  { name: "obsidian", label: "Obsidian", note: "champagne", swatch: ["#070708", "#121214", "#ece8e0", "#d4b483"] },
  { name: "ember", label: "Ember", note: "vermilion", swatch: ["#0d0a09", "#1a1411", "#f0e6de", "#ff6a3d"] },
  { name: "velvet", label: "Velvet", note: "rose", swatch: ["#0f0a0e", "#1c1219", "#f2e5ec", "#f07aa5"] },
  { name: "iris", label: "Iris", note: "violet", swatch: ["#0b0a12", "#161423", "#e8e6f6", "#9d8cff"] },
  { name: "cobalt", label: "Cobalt", note: "blue", swatch: ["#080b12", "#111725", "#e2e8f4", "#62a6ff"] },
  { name: "abyss", label: "Abyss", note: "aqua", swatch: ["#050b0c", "#0d181b", "#dcecec", "#46d6c4"] },
  { name: "frost", label: "Frost", note: "nord", swatch: ["#1c2029", "#272d39", "#e5e9f0", "#88c0d0"] },
  { name: "graphite", label: "Graphite", note: "mono", swatch: ["#141414", "#202020", "#ececec", "#f0f0f0"] },
  { name: "paper", label: "Paper", note: "light", swatch: ["#f3efe7", "#e5dfd3", "#221e18", "#b8541c"] },
  { name: "porcelain", label: "Porcelain", note: "light", swatch: ["#f6f7f9", "#e4e7ed", "#141821", "#3b5bdb"] },
] as const;

export type ThemeName = (typeof THEMES)[number]["name"];

export const DEFAULT_THEME: ThemeName = "sodium";

export function isTheme(name: unknown): name is ThemeName {
  return THEMES.some((t) => t.name === name);
}

/**
 * Bumped whenever the theme changes.
 *
 * CSS repaints itself; canvases do not. Anything that draws with theme colours
 * reads this inside its draw so a change of theme redraws it at once rather
 * than on the next change of data.
 */
const [themeTick, setThemeTick] = createSignal(0);
export { themeTick };

/** Puts a theme on the root element and tells the canvases. */
export function applyTheme(name: ThemeName) {
  const root = document.documentElement;
  if (name === DEFAULT_THEME) delete root.dataset.theme;
  else root.dataset.theme = name;
  setThemeTick((n) => n + 1);
}
