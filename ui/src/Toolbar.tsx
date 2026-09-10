import { For } from "solid-js";
import {
  BroomIcon,
  CopiesIcon,
  GlobeIcon,
  ImageIcon,
  RenameIcon,
  SpectrumIcon,
  StarIcon,
  TagIcon,
} from "./icons";

/** A thing the toolbar can do to the selection. */
export type ActionId =
  | "tags"
  | "match"
  | "art"
  | "rename"
  | "clean"
  | "quality"
  | "copies"
  | "quick";

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
  icon: () => import("solid-js").JSX.Element;
}[] = [
  { id: "tags", label: "Edit metadata", icon: TagIcon },
  { id: "match", label: "Find metadata", icon: GlobeIcon },
  { id: "art", label: "Find album art", icon: ImageIcon },
  { id: "rename", label: "Rename", icon: RenameIcon },
  { id: "clean", label: "Clean up", icon: BroomIcon },
  { id: "quality", label: "Check quality", icon: SpectrumIcon },
  { id: "copies", label: "Find duplicates", icon: CopiesIcon },
  { id: "quick", label: "Quick tag", icon: StarIcon },
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
              //
              // The name and nothing else. What the button would do to how many
              // files is answered by the dialog it opens, which lists them.
              data-tip={button.label}
              aria-label={button.label}
              // Marked unavailable rather than `disabled`, because a disabled
              // button receives no mouse events in this engine and would lose
              // its tooltip along with them.
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
