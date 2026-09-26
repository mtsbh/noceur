import { createSignal } from "solid-js";

/**
 * The themes on offer. Each is a block of token overrides in `theme.css`,
 * keyed by `data-theme` on the root element; `swatch` is only what the picker
 * draws - background, panel, text, accent - so the choice can be made by eye.
 */
export const THEMES = [
  { name: "sodium", label: "Sodium", swatch: ["#0a0a0c", "#131317", "#eeeae4", "#e8a55f"] },
  { name: "cobalt", label: "Cobalt", swatch: ["#080b12", "#111725", "#e2e8f4", "#62a6ff"] },
  { name: "frost", label: "Frost", swatch: ["#1c2029", "#272d39", "#e5e9f0", "#88c0d0"] },
  { name: "velvet", label: "Velvet", swatch: ["#0f0a0e", "#1c1219", "#f2e5ec", "#f07aa5"] },
  { name: "phosphor", label: "Phosphor", swatch: ["#040705", "#0b120d", "#c4f2ce", "#40f07c"] },
  { name: "graphite", label: "Graphite", swatch: ["#141414", "#202020", "#ececec", "#f0f0f0"] },
  { name: "paper", label: "Paper", swatch: ["#f3efe7", "#e5dfd3", "#221e18", "#b8541c"] },
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
