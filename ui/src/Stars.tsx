import { For, Show } from "solid-js";
import type { RowDisplay } from "./api";

/**
 * A row's rating, as five stars that set it.
 *
 * Drawn in reverse and laid out right to left, so CSS alone can light a star
 * and every star before it under the pointer: the stars "before" one on screen
 * are its later siblings in the markup, which `~` can reach.
 *
 * Clicks and double-clicks stop here, so rating a track neither changes the
 * selection nor starts it playing.
 */
export default function Stars(props: { row: RowDisplay; onRate: (stars: number) => void }) {
  return (
    <Show when={props.row.kind === 1} fallback={<span />}>
      <span
        class={`stars ${props.row.rating ? "rated" : ""}`}
        title={props.row.rating ? `${props.row.rating} of 5 - click the same star to clear` : "Rate"}
        onDblClick={(e) => e.stopPropagation()}
        onPointerDown={(e) => e.stopPropagation()}
      >
        <For each={[5, 4, 3, 2, 1]}>
          {(n) => (
            <span
              class={`star ${n <= props.row.rating ? "on" : ""}`}
              onClick={(e) => {
                e.stopPropagation();
                props.onRate(n);
              }}
            >
              ★
            </span>
          )}
        </For>
      </span>
    </Show>
  );
}
