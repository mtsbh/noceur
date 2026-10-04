import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { matchTrack, type Matched, type Query, type TagField, type TrackMatch } from "./api";

/**
 * Shows what the sources think a track is, and hands the chosen one to the
 * tag editor.
 *
 * Nothing is written from here. Picking a match opens the editor with its
 * fields filled in and outlined as edits, so what an autotagger proposes is
 * visible as a proposal — adjustable, and refusable — before anything touches
 * the file. That is the difference between a tool and a thing that reorganises
 * your library while you watch.
 *
 * Sources disagree constantly: a different release date, a genre one calls
 * Techno and another Electronic, a title with and without its remix credit. All
 * of them are shown rather than merged, because which one is right for a
 * particular file is a judgement its owner makes.
 */
/** Rows shown per source. */
const PER_SOURCE = 3;

export default function MatchPicker(props: {
  fileId: number;
  onClose: () => void;
  /** What an earlier search of this track found, so coming back is instant. */
  cached?: Matched;
  /** Told what the search found, so the app can hand it back via `cached`. */
  onLoaded?: (found: Matched) => void;
  /**
   * Called with the fields to pre-fill the editor with, and the cover art the
   * release came with when it has one.
   *
   * The art travels with the fields rather than being applied here: it belongs
   * to the same release, so it is right exactly when they are, and it should be
   * agreed to in the same place and at the same moment they are.
   */
  onChoose: (
    fields: Partial<Record<TagField, string>>,
    art: string | undefined,
  ) => void;
}) {
  const [matches, setMatches] = createSignal<TrackMatch[] | null>(null);
  const [error, setError] = createSignal("");
  /** What the search used, when it was the sound or the file name. */
  const [query, setQuery] = createSignal<Query | null>(null);
  /**
   * Art that failed to load. A source's cover link can point at nothing, and a
   * row with a broken sleeve is worse than no row.
   */
  const [missing, setMissing] = createSignal<Set<string>>(new Set());

  /**
   * What is listed: only releases with a sleeve, and the best few of each
   * source. A row whose art fails drops out and the next one from the same
   * source takes its place.
   */
  const shown = createMemo(() => {
    const per = new Map<string, number>();
    return (matches() ?? []).filter((m) => {
      if (!m.art || missing().has(m.art)) return false;
      const n = (per.get(m.source) ?? 0) + 1;
      per.set(m.source, n);
      return n <= PER_SOURCE;
    });
  });

  createEffect(() => {
    const id = props.fileId;
    const cached = props.cached;
    (cached ? Promise.resolve(cached) : matchTrack(id))
      .then((r) => {
        setQuery(r.query);
        setMatches(r.matches);
        if (!cached) props.onLoaded?.(r);
      })
      .catch((e) => {
        setMatches([]);
        setError(String(e));
      });
  });

  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  /**
   * Turns a match into editor fields.
   *
   * Only what the source actually knows: a field it has nothing for is left out
   * entirely rather than sent as an empty string, which the editor would read as
   * a request to clear the tag.
   */
  function fieldsOf(m: TrackMatch): Partial<Record<TagField, string>> {
    const out: Partial<Record<TagField, string>> = {};
    if (m.title) out.title = m.title;
    if (m.artists.length) out.artist = m.artists.join(", ");
    if (m.album) out.album = m.album;
    if (m.albumArtist) out.albumArtist = m.albumArtist;
    if (m.label) out.label = m.label;
    if (m.genres.length) out.genre = m.genres.join(", ");
    // A release date is a full ISO date in most sources; the tag holds a year.
    if (m.releaseDate) out.year = m.releaseDate.slice(0, 4);
    if (m.trackNo) out.trackNo = String(m.trackNo);
    if (m.bpm) out.bpm = String(m.bpm);
    if (m.key) out.key = m.key;
    if (m.isrc) out.isrc = m.isrc;
    return out;
  }

  /** The fields this match would actually contribute, for the summary line. */
  function carries(m: TrackMatch): string[] {
    const has: string[] = [];
    if (m.album) has.push("album");
    if (m.label) has.push("label");
    if (m.releaseDate) has.push("year");
    if (m.genres.length) has.push("genre");
    if (m.bpm) has.push("BPM");
    if (m.key) has.push("key");
    if (m.isrc) has.push("ISRC");
    return has;
  }

  const duration = (ms: number | null) => {
    if (!ms) return "";
    const s = Math.round(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  };

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="matches" role="dialog" aria-label="Find metadata">
        <header class="sethead">
          <span class="settitle">Find metadata</span>
          <button
            class="setclose"
            onClick={props.onClose}
            data-tip="Close"
            data-tip-at="end"
            aria-label="Close"
          >
            <CloseIcon />
          </button>
        </header>

        <div class="matchbody">
          <Show when={matches()} fallback={<p class="sethint">Searching…</p>}>
            <Show when={query()}>
              {(q) => (
                <p class="sethint">
                  {q().by === "sound"
                    ? "By sound"
                    : q().by === "shazam"
                      ? "By Shazam"
                      : "From filename"}
                  :{" "}
                  {[q().artist, q().title].filter((s) => s.trim()).join(" — ")}
                </p>
              )}
            </Show>
            <Show
              when={shown().length > 0}
              fallback={
                <p class="sethint">
                  <span data-tip="Try adding artist and title" data-tip-at="start">
                    No match
                  </span>
                </p>
              }
            >
              <For each={shown()}>
                {(m) => (
                  <button
                    class="matchrow"
                    onClick={() => props.onChoose(fieldsOf(m), m.art ?? undefined)}
                  >
                    <span class="matchart">
                      <Show when={m.art}>
                        <img
                          src={m.art!}
                          alt=""
                          loading="lazy"
                          draggable={false}
                          onError={() => setMissing(new Set([...missing(), m.art!]))}
                        />
                      </Show>
                    </span>
                    <span class="matchmain">
                      <span class="matchtitle">
                        {m.artists.join(", ")} — {m.title}
                      </span>
                      <span class="matchmeta">
                        {[
                          m.album,
                          m.label,
                          m.releaseDate?.slice(0, 4),
                          duration(m.durationMs),
                        ]
                          .filter(Boolean)
                          .join(" · ")}
                      </span>
                      <span class="matchcarries">
                        {m.source}
                        <Show when={carries(m).length > 0}>
                          {" · adds "}
                          {carries(m).join(", ")}
                        </Show>
                      </span>
                    </span>
                    {/* The score is shown because it is the one number that says
                        how much to trust the row, and hiding it would be asking
                        for trust the app has not earned. */}
                    <span class="matchscore mono">{Math.round(m.score * 100)}</span>
                  </button>
                )}
              </For>
            </Show>
          </Show>

          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>
        </div>

        <footer class="tagfoot">
          <span class="sethint credit" />
          <button class="btn" onClick={props.onClose}>
            Close
          </button>
        </footer>
      </div>
    </>
  );
}
