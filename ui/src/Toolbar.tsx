import { For, Show } from "solid-js";
import { TOOLBAR_ICONS } from "./icons";
import { ACTIONS, type ActionId, type ToolbarButton } from "./settings";

/**
 * Shortcut buttons for the things the right-click menu does.
 *
 * Everything here is already in that menu and stays there — this is a second
 * way to reach it, not the only one, which is what makes it safe to let someone
 * remove a button they never use. Which buttons appear, in what order and with
 * which icon, is a setting.
 *
 * They act on the selection, so they are dead without one. Disabled rather than
 * hidden: a bar whose buttons come and go as rows are clicked is harder to aim
 * at than one whose buttons are always in the same place.
 */
export default function Toolbar(props: {
  buttons: ToolbarButton[];
  /** How many audio files are selected. */
  count: number;
  /** Whether the quick-tag panel is open, so its button can show it. */
  quickOpen: boolean;
  onRun: (id: ActionId) => void;
}) {
  /** Quick tag works on whatever is playing, so it never needs a selection. */
  const needsSelection = (id: ActionId) => id !== "quick";

  const title = (id: ActionId) => {
    const { label, hint } = ACTIONS[id];
    if (!needsSelection(id)) return `${label} — ${hint}`;
    if (props.count === 0) return `${label} — select a track first`;
    if (props.count === 1) return `${label} — ${hint}`;
    return `${label} — ${props.count} tracks`;
  };

  return (
    <Show when={props.buttons.some((b) => b.on)}>
      <div class="toolbar" role="group" aria-label="Track actions">
        <For each={props.buttons.filter((b) => b.on)}>
          {(button) => {
            const Icon = TOOLBAR_ICONS[button.icon];
            return (
              <button
                class={`toolbtn ${
                  button.id === "quick" && props.quickOpen ? "on" : ""
                }`}
                title={title(button.id)}
                aria-label={ACTIONS[button.id].label}
                disabled={needsSelection(button.id) && props.count === 0}
                onClick={() => props.onRun(button.id)}
              >
                <Icon />
              </button>
            );
          }}
        </For>
      </div>
    </Show>
  );
}
