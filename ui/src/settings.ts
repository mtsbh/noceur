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
export type Settings = {
  wave: { shape: WaveShape; colour: WaveColour };
};

export const DEFAULTS: Settings = {
  wave: { shape: "filled", colour: "rgb" },
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
    };
  } catch {
    return DEFAULTS;
  }
}

export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(settings));
  } catch {
    // A full or disabled store costs the preference, not the session.
  }
}
