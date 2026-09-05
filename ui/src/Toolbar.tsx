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

  /**
   * What the tooltip says.
   *
   * The label, and then what it would actually do right now — how many tracks
   * it would touch, or why it is greyed out. A tooltip that only repeats the
   * label answers a question nobody with the icon in front of them is asking.
   */
  const tip = (button: (typeof BUTTONS)[number]) => {
    if (!needsSelection(button.id)) return `${button.label} — ${button.hint}`;
    if (props.count === 0) return `${button.label} — select a track first`;
    if (props.count === 1) return `${button.label} — ${button.hint}`;
    return `${button.label} — ${props.count} tracks`;
  };

  return (
    <div class="toolbar" role="group" aria-label="Track actions">
      <For each={BUTTONS}>
        {(button) => {
          const off = () => needsSelection(button.id) && props.count === 0;
          return (
            <button
              class={`toolbtn ${off() ? "off" : ""} ${
                button.id === "quick" && props.quickOpen ? "on" : ""
              }`}
              // `data-tip` rather than `title`: the browser's own tooltip takes
              // about a second to appear, is drawn by the OS in the OS's
              // colours, and would sit under a window that draws its own title
              // bar. This one is the app's.
              data-tip={tip(button)}
              aria-label={tip(button)}
              // Marked unavailable rather than `disabled`, because a disabled
              // button receives no mouse events in this engine — so it would
              // lose its tooltip at exactly the moment the tooltip is the whole
              // point, when it is greyed out and the reason is not obvious.
              aria-disabled={off()}
              onClick={() => {
                if (off()) return;
                props.onRun(button.id);
              }}
            >
              <button.icon />
            </button>
          );
        }}
      </For>
    </div>
  );
}
