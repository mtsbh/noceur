import { For, Show } from "solid-js";
import {
  BroomIcon,
  CopiesIcon,
  GlobeIcon,
  ImageIcon,
  RenameIcon,
  SpectrumIcon,
  StarIcon,
  TagIcon,
  ConvertIcon,
  NameToTagIcon,
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
  | "quick"
  | "convert"
  | "fromname";

type Button = {
  id: ActionId;
  label: string;
  icon: () => import("solid-js").JSX.Element;
};

/**
 * The buttons, in two groups: what a file *says* (its tags, art and rating),
 * and what is done *to* the file (its name, its junk, its audio, its copies).
 *
 * Fixed — which glyph reads as "find metadata" is arguable, but arguing about
 * it in a settings panel cost more interface than the whole toolbar is worth.
 * Every one of these is in the right-click menu as well, so the bar is a
 * shorter route rather than the only one.
 */
const GROUPS: { label: string; buttons: Button[] }[] = [
  {
    label: "Metadata",
    buttons: [
      { id: "tags", label: "Edit metadata", icon: TagIcon },
      { id: "match", label: "Find metadata", icon: GlobeIcon },
      { id: "fromname", label: "Tags from file name", icon: NameToTagIcon },
      { id: "art", label: "Find album art", icon: ImageIcon },
      { id: "quick", label: "Quick tag", icon: StarIcon },
    ],
  },
  {
    label: "Files",
    buttons: [
      { id: "rename", label: "Rename", icon: RenameIcon },
      { id: "clean", label: "Clean up", icon: BroomIcon },
      { id: "quality", label: "Check quality", icon: SpectrumIcon },
      { id: "copies", label: "Find duplicates", icon: CopiesIcon },
      { id: "convert", label: "Convert", icon: ConvertIcon },
    ],
  },
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
  /** User-defined buttons that were given an icon, after the built-in ones. */
  custom?: { id: string; label: string; icon: () => import("solid-js").JSX.Element; run: () => void }[];
}) {
  /** Quick tag works on whatever is playing, so it never needs a selection. */
  const needsSelection = (id: ActionId) => id !== "quick";

  return (
    <div class="toolbar">
      <For each={GROUPS}>
        {(group) => (
          <div class="tgroup" role="group" aria-label={group.label}>
            <For each={group.buttons}>
              {(button) => {
                const off = () => needsSelection(button.id) && props.count === 0;
                return (
                  <button
                    class={`toolbtn ${off() ? "off" : ""} ${
                      button.id === "quick" && props.quickOpen ? "on" : ""
                    }`}
                    // `data-tip` rather than `title`: the browser's own tooltip
                    // takes about a second to appear, is drawn by the OS in the
                    // OS's colours, and would sit under a window that draws its
                    // own title bar. This one is the app's.
                    data-tip={button.label}
                    aria-label={button.label}
                    // Marked unavailable rather than `disabled`, because a
                    // disabled button receives no mouse events in this engine
                    // and would lose its tooltip along with them.
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
        )}
      </For>
      <Show when={props.custom?.length}>
        <div class="tgroup" role="group" aria-label="Your buttons">
          <For each={props.custom}>
            {(button) => {
              const off = () => props.count === 0;
              return (
                <button
                  class={`toolbtn ${off() ? "off" : ""}`}
                  data-tip={button.label}
                  aria-label={button.label}
                  aria-disabled={off()}
                  onClick={() => {
                    if (off()) return;
                    button.run();
                  }}
                >
                  <button.icon />
                </button>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
}
