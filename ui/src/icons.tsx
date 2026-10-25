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

/** Points left; turned by CSS to point the other way. */
export function ChevronIcon() {
  return (
    <Icon>
      <path d="M10 3.5 5.5 8l4.5 4.5" />
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
/** Detect BPM & key: a metronome, its arm off to one side. */
export function TempoIcon() {
  return (
    <Icon>
      <path d="M6.2 2.2h3.6l2.8 11.4H3.4z" />
      <path d="M8 10.6l3.4-6.2M4.3 10.6h7.4" />
    </Icon>
  );
}

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

/** Duplicates: two overlapping squares, the ordinary sign for a copy. */
export function CopiesIcon() {
  return (
    <Icon>
      <rect x="2.4" y="2.4" width="8.2" height="8.2" rx="1.1" />
      <path d="M5.4 13.6h7.1a1.1 1.1 0 0 0 1.1-1.1V5.4" />
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

/**
 * The split: one rectangle divided, drawn the way the panes actually sit.
 *
 * Two icons rather than one that rotates, because the button both toggles the
 * split and says which way it would go — a single glyph would have to mean
 * "split" and "the other orientation" at once.
 */
export function SplitColsIcon() {
  return (
    <Icon>
      <rect x="2" y="3" width="12" height="10" rx="1.1" />
      <path d="M8 3v10" />
    </Icon>
  );
}

export function SplitRowsIcon() {
  return (
    <Icon>
      <rect x="2" y="3" width="12" height="10" rx="1.1" />
      <path d="M2 8h12" />
    </Icon>
  );
}

/** The volume glyph with the waves struck out, for a muted transport. */
export function MutedIcon() {
  return (
    <Icon>
      <path {...solid} d="M7.2 3.1 4.4 5.4H2.6a.6.6 0 0 0-.6.6v4a.6.6 0 0 0 .6.6h1.8l2.8 2.3a.6.6 0 0 0 .98-.47V3.57a.6.6 0 0 0-.98-.47Z" />
      <path d="m10.4 6.2 3.2 3.6M13.6 6.2l-3.2 3.6" />
    </Icon>
  );
}

/* ── Icons for user-defined buttons ─────────────────────────────────────────
   A small fixed set rather than anything free-form, drawn on the same grid as
   the toolbar's own so a custom button sits beside them without looking
   borrowed. */

export function DriveIcon() {
  return (
    <Icon>
      <rect x="2.2" y="4.2" width="11.6" height="7.6" rx="1.3" />
      <path d="M2.4 9h11.2" />
      <circle cx="11.2" cy="10.4" r=".5" fill="currentColor" stroke="none" />
    </Icon>
  );
}

export function UsbIcon() {
  return (
    <Icon>
      <rect x="5" y="6.2" width="6" height="7.6" rx="1" />
      <path d="M6.2 6.2V2.4h3.6v3.8" />
      <path d="M7.2 4h.01M8.8 4h.01" />
    </Icon>
  );
}

export function FolderIcon() {
  return (
    <Icon>
      <path d="M2.4 4.4a1 1 0 0 1 1-1h2.9l1.3 1.4h4.9a1 1 0 0 1 1 1v6.4a1 1 0 0 1-1 1H3.4a1 1 0 0 1-1-1Z" />
    </Icon>
  );
}

export function MoveIcon() {
  return (
    <Icon>
      <path d="M2.6 8h9.6" />
      <path d="m9.2 4.8 3.2 3.2-3.2 3.2" />
      <path d="M13.6 3v10" />
    </Icon>
  );
}

export function OpenIcon() {
  return (
    <Icon>
      <path d="M12.6 9v3.4a1 1 0 0 1-1 1H3.6a1 1 0 0 1-1-1V4.4a1 1 0 0 1 1-1H7" />
      <path d="M9.6 2.6h3.8v3.8M13.2 2.8 7.6 8.4" />
    </Icon>
  );
}

export function SearchIcon() {
  return (
    <Icon>
      <circle cx="7" cy="7" r="4.2" />
      <path d="m10.1 10.1 3.4 3.4" />
    </Icon>
  );
}

export function NoteIcon() {
  return (
    <Icon>
      <path d="M6 11.6V3.4l6.4-1.2v8" />
      <circle cx="4.4" cy="11.6" r="1.6" />
      <circle cx="10.8" cy="10.2" r="1.6" />
    </Icon>
  );
}

export function HeartIcon() {
  return (
    <Icon>
      <path d="M8 13.2S2.4 10 2.4 6.2a2.9 2.9 0 0 1 5.6-1.1 2.9 2.9 0 0 1 5.6 1.1C13.6 10 8 13.2 8 13.2Z" />
    </Icon>
  );
}

export function FlagIcon() {
  return (
    <Icon>
      <path d="M3.4 14V2.6" />
      <path d="M3.4 3h8.4l-1.8 2.9 1.8 2.9H3.4" />
    </Icon>
  );
}

export function HeadphonesIcon() {
  return (
    <Icon>
      <path d="M2.8 10.4V8a5.2 5.2 0 0 1 10.4 0v2.4" />
      <rect x="2.4" y="9.4" width="2.6" height="4" rx=".8" />
      <rect x="11" y="9.4" width="2.6" height="4" rx=".8" />
    </Icon>
  );
}

/** Identify by sound: a speaker's waves, heard. */
export function ListenIcon() {
  return (
    <Icon>
      <circle cx="4" cy="8" r="1.3" />
      <path d="M7 5.2a4 4 0 0 1 0 5.6" />
      <path d="M9.4 3.2a7 7 0 0 1 0 9.6" />
      <path d="M11.8 1.6a9.6 9.6 0 0 1 0 12.8" />
    </Icon>
  );
}

export function CrateIcon() {
  return (
    <Icon>
      <path d="M2.4 5.2h11.2v7.6a.8.8 0 0 1-.8.8H3.2a.8.8 0 0 1-.8-.8Z" />
      <path d="M5.4 5.2 6.4 2.4M10.6 5.2 9.6 2.4M5.2 8.6h5.6" />
    </Icon>
  );
}

/** What a user-defined button can wear, in the order the picker shows them. */
export const ACTION_ICONS: { name: string; label: string; icon: () => JSX.Element }[] = [
  { name: "bolt", label: "Bolt", icon: BoltIcon },
  { name: "drive", label: "Drive", icon: DriveIcon },
  { name: "usb", label: "USB stick", icon: UsbIcon },
  { name: "folder", label: "Folder", icon: FolderIcon },
  { name: "move", label: "Move", icon: MoveIcon },
  { name: "copy", label: "Copy", icon: CopiesIcon },
  { name: "open", label: "Open", icon: OpenIcon },
  { name: "search", label: "Search", icon: SearchIcon },
  { name: "globe", label: "Web", icon: GlobeIcon },
  { name: "tag", label: "Tag", icon: TagIcon },
  { name: "rename", label: "Rename", icon: RenameIcon },
  { name: "note", label: "Music", icon: NoteIcon },
  { name: "headphones", label: "Headphones", icon: HeadphonesIcon },
  { name: "crate", label: "Crate", icon: CrateIcon },
  { name: "star", label: "Star", icon: StarIcon },
  { name: "heart", label: "Heart", icon: HeartIcon },
  { name: "flag", label: "Flag", icon: FlagIcon },
];

/** The icon a button was given, or `undefined` for none or one this build lacks. */
export function actionIcon(name?: string) {
  return name ? ACTION_ICONS.find((i) => i.name === name)?.icon : undefined;
}

/* ── Menu and converter icons ─────────────────────────────────────────────── */

/** Two arrows chasing each other: one format into another. */
export function ConvertIcon() {
  return (
    <Icon>
      <path d="M2.8 6.2a5.2 5.2 0 0 1 9.4-2" />
      <path d="M12.6 2.2v2.4h-2.4" />
      <path d="M13.2 9.8a5.2 5.2 0 0 1-9.4 2" />
      <path d="M3.4 13.8v-2.4h2.4" />
    </Icon>
  );
}

export function ClipboardIcon() {
  return (
    <Icon>
      <rect x="3.4" y="3.2" width="9.2" height="10.8" rx="1.1" />
      <path d="M6 3.2V2.6a.6.6 0 0 1 .6-.6h2.8a.6.6 0 0 1 .6.6v.6" />
      <path d="M5.8 7.4h4.4M5.8 10h3" />
    </Icon>
  );
}

export function TrashIcon() {
  return (
    <Icon>
      <path d="M2.8 4.4h10.4M6.2 4.4V3a.6.6 0 0 1 .6-.6h2.4a.6.6 0 0 1 .6.6v1.4" />
      <path d="m4 4.4.7 8.6a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-8.6" />
      <path d="M6.6 7v4.2M9.4 7v4.2" />
    </Icon>
  );
}

export function WindowsIcon() {
  return (
    <Icon>
      <rect x="2.6" y="2.6" width="4.8" height="4.8" rx=".5" />
      <rect x="8.6" y="2.6" width="4.8" height="4.8" rx=".5" />
      <rect x="2.6" y="8.6" width="4.8" height="4.8" rx=".5" />
      <rect x="8.6" y="8.6" width="4.8" height="4.8" rx=".5" />
    </Icon>
  );
}

export function VinylIcon() {
  return (
    <Icon>
      <circle cx="8" cy="8" r="5.8" />
      <circle cx="8" cy="8" r="1.8" />
      <path d="M4.6 5.6a4 4 0 0 1 2-1.4" />
    </Icon>
  );
}

export function VideoIcon() {
  return (
    <Icon>
      <rect x="1.8" y="3.6" width="12.4" height="8.8" rx="2" />
      <path {...solid} d="M6.8 6.1v3.8L10 8Z" />
    </Icon>
  );
}

export function TabIcon() {
  return (
    <Icon>
      <path d="M2.4 13V4.2a.8.8 0 0 1 .8-.8h3.4l1 1.6h5.2a.8.8 0 0 1 .8.8V13a.8.8 0 0 1-.8.8H3.2a.8.8 0 0 1-.8-.8Z" />
      <path d="M8 7.6v4M6 9.6h4" />
    </Icon>
  );
}

export function NewFolderIcon() {
  return (
    <Icon>
      <path d="M2.4 4.4a1 1 0 0 1 1-1h2.9l1.3 1.4h4.9a1 1 0 0 1 1 1v6.4a1 1 0 0 1-1 1H3.4a1 1 0 0 1-1-1Z" />
      <path d="M8 7.4v3.6M6.2 9.2h3.6" />
    </Icon>
  );
}

/** A name turning into a tag: the file-name-to-tags dialog. */
export function NameToTagIcon() {
  return (
    <Icon>
      <path d="M2.4 4h6M2.4 6.8h4" />
      <path d="m7 10.4 1.6 1.6L7 13.6" />
      <path d="M2.4 12h6" />
      <path d="M9.6 6.6V3.8a.5.5 0 0 1 .5-.5h2.8l1.6 1.6v2.8l-2.4 2.4a.5.5 0 0 1-.7 0l-1.6-1.6a.5.5 0 0 1-.2-.3Z" />
    </Icon>
  );
}

/** A keyboard: the shortcuts overview. */
export function KeyboardIcon() {
  return (
    <Icon>
      <rect x="1.6" y="4" width="12.8" height="8" rx="1.2" />
      <path d="M4 6.6h.01M6.4 6.6h.01M8.8 6.6h.01M11.2 6.6h.01M5.2 9.4h5.6" />
    </Icon>
  );
}

/** The command key's loop-square: the command palette. */
export function CommandIcon() {
  return (
    <Icon>
      <path d="M6 6h4v4H6z" />
      <path d="M6 6V4.6A1.6 1.6 0 1 0 4.6 6H6M10 6V4.6A1.6 1.6 0 1 1 11.4 6H10M6 10v1.4A1.6 1.6 0 1 1 4.6 10H6M10 10v1.4a1.6 1.6 0 1 0 1.4-1.4H10" />
    </Icon>
  );
}

/** Three bars of a chart: the library overview. */
export function ChartIcon() {
  return (
    <Icon>
      <path d="M2.4 13.6h11.2" />
      <path d="M4.2 13.6V8.4M8 13.6V3.6M11.8 13.6V6.4" />
    </Icon>
  );
}

/** A paper plane, for sending to Telegram. */
export function SendIcon() {
  return (
    <Icon>
      <path d="M13.8 2.2 2.2 7l4.6 1.8L8.6 13.4z" />
      <path d="M13.8 2.2 6.8 8.8" />
    </Icon>
  );
}

/** Three lines of a list and a note: playlists. */
export function PlaylistIcon() {
  return (
    <Icon>
      <path d="M2.4 4h7.2M2.4 7.4h7.2M2.4 10.8h4.4" />
      <path d="M12.2 12.2V5.4l2 .8" />
      <circle cx="10.9" cy="12.2" r="1.3" />
    </Icon>
  );
}

/**
 * Every glyph a toolbar button can be given when the bar is customised.
 *
 * The button icons first, then the ones the built-in tools wear, so a user
 * button can borrow a tool's look and a tool can take a plainer one.
 */
export const BAR_ICONS: { name: string; label: string; icon: () => JSX.Element }[] = [
  ...ACTION_ICONS,
  { name: "image", label: "Image", icon: ImageIcon },
  { name: "nametotag", label: "Name to tag", icon: NameToTagIcon },
  { name: "broom", label: "Broom", icon: BroomIcon },
  { name: "spectrum", label: "Spectrum", icon: SpectrumIcon },
  { name: "convert", label: "Convert", icon: ConvertIcon },
  { name: "pig", label: "Pig", icon: PigIcon },
  { name: "splitcols", label: "Columns", icon: SplitColsIcon },
  { name: "splitrows", label: "Rows", icon: SplitRowsIcon },
  { name: "vinyl", label: "Vinyl", icon: VinylIcon },
  { name: "clipboard", label: "Clipboard", icon: ClipboardIcon },
  { name: "trash", label: "Trash", icon: TrashIcon },
  { name: "newfolder", label: "New folder", icon: NewFolderIcon },
  { name: "tab", label: "Tab", icon: TabIcon },
  { name: "video", label: "Video", icon: VideoIcon },
  { name: "keyboard", label: "Keyboard", icon: KeyboardIcon },
  { name: "chart", label: "Chart", icon: ChartIcon },
  { name: "playlist", label: "Playlist", icon: PlaylistIcon },
];

export function barIcon(name?: string) {
  return name ? BAR_ICONS.find((i) => i.name === name)?.icon : undefined;
}

/** A window with an arrow leaving it: open in another program. */
export function OpenWithIcon() {
  return (
    <Icon>
      <rect x="2.4" y="4.4" width="9.2" height="9.2" rx="1.2" />
      <path d="M8.4 2.4h5.2v5.2" />
      <path d="m13.6 2.4-6 6" />
    </Icon>
  );
}

/** A set's tracklist: a waveform cut into tracks, a mark where each begins. */
export function TracklistIcon() {
  return (
    <Icon>
      <circle cx="5" cy="8" r="3.4" />
      <circle cx="5" cy="8" r="0.7" />
      <path d="M10.4 4.8h3.6M10.4 8h3.6M10.4 11.2h2.4" />
    </Icon>
  );
}
