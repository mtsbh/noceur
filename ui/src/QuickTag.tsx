import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import {
  HASHTAG_FIELD,
  RATING_FIELD,
  announceRating,
  onRating,
  quickFields,
  writeTags,
} from "./api";
import { QUICK_KEYS, valueColour, type QuickGroup } from "./settings";

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
 * Whether two values are the same tag. Case aside, and with spaces as dashes,
 * since "peak time" comes back out of a comment as `#peak-time`.
 */
function same(a: string, b: string): boolean {
  const norm = (s: string) => s.trim().toLowerCase().split(/\s+/).join("-");
  return norm(a) === norm(b);
}

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
  /**
   * Fields changed here since the last write, by name.
   *
   * Only these are written. Writing everything the panel had read put back
   * whatever the file held when the track started - a rating cleared from the
   * list in the meantime came back the moment the track changed.
   */
  let touched = new Set<string>();

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
    touched.add(RATING_FIELD);
    // The list shows it now; the file gets it when the track changes.
    announceRating({ ids: [props.fileId], stars: next, from: "quick" });
    schedule();
  }

  // Stars set from the list on the playing track are already written; show
  // them, and drop any rating pending here so it is not written over them.
  onCleanup(
    onRating((r) => {
      if (r.from === "quick" || !r.ids.includes(props.fileId)) return;
      setCurrent({ ...current(), [RATING_FIELD]: String(r.stars) });
      touched.delete(RATING_FIELD);
    }),
  );

  // Re-read whenever the playing track changes. Any edits still pending belong
  // to the track that is leaving and are written to it first, not to the one
  // arriving — that mistake would silently tag the wrong file.
  //
  // Keyed on the id rather than simply on the transport, which is replaced ten
  // times a second while a track plays. Without the check this cleared the panel
  // and re-read the file on every one of those, and the buttons visibly blinked
  // the whole time something was playing.
  createEffect<number>((previous) => {
    const id = props.fileId;
    if (id === previous) return id;

    flush();
    owner = id;
    touched = new Set();
    setCurrent({});
    if (id > 0) {
      quickFields(id, names())
        .then((v) => {
          if (props.fileId === id) setCurrent(v);
        })
        .catch(() => {});
    }
    return id;
  }, 0);

  onCleanup(() => flush(true));

  /** Values currently set for a group. */
  const valuesOf = (g: QuickGroup): string[] =>
    (current()[g.field] ?? "")
      .split(",")
      .map((v) => v.trim())
      .filter(Boolean);

  const isOn = (g: QuickGroup, value: string) => valuesOf(g).some((v) => same(v, value));

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
    // Comment tags are shared by every group kept there, so a group only
    // swaps its own values and leaves the others' tags where they are.
    const mine = (v: string) =>
      g.field !== HASHTAG_FIELD || g.values.some((e) => same(e.value, v));
    const own = valuesOf(g).filter(mine);
    const others = valuesOf(g).filter((v) => !mine(v));
    let next: string[];
    if (g.multi) {
      next = on ? own.filter((v) => !same(v, value)) : [...own, value];
    } else {
      next = on ? [] : [value];
    }
    setCurrent({ ...current(), [g.field]: [...others, ...next].join(", ") });
    touched.add(g.field);
    schedule();
  }

  function schedule() {
    setPending(true);
    if (timer) window.clearTimeout(timer);
    timer = window.setTimeout(flush, FLUSH_MS);
  }

  /**
   * Writes what is pending to the track it belongs to.
   *
   * Held back while the player still has that file open. Writing a tag rewrites
   * the whole file, and on a long lossless track that is real work happening
   * underneath a decoder that is reading it — which stalled playback and
   * sometimes stopped it outright. Waiting costs nothing: the edits are already
   * on screen, and the moment the player moves to another track this runs.
   *
   * `force` is for closing the panel, where holding on would mean losing them.
   * One rewrite of a file that happens to be playing is a far better outcome
   * than discarding what somebody just tagged.
   */
  function flush(force = false) {
    if (timer) {
      window.clearTimeout(timer);
      timer = undefined;
    }
    if (!pending() || owner <= 0) return;
    if (!force && owner === props.fileId) return;

    const custom: Record<string, string> = {};
    for (const g of props.groups) {
      if (g.field && g.field !== HASHTAG_FIELD && touched.has(g.field)) {
        custom[g.field] = current()[g.field] ?? "";
      }
    }
    // Comment tags go as what should be there and what should not, so the
    // rest of the comment is rebuilt from the file rather than from here.
    let hashtags: { set: string[]; clear: string[] } | undefined;
    if (touched.has(HASHTAG_FIELD)) {
      const on = (current()[HASHTAG_FIELD] ?? "")
        .split(",")
        .map((v) => v.trim())
        .filter(Boolean);
      const offered = props.groups
        .filter((g) => g.field === HASHTAG_FIELD)
        .flatMap((g) => g.values.map((e) => e.value));
      hashtags = { set: on, clear: offered.filter((v) => !on.some((o) => same(o, v))) };
    }
    const rating =
      props.rating && touched.has(RATING_FIELD) ? (current()[RATING_FIELD] ?? "0") : undefined;
    touched = new Set();
    setPending(false);
    if (Object.keys(custom).length === 0 && rating === undefined && !hashtags) return;

    writeTags([owner], {}, custom, rating, hashtags)
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
          toggle(g, g.values[index].value);
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

        {/* Colours are handed down as custom properties rather than as classes,
            so a palette entry stays one place rather than seven CSS rules that
            have to be kept in step with it. Each value gets its own, because a
            row where every value looks alike is a row you have to read. */}
        <For each={props.groups}>
          {(g, row) => (
            <div class="quickrow">
              <span class="quicklabel">{g.label}</span>
              <For each={g.values}>
                {(entry, i) => (
                  <button
                    class={`quickbtn ${isOn(g, entry.value) ? "on" : ""}`}
                    style={
                      g.coloured
                        ? {
                            "--tag": valueColour(entry, i()).line,
                            "--tag-fill": valueColour(entry, i()).fill,
                          }
                        : {}
                    }
                    disabled={props.fileId <= 0}
                    onClick={() => toggle(g, entry.value)}
                    title={`${g.field} = ${entry.value}`}
                  >
                    <span class="quickkey">{QUICK_KEYS[row()]?.[i()] ?? ""}</span>
                    {entry.value}
                  </button>
                )}
              </For>
            </div>
          )}
        </For>
      </div>

      <span class="quickstate">
        <Show when={props.fileId > 0} fallback="Play a track">
          <Show when={pending()} fallback="Saved">
            {/* Said, because a panel that shows edits and reports nothing
                written invites pressing the keys again. Written when the
                track changes. */}
            Unsaved
          </Show>
        </Show>
      </span>
      <button class="btn" onClick={props.onClose} data-tip="Close" data-tip-at="end">
        Done
      </button>
    </div>
  );
}
