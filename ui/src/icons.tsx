import type { JSX } from "solid-js";

/**
 * Transport and toolbar icons, drawn rather than typed.
 *
 * These were text glyphs — `⏮ ⏪ ▶ ■` — which is why they never quite looked
 * right: the glyphs come from whichever font on the system happens to carry
 * them, so they arrive at different weights and different optical sizes, sit on
 * a text baseline rather than centred in their button, and cannot take the
 * theme's colours. As paths they are one weight, one grid, and inherit
 * `currentColor` like everything else.
 *
 * All are drawn in a 16-unit box so they line up without per-icon nudging.
 */
function Icon(props: { children: JSX.Element; label?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden={props.label ? undefined : "true"}
      aria-label={props.label}
    >
      {props.children}
    </svg>
  );
}

/** Filled triangles and bars read better than outlines at this size. */
const solid = { fill: "currentColor", stroke: "none" } as const;

export function PlayIcon() {
  return (
    <Icon>
      <path {...solid} d="M5 3.4v9.2a.6.6 0 0 0 .92.5l7.2-4.6a.6.6 0 0 0 0-1L5.92 2.9A.6.6 0 0 0 5 3.4Z" />
    </Icon>
  );
}

export function PauseIcon() {
  return (
    <Icon>
      <rect {...solid} x="4" y="3" width="3" height="10" rx="0.8" />
      <rect {...solid} x="9" y="3" width="3" height="10" rx="0.8" />
    </Icon>
  );
}

export function StopIcon() {
  return (
    <Icon>
      <rect {...solid} x="4" y="4" width="8" height="8" rx="1" />
    </Icon>
  );
}

export function PrevIcon() {
  return (
    <Icon>
      <rect {...solid} x="3" y="3.5" width="1.8" height="9" rx="0.7" />
      <path {...solid} d="M13 4.1v7.8a.6.6 0 0 1-.93.5L6.2 8.5a.6.6 0 0 1 0-1l5.87-3.9a.6.6 0 0 1 .93.5Z" />
    </Icon>
  );
}

export function NextIcon() {
  return (
    <Icon>
      <rect {...solid} x="11.2" y="3.5" width="1.8" height="9" rx="0.7" />
      <path {...solid} d="M3 4.1v7.8a.6.6 0 0 0 .93.5L9.8 8.5a.6.6 0 0 0 0-1L3.93 3.6A.6.6 0 0 0 3 4.1Z" />
    </Icon>
  );
}

export function BackIcon() {
  return (
    <Icon>
      <path {...solid} d="M8.5 4.4v7.2a.6.6 0 0 1-.93.5L2.4 8.6a.6.6 0 0 1 0-1l5.17-3.7a.6.6 0 0 1 .93.5Z" />
      <path {...solid} d="M14 4.4v7.2a.6.6 0 0 1-.93.5L7.9 8.6a.6.6 0 0 1 0-1l5.17-3.7a.6.6 0 0 1 .93.5Z" />
    </Icon>
  );
}

export function ForwardIcon() {
  return (
    <Icon>
      <path {...solid} d="M2 4.4v7.2a.6.6 0 0 0 .93.5L8.1 8.6a.6.6 0 0 0 0-1L2.93 3.9A.6.6 0 0 0 2 4.4Z" />
      <path {...solid} d="M7.5 4.4v7.2a.6.6 0 0 0 .93.5l5.17-3.5a.6.6 0 0 0 0-1L8.43 3.9a.6.6 0 0 0-.93.5Z" />
    </Icon>
  );
}

export function VolumeIcon() {
  return (
    <Icon>
      <path {...solid} d="M7.2 3.1 4.4 5.4H2.6a.6.6 0 0 0-.6.6v4a.6.6 0 0 0 .6.6h1.8l2.8 2.3a.6.6 0 0 0 .98-.47V3.57a.6.6 0 0 0-.98-.47Z" />
      <path d="M10.4 6.1a2.7 2.7 0 0 1 0 3.8" />
      <path d="M12.3 4.4a5.2 5.2 0 0 1 0 7.2" />
    </Icon>
  );
}

export function SettingsIcon() {
  return (
    <Icon>
      <circle cx="8" cy="8" r="2.1" />
      <path d="M12.9 9.8a1.1 1.1 0 0 0 .22 1.21l.04.04a1.33 1.33 0 1 1-1.88 1.88l-.04-.04a1.1 1.1 0 0 0-1.21-.22 1.1 1.1 0 0 0-.67 1v.11a1.33 1.33 0 1 1-2.66 0v-.06a1.1 1.1 0 0 0-.72-1 1.1 1.1 0 0 0-1.21.22l-.04.04a1.33 1.33 0 1 1-1.88-1.88l.04-.04a1.1 1.1 0 0 0 .22-1.21 1.1 1.1 0 0 0-1-.67h-.11a1.33 1.33 0 1 1 0-2.66h.06a1.1 1.1 0 0 0 1-.72 1.1 1.1 0 0 0-.22-1.21l-.04-.04a1.33 1.33 0 1 1 1.88-1.88l.04.04a1.1 1.1 0 0 0 1.21.22h.05a1.1 1.1 0 0 0 .67-1v-.11a1.33 1.33 0 1 1 2.66 0v.06a1.1 1.1 0 0 0 .67 1 1.1 1.1 0 0 0 1.21-.22l.04-.04a1.33 1.33 0 1 1 1.88 1.88l-.04.04a1.1 1.1 0 0 0-.22 1.21v.05a1.1 1.1 0 0 0 1 .67h.11a1.33 1.33 0 1 1 0 2.66h-.06a1.1 1.1 0 0 0-1 .67Z" />
    </Icon>
  );
}

