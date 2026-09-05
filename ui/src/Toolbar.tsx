import { For } from "solid-js";
import {
  BroomIcon,
  GlobeIcon,
  ImageIcon,
  RenameIcon,
  StarIcon,
  TagIcon,
} from "./icons";

/** A thing the toolbar can do to the selection. */
export type ActionId = "tags" | "match" | "art" | "rename" | "clean" | "quick";

/**
 * The buttons, in order.
 *
 * Fixed — which glyph reads as "find metadata" is arguable, but arguing about
 * it in a settings panel cost more interface than the whole toolbar is worth.
 * Every one of these is in the right-click menu as well, so the bar is a
 * shorter route rather than the only one.
 */
const BUTTONS: {
  id: ActionId;
  label: string;
  hint: string;
  icon: () => import("solid-js").JSX.Element;
}[] = [
  { id: "tags", label: "Edit metadata", hint: "Edit the tags of the selection", icon: TagIcon },
  { id: "match", label: "Find metadata", hint: "Look the track up online", icon: GlobeIcon },
  { id: "art", label: "Find album art", hint: "Search for cover art", icon: ImageIcon },
  { id: "rename", label: "Rename", hint: "Rename from a template", icon: RenameIcon },
  { id: "clean", label: "Clean up", hint: "Fix names and tags with rules", icon: BroomIcon },
  { id: "quick", label: "Quick tag", hint: "Tag the playing track by keyboard", icon: StarIcon },
];

/**
 * Shortcut buttons for the things the right-click menu does.
 *
 * They act on the selection, so they are dead without one. Disabled rather than
 * hidden: a bar whose buttons come and go as rows are clicked is harder to aim
 * at than one whose buttons are always in the same place.
 */
export default function Toolbar(props: {
  /** How many audio files are selected. */
  count: number;
  /** Whether the quick-tag panel is open, so its button can show it. */
  quickOpen: boolean;
  onRun: (id: ActionId) => void;
}) {
  /** Quick tag works on whatever is playing, so it never needs a selection. */
  const needsSelection = (id: ActionId) => id !== "quick";

  const title = (button: (typeof BUTTONS)[number]) => {
    if (!needsSelection(button.id)) return `${button.label} — ${button.hint}`;
    if (props.count === 0) return `${button.label} — select a track first`;
    if (props.count === 1) return `${button.label} — ${button.hint}`;
    return `${button.label} — ${props.count} tracks`;
  };

  return (
    <div class="toolbar" role="group" aria-label="Track actions">
      <For each={BUTTONS}>
        {(button) => (
          <button
            class={`toolbtn ${button.id === "quick" && props.quickOpen ? "on" : ""}`}
            title={title(button)}
            aria-label={button.label}
            disabled={needsSelection(button.id) && props.count === 0}
            onClick={() => props.onRun(button.id)}
          >
            <button.icon />
          </button>
        )}
      </For>
    </div>
  );
}
