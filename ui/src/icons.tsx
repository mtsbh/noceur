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

/** The track actions button. These are all ways of changing a track's tags. */
export function TagIcon() {
  return (
    <Icon>
      <path d="M2.6 7.4V3.2a.6.6 0 0 1 .6-.6h4.2a.6.6 0 0 1 .42.18l5.4 5.4a.6.6 0 0 1 0 .85l-4.2 4.2a.6.6 0 0 1-.85 0l-5.4-5.4a.6.6 0 0 1-.17-.43Z" />
      <circle cx="5.4" cy="5.4" r=".9" />
    </Icon>
  );
}

/** The little triangle that says a button opens a menu. */
export function CaretIcon() {
  return (
    <svg viewBox="0 0 16 16" width="8" height="8" aria-hidden="true">
      <path fill="currentColor" d="M3 6h10l-5 5.5Z" />
    </svg>
  );
}
