import { createSignal, createEffect, on, For, Show, onCleanup } from "solid-js";
import { search, searchTag, type SearchHit, type TagLink } from "./api";

/**
 * How long typing settles before a query is sent.
 *
 * Long enough that a typed word is one query rather than five, short enough that
 * results feel attached to the keystroke. The spec budgets 80ms to first results;
 * this is the part of that budget spent deliberately.
 */
const DEBOUNCE_MS = 90;

/**
 * The `Ctrl+F` search popover.
 *
 * Searches every pinned folder plus the tree the user is currently in — not the
 * current directory alone, which is the case the feature exists for.
 */
export default function Search(props: {
  scopes: string[];
  /** Opens on every track with this tag - a sidebar link - until typed over. */
  tag?: TagLink;
  onClose: () => void;
  onOpen: (dirPath: string, fileId: number) => void;
}) {
  const [query, setQuery] = createSignal("");
  const [hits, setHits] = createSignal<SearchHit[]>([]);
  const [cursor, setCursor] = createSignal(0);
  const [busy, setBusy] = createSignal(false);
  /** The tag being listed; gone once something is typed. */
  const [tag, setTag] = createSignal(props.tag ?? null);

  let input: HTMLInputElement | undefined;
  let listEl: HTMLDivElement | undefined;
  let timer: number | undefined;
  // Only the newest query may write results: a slow one landing late would
  // otherwise replace the answers to what the user is typing now.
  let generation = 0;

  const listed = tag();
  if (listed) {
    const mine = ++generation;
    setBusy(true);
    searchTag(listed, props.scopes)
      .then((found) => {
        if (mine === generation) setHits(found);
      })
      .catch(() => {})
      .finally(() => {
        if (mine === generation) setBusy(false);
      });
  }

  createEffect(
    on(query, (q) => {
      if (q !== "") setTag(null);
      if (tag()) return;
      window.clearTimeout(timer);
      const mine = ++generation;

      if (q.trim() === "") {
        setHits([]);
        setBusy(false);
        return;
      }

      setBusy(true);
      timer = window.setTimeout(() => {
        search(q, props.scopes)
          .then((found) => {
            if (mine !== generation) return;
            setHits(found);
            setCursor(0);
          })
          .catch(() => {
            if (mine === generation) setHits([]);
          })
          .finally(() => {
            if (mine === generation) setBusy(false);
          });
      }, DEBOUNCE_MS);
    }),
  );

  onCleanup(() => window.clearTimeout(timer));

  /** Keeps the highlighted result in view when moving through it with the keyboard. */
  function revealCursor() {
    listEl
      ?.querySelector(".hit.on")
      ?.scrollIntoView({ block: "nearest" });
  }

  function move(delta: number) {
    const n = hits().length;
    if (n === 0) return;
    // Wraps, so holding a key does not stall at the end of a long result list.
    setCursor((cursor() + delta + n) % n);
    revealCursor();
  }

  function openHit(hit: SearchHit) {
    // A directory result opens itself; a file opens the folder holding it and
    // asks the list to select it.
    const target =
      hit.kind === 0
        ? `${hit.dirPath}${hit.dirPath.endsWith("\\") ? "" : "\\"}${hit.name}`
        : hit.dirPath;
    props.onOpen(target, hit.fileId);
    props.onClose();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const hit = hits()[cursor()];
      if (hit) openHit(hit);
    }
  }

  return (
    <>
      {/* Clicking anywhere outside dismisses, which is what a popover over a
          file list should do. */}
      <div class="scrim" onClick={() => props.onClose()} />
      <div class="finder" onKeyDown={onKey}>
        <input
          ref={(el) => {
            input = el;
            // Focus after mount, so the first keystroke after Ctrl+F is typed
            // into the box rather than lost.
            queueMicrotask(() => input?.focus());
          }}
          class="finput"
          type="text"
          spellcheck={false}
          placeholder={
            tag()
              ? `${tag()!.field[0].toUpperCase()}${tag()!.field.slice(1)}: ${tag()!.value} · type to search`
              : "Search pinned folders and this tree…"
          }
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
        />

        <div class="fresults" ref={(el) => (listEl = el)}>
          <Show
            when={hits().length > 0}
            fallback={
              <div class="fempty">
                {query().trim() === "" && !tag()
                  ? "Type to search."
                  : busy()
                    ? "Searching…"
                    : "Nothing found."}
              </div>
            }
          >
            <For each={hits()}>
              {(hit, i) => (
                <div
                  class={`hit ${i() === cursor() ? "on" : ""} ${hit.kind === 0 ? "dir" : ""}`}
                  onClick={() => openHit(hit)}
                  onMouseEnter={() => setCursor(i())}
                >
                  <span class="hname" title={hit.name}>
                    {hit.name}
                  </span>
                  <span class="num mono">{hit.bpm}</span>
                  <span class="mono">{hit.key}</span>
                  <span class="num mono">{hit.duration}</span>
                  <span class="hpath" title={hit.dirPath}>
                    {hit.dirPath}
                  </span>
                </div>
              )}
            </For>
          </Show>
        </div>

        <div class="fhint">
          <span>↑↓ move</span>
          <span>↵ open</span>
          <span>esc close</span>
          <Show when={hits().length > 0}>
            <span class="grow" />
            <span>{hits().length} results</span>
          </Show>
        </div>
      </div>
    </>
  );
}