export function CloseIcon() {
  return (
    <Icon>
      <path d="m4 4 8 8M12 4l-8 8" />
    </Icon>
  );
}

/* ── Track action icons ─────────────────────────────────────────────────────
   One per button in the path bar. Fixed rather than chosen: which glyph reads
   as "find metadata" is arguable, but arguing about it in a settings panel cost
   more interface than the whole toolbar was worth. */

export function TagIcon() {
  return (
    <Icon>
      <path d="M2.6 7.4V3.2a.6.6 0 0 1 .6-.6h4.2a.6.6 0 0 1 .42.18l5.4 5.4a.6.6 0 0 1 0 .85l-4.2 4.2a.6.6 0 0 1-.85 0l-5.4-5.4a.6.6 0 0 1-.17-.43Z" />
      <circle cx="5.4" cy="5.4" r=".9" />
    </Icon>
  );
}

export function GlobeIcon() {
  return (
    <Icon>
      <circle cx="8" cy="8" r="5.6" />
      <path d="M2.4 8h11.2" />
      <path d="M8 2.4a9 9 0 0 1 0 11.2 9 9 0 0 1 0-11.2Z" />
    </Icon>
  );
}

export function ImageIcon() {
  return (
    <Icon>
      <rect x="2.4" y="3.2" width="11.2" height="9.6" rx="1.2" />
      <circle cx="5.9" cy="6.4" r="1" />
      <path d="m3 11.4 3.1-3a.8.8 0 0 1 1.1 0l2.2 2.1 1.3-1.2a.8.8 0 0 1 1.1 0l1.2 1.1" />
    </Icon>
  );
}

export function RenameIcon() {
  return (
    <Icon>
      <path d="M8 3.4v9.2" />
      <path d="M5.6 3.4h4.8M5.6 12.6h4.8" />
      <path d="M2.4 5.6V4.2a.8.8 0 0 1 .8-.8h9.6a.8.8 0 0 1 .8.8v1.4" />
    </Icon>
  );
}

export function BroomIcon() {
  return (
    <Icon>
      <path d="M9.6 2.6 6.4 5.8" />
      <path d="M4.6 7.6a1.4 1.4 0 0 1 2 0l1.8 1.8a1.4 1.4 0 0 1 0 2l-.7.7-3.8-3.8Z" />
      <path d="m3.9 8.3-1.5 4.3a.6.6 0 0 0 .74.76l4.36-1.36" />
      <path d="M11 4.2a2 2 0 0 1 2.8 2.8" />
    </Icon>
  );
}

/** Bars falling away to the right: a spectrum that stops. */
export function SpectrumIcon() {
  return (
    <Icon>
      <path d="M2.6 12.4V8M5.3 12.4V4.4M8 12.4V6M10.7 12.4v-3.6M13.4 12.4v-1.4" />
    </Icon>
  );
}

/**
 * The spectrogram toggle: a pig, because Spek's is.
 *
 * Not the falling bars — those belong to the quality check, and two buttons in
 * the same bar wearing one glyph is worse than either being less literal. A face
 * is also the easier thing to pick out of a row of abstract marks, which is what
 * a toolbar mostly is.
 */
export function PigIcon() {
  return (
    <Icon>
      {/* Ears first, so the head's outline draws over their base. */}
      <path d="M5.2 4.3 3.6 2.7l.2 2.5M10.8 4.3l1.6-1.6-.2 2.5" />
      <path d="M8 3.7c2.85 0 5.1 1.9 5.1 4.45S10.85 12.9 8 12.9 2.9 10.7 2.9 8.15 5.15 3.7 8 3.7Z" />
      <ellipse cx="8" cy="9.2" rx="2" ry="1.45" />
      <circle cx="7.35" cy="9.2" r="0.42" fill="currentColor" stroke="none" />
      <circle cx="8.65" cy="9.2" r="0.42" fill="currentColor" stroke="none" />
      <circle cx="6" cy="6.6" r="0.5" fill="currentColor" stroke="none" />
      <circle cx="10" cy="6.6" r="0.5" fill="currentColor" stroke="none" />
    </Icon>
  );
}

/** User-defined buttons: a bolt, for "do the thing I set up". */
export function BoltIcon() {
  return (
    <Icon>
      <path d="M8.9 1.9 3.6 9.1h3.7l-.9 5 5.3-7.2H8l.9-5Z" />
    </Icon>
  );
}

export function StarIcon() {
  return (
    <Icon>
      <path d="m8 2.6 1.66 3.36 3.71.54-2.69 2.62.64 3.7L8 11.07l-3.32 1.75.64-3.7L2.63 6.5l3.71-.54Z" />
    </Icon>
  );
}
