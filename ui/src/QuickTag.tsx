import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { RATING_FIELD, quickFields, writeTags } from "./api";
import { QUICK_KEYS, colourOf, type QuickGroup } from "./settings";

/**
 * How long after the last keypress the changes are written.
 *
 * Writing a tag rewrites the file around it, which for a 150MB WAV is real work.
 * Tapping through energy, then mood, then a second mood would be three rewrites
 * of the same file in a second. Waiting a moment turns a burst into one write,
 * and the panel shows what is pending in the meantime — so the feedback is
 * immediate even though the disk is not.
 */
const FLUSH_MS = 900;

/**
 * Tagging by keyboard, while listening.
 *
 * The workflow this exists for is going through a folder with the player
 * running: hear a track, press a key for its energy, another for its mood, move
 * on. Everything about it is arranged so a hand can stay on the keyboard — the
 * bindings are the digits and the letter rows above them, and nothing here needs
 * the mouse.
 *
 * It tags the track that is *playing*, not the one selected, because the whole
 * point is judging something you are listening to.
 */
export default function QuickTag(props: {
  /** The playing track, or 0 when nothing is loaded. */
  fileId: number;
  groups: QuickGroup[];
  /** Whether to offer a star rating row. */
  rating: boolean;
  onClose: () => void;
  /** Called after a write lands, so the listing can re-read the row. */
  onWritten: () => void;
}) {
  /** What the track carries, as `field -> comma-joined values`. */
  const [current, setCurrent] = createSignal<Record<string, string>>({});
  const [pending, setPending] = createSignal(false);
  let timer: number | undefined;
  /** The file the pending edits belong to, so a track change flushes to it. */
  let owner = 0;

  const names = () => props.groups.map((g) => g.field).filter(Boolean);

  /** Stars currently on the track, 0 when none. */
  const stars = () => Number(current()[RATING_FIELD] ?? "0") || 0;

  /**
   * Sets the rating, or clears it when the star already given is pressed again.
   *
   * Which is how every star control anyone has used behaves, and the only way
   * to say "actually, no rating" without a separate control for it.
   */
  function rate(value: number) {
    if (props.fileId <= 0) return;
    const next = stars() === value ? 0 : value;
    setCurrent({ ...current(), [RATING_FIELD]: String(next) });
    schedule();
  }

  // Re-read whenever the playing track changes. Any edits still pending belong
  // to the track that is leaving and are written to it first, not to the one
  // arriving — that mistake would silently tag the wrong file.
  createEffect(() => {
    const id = props.fileId;
    flush();
    owner = id;
    setCurrent({});
    if (id > 0) {
      quickFields(id, names())
        .then((v) => {
          if (props.fileId === id) setCurrent(v);
        })
        .catch(() => {});
    }
  });

  onCleanup(() => flush());

  /** Values currently set for a group. */
  const valuesOf = (g: QuickGroup): string[] =>
    (current()[g.field] ?? "")
      .split(",")
      .map((v) => v.trim())
      .filter(Boolean);

  const isOn = (g: QuickGroup, value: string) =>
    valuesOf(g).some((v) => v.toLowerCase() === value.toLowerCase());

  /**
   * Turns one value on or off.
   *
   * Pressing the value a track already has removes it, so the same key both
   * sets and corrects — with a hand on the keyboard and eyes elsewhere, undo has
   * to be the key you just pressed.
   */
  function toggle(g: QuickGroup, value: string) {
    if (props.fileId <= 0) return;
    const on = isOn(g, value);
    let next: string[];
    if (g.multi) {
      next = on
        ? valuesOf(g).filter((v) => v.toLowerCase() !== value.toLowerCase())
        : [...valuesOf(g), value];
    } else {
      next = on ? [] : [value];
    }
    setCurrent({ ...current(), [g.field]: next.join(", ") });
    schedule();
  }

  function schedule() {
    setPending(true);
    if (timer) window.clearTimeout(timer);
    timer = window.setTimeout(flush, FLUSH_MS);
  }

  /** Writes what is pending to the track it belongs to. */
  function flush() {
    if (timer) {
      window.clearTimeout(timer);
      timer = undefined;
    }
    if (!pending() || owner <= 0) return;

    const custom: Record<string, string> = {};
    for (const g of props.groups) {
      if (g.field) custom[g.field] = current()[g.field] ?? "";
    }
    const rating = props.rating ? (current()[RATING_FIELD] ?? "0") : undefined;
    setPending(false);

    writeTags([owner], {}, custom, rating)
      .then(() => props.onWritten())
      .catch(() => {});
  }

  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing =
        e.target instanceof HTMLElement &&
        (e.target.tagName === "INPUT" ||
          e.target.tagName === "TEXTAREA" ||
          e.target.isContentEditable);
      if (typing || e.ctrlKey || e.altKey || e.metaKey) return;

      if (e.key === "Escape") {
        e.preventDefault();
        props.onClose();
        return;
      }

      const pressed = e.key.toLowerCase();

      // Stars are on the number row with a shift, so the digits stay free for
      // the first tag group — energy is pressed far more often than a rating.
      if (props.rating && e.shiftKey) {
        const digit = "!\"£$%".indexOf(e.key);
        const viaCode = ["Digit1", "Digit2", "Digit3", "Digit4", "Digit5"].indexOf(e.code);
        const star = viaCode >= 0 ? viaCode : digit;
        if (star >= 0) {
          e.preventDefault();
          e.stopPropagation();
          rate(star + 1);
          return;
        }
      }

      props.groups.forEach((g, row) => {
        const keys = QUICK_KEYS[row] ?? [];
        const index = keys.indexOf(pressed);
        if (index >= 0 && index < g.values.length) {
          e.preventDefault();
          // Stopped here so the space bar and arrow keys still reach the app
          // while these do not: a digit means energy in this mode, not
          // something the list should act on.
          e.stopPropagation();
          toggle(g, g.values[index]);
        }
      });
    };
    // Capture, so the list's own key handling does not see these first.
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  return (
    <div class="quicktag">
      <div class="quickrows">
        <Show when={props.rating}>
          <div class="quickrow">
            <span class="quicklabel">Rating</span>
            <For each={[1, 2, 3, 4, 5]}>
              {(n) => (
                <button
                  class={`quickstar ${stars() >= n ? "on" : ""}`}
                  disabled={props.fileId <= 0}
                  onClick={() => rate(n)}
                  title={`${n} star${n > 1 ? "s" : ""} — shift+${n}`}
                >
                  ★
                </button>
              )}
            </For>
          </div>
        </Show>

        {/* A row's colour is handed down as a custom property rather than as a
            class, so a palette entry stays one place rather than seven CSS
            rules that have to be kept in step with it. */}
        <For each={props.groups}>
          {(g, row) => (
            <div
              class="quickrow"
              style={{
                "--tag": colourOf(g.colour).line,
                "--tag-fill": colourOf(g.colour).fill,
              }}
            >
              <span class="quicklabel">{g.label}</span>
              <For each={g.values}>
                {(value, i) => (
                  <button
                    class={`quickbtn ${isOn(g, value) ? "on" : ""}`}
                    disabled={props.fileId <= 0}
                    onClick={() => toggle(g, value)}
                    title={`${g.field} = ${value}`}
                  >
                    <span class="quickkey">{QUICK_KEYS[row()]?.[i()] ?? ""}</span>
                    {value}
                  </button>
                )}
              </For>
            </div>
          )}
        </For>
      </div>

      <span class="quickstate">
        <Show
          when={props.fileId > 0}
          fallback="Play a track to tag it"
        >
          <Show when={pending()} fallback="Saved">
            Saving…
          </Show>
        </Show>
      </span>
      <button class="btn" onClick={props.onClose} title="Close (Esc)">
        Done
      </button>
    </div>
  );
}
