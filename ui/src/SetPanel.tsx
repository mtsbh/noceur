import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  on,
  onCleanup,
} from "solid-js";
import { listen } from "@tauri-apps/api/event";
import {
  credentials,
  linkSet,
  openUrl,
  previewSet,
  searchSets,
  setAppearances,
  setOffset,
  setTracklist,
  unlinkSet,
  uploadSet,
  type LinkedSet,
  type SetAppearance,
  type SetSummary,
} from "./api";

/** Gaps shorter than this between two tracks are a blend, not a gap worth a row. */
const GAP_ROW_MS = 60_000;

/** One step of the offset arrows; with Shift, ten. */
const NUDGE_MS = 500;

/** `1:07:10`, or `7:10` under an hour. */
function clock(ms: number): string {
  const whole = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(whole / 3600);
  const m = Math.floor((whole % 3600) / 60);
  const s = String(whole % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** `+1.5 s`, for the offset. */
function seconds(ms: number): string {
  return `${ms >= 0 ? "+" : "−"}${(Math.abs(ms) / 1000).toFixed(1)} s`;
}

/** A file's name without its folder or extension: the title an upload gets. */
function stem(path: string): string {
  return (path.split(/[\\/]/).pop() ?? "").replace(/\.[^.]+$/, "");
}

/**
 * A file name as a search. TrackID wants every word to match and never matches
 * on a number or on punctuation, so dates, catalogue numbers, `@` and commas
 * go, as does anything in brackets.
 */
function searchFrom(path: string): string {
  return stem(path)
    .replace(/\[[^\]]*\]|\([^)]*\)/g, " ")
    .replace(/[^\p{L}\p{N}']+/gu, " ")
    .split(" ")
    .filter((w) => w && !/\d/.test(w))
    .join(" ");
}

/** Titles compared as TrackID and a file name both write them. */
function sameTitle(a: string, b: string): boolean {
  const fold = (t: string) => t.toLowerCase().replace(/\s+/g, " ").trim();
  return fold(a) === fold(b);
}

const HEIGHT_KEY = "noceur.setHeight";
const MIN_HEIGHT = 140;

function storedHeight(): number {
  try {
    const v = Number(localStorage.getItem(HEIGHT_KEY));
    return v >= MIN_HEIGHT ? v : 320;
  } catch {
    return 320;
  }
}

/** How often to ask whether TrackID has finished with an upload. */
const POLL_MS = 60_000;

const SOURCE_NAMES: Record<string, string> = {
  soundcloud: "SoundCloud",
  mixcloud: "Mixcloud",
  youtube: "YouTube",
  hearthis: "HearThis",
  upload: "Upload",
};

function SearchGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" aria-hidden="true">
      <circle cx="7" cy="7" r="4.2" />
      <path d="M10.2 10.2 13.4 13.4" />
    </svg>
  );
}

function CopyGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" aria-hidden="true">
      <rect x="5.4" y="5.4" width="7.6" height="7.6" rx="1.4" />
      <path d="M10.6 3.6V3.4A1.4 1.4 0 0 0 9.2 2H4A1.4 1.4 0 0 0 2.6 3.4v5.2A1.4 1.4 0 0 0 4 10h.2" />
    </svg>
  );
}

/** A YouTube video's id from any of the forms its links take. */
function youtubeId(url: string): string | null {
  const m = url.match(/(?:youtube\.com\/(?:watch\?(?:.*&)?v=|embed\/|live\/)|youtu\.be\/)([\w-]{6,})/);
  return m ? m[1] : null;
}

/** Whether a set's published copy can be played here, from a given moment. */
function playable(url: string): "soundcloud" | "youtube" | null {
  if (/^https:\/\/(www\.|m\.)?soundcloud\.com\//.test(url)) return "soundcloud";
  if (youtubeId(url)) return "youtube";
  return null;
}

type SoundCloudWidget = {
  bind: (event: string, fn: () => void) => void;
  seekTo: (ms: number) => void;
  play: () => void;
};
declare global {
  interface Window {
    SC?: { Widget: ((el: HTMLIFrameElement) => SoundCloudWidget) & { Events: { READY: string } } };
  }
}

let soundcloudApi: Promise<void> | null = null;
/** SoundCloud's widget API, loaded the first time a set is played from there. */
function loadSoundCloud(): Promise<void> {
  soundcloudApi ??= new Promise((ok, fail) => {
    const tag = document.createElement("script");
    tag.src = "https://w.soundcloud.com/player/api.js";
    tag.onload = () => ok();
    tag.onerror = () => {
      soundcloudApi = null;
      fail(new Error("SoundCloud's player could not be loaded"));
    };
    document.head.appendChild(tag);
  });
  return soundcloudApi;
}

/**
 * The set as it is published, playing from a moment: SoundCloud's own player or
 * YouTube's, embedded. Not this app's player — the audio is theirs, streamed —
 * which is why it is only offered for a set that is not on disk.
 */
function SourcePlayer(props: { url: string; at: { ms: number; n: number } }) {
  const kind = () => playable(props.url);
  let frame: HTMLIFrameElement | undefined;
  let widget: SoundCloudWidget | null = null;
  let widgetUrl = "";

  createEffect(
    on(
      () => [props.url, props.at] as const,
      ([url, at]) => {
        if (kind() !== "soundcloud" || !frame) return;
        if (widget && widgetUrl === url) {
          widget.seekTo(at.ms);
          widget.play();
          return;
        }
        widget = null;
        widgetUrl = url;
        frame.src = `https://w.soundcloud.com/player/?url=${encodeURIComponent(url)}&auto_play=true&visual=false&show_comments=false&hide_related=true&show_teaser=false&show_artwork=false`;
        loadSoundCloud()
          .then(() => {
            if (!frame || !window.SC || widgetUrl !== url) return;
            const w = window.SC.Widget(frame);
            w.bind(window.SC.Widget.Events.READY, () => {
              widget = w;
              w.seekTo(props.at.ms);
              w.play();
            });
          })
          .catch(() => {});
      },
    ),
  );

  return (
    <Show
      when={kind() === "youtube"}
      fallback={<iframe ref={frame} class="setpsc" allow="autoplay; encrypted-media" title="SoundCloud" />}
    >
      <iframe
        class="setpyt"
        allow="autoplay; encrypted-media"
        title="YouTube"
        src={`https://www.youtube.com/embed/${youtubeId(props.url)}?autoplay=1&start=${Math.floor(props.at.ms / 1000)}&n=${props.at.n}`}
      />
    </Show>
  );
}

type Row =
  | {
      kind: "track";
      startMs: number;
      endMs: number;
      artist: string;
      title: string;
      label: string;
      plays: number;
    }
  | { kind: "gap"; startMs: number; endMs: number };

/**
 * The tracklist of the set that is playing.
 *
 * The tracks TrackID heard, in order, each one a click from where it starts,
 * and a thin strip above them marking where in the set each falls. No
 * waveform of its own: the player's bar already draws one. Until the file is
 * linked, a search for its entry on TrackID, prefilled from its name.
 */
export default function SetPanel(props: {
  /** The loaded track. 0 when nothing is. */
  fileId: number;
  path: string;
  durationMs: number;
  positionMs: number;
  playing: boolean;
  onSeek: (ms: number) => void;
  /** After the file is linked, relinked or unlinked. */
  onLinked: () => void;
  /** Stops this app's player, for a set played from where it is published. */
  onPausePlayer: () => void;
  onSettings: () => void;
  onClose: () => void;
}) {
  /** `undefined` while it is being read. */
  const [linked, setLinked] = createSignal<LinkedSet | null | undefined>(
    undefined,
  );
  const [choosing, setChoosing] = createSignal(false);
  const [query, setQuery] = createSignal("");
  const [results, setResults] = createSignal<SetSummary[] | null>(null);
  const [busy, setBusy] = createSignal("");
  const [error, setError] = createSignal("");
  /** Whether a trackid.net sign-in is stored, so sets can be uploaded. */
  const [canUpload, setCanUpload] = createSignal(false);
  /** Bytes of the upload sent and in all, while one is going. */
  const [sending, setSending] = createSignal<{ sent: number; total: number } | null>(null);
  /** The title of an upload TrackID is still listening to. */
  const [waiting, setWaiting] = createSignal<string | null>(null);
  /** A search result being looked at, linked to nothing. */
  const [preview, setPreview] = createSignal<LinkedSet | null>(null);
  /** Where the published copy of the previewed set is playing from. */
  const [streaming, setStreaming] = createSignal<{ ms: number; n: number } | null>(null);

  /** The tracklist on screen: the one being previewed, or this file's own. */
  const shown = () => preview() ?? (linked() && !choosing() ? linked() : null);
  /** The length the shown tracklist runs to. */
  const span = () =>
    preview() ? (preview()!.link.durationMs ?? 0) : props.durationMs;

  /** Plays the previewed set where it is published, or opens it there. */
  function playPublished(ms: number) {
    const set = preview();
    if (!set) return;
    if (!playable(set.link.url)) {
      openUrl(set.link.url || `https://trackid.net/audiostreams/${set.link.slug}`).catch(() => {});
      return;
    }
    props.onPausePlayer();
    setStreaming((s) => ({ ms: Math.max(0, ms), n: (s?.n ?? 0) + 1 }));
  }
  /** The row whose artist and title were just copied, for the tick. */
  const [copied, setCopied] = createSignal<number | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(copiedTimer));

  function copy(text: string, row: number) {
    navigator.clipboard
      .writeText(text)
      .then(() => {
        setCopied(row);
        clearTimeout(copiedTimer);
        copiedTimer = setTimeout(() => setCopied(null), 1400);
      })
      .catch(() => {});
  }

  credentials()
    .then((c) => setCanUpload(c.trackidUpload != null))
    .catch(() => {});

  const unlisten = listen<{ fileId: number; sent: number; total: number }>(
    "set-upload-progress",
    (e) => {
      if (e.payload.fileId === props.fileId) setSending(e.payload);
    },
  );
  onCleanup(() => {
    unlisten.then((stop) => stop()).catch(() => {});
  });

  // Memoised: the prop is read from the whole transport state, which is
  // replaced ten times a second while something plays, and `on` alone would
  // reset the panel on every one of those.
  const fileId = createMemo(() => props.fileId);

  createEffect(
    on(fileId, (id) => {
      setLinked(undefined);
      setChoosing(false);
      setResults(null);
      setError("");
      setWaiting(null);
      setSending(null);
      setPreview(null);
      setStreaming(null);
      setQuery(searchFrom(props.path));
      if (id <= 0) return;
      setTracklist(id)
        .then((set) => {
          if (props.fileId !== id) return;
          setLinked(set);
          // Not linked yet: look for it at once. An upload of this very file
          // carries its name, and is linked without asking.
          if (set === null && query().trim()) lookFor(id, true);
        })
        .catch((e) => setError(String(e)));
    }),
  );

  const offset = () => (preview() ? 0 : (linked()?.link.offsetMs ?? 0));

  /** Tracks in this copy's time, with the gaps worth a row between them. */
  const rows = createMemo<Row[]>(() => {
    const set = shown();
    if (!set) return [];
    const shift = preview() ? 0 : set.link.offsetMs;
    const out: Row[] = [];
    let reached = 0;
    for (const t of set.tracks) {
      const startMs = t.startMs + shift;
      if (startMs - reached >= GAP_ROW_MS)
        out.push({ kind: "gap", startMs: reached, endMs: startMs });
      out.push({ kind: "track", ...t, startMs, endMs: t.endMs + shift });
      reached = Math.max(reached, t.endMs + shift);
    }
    const end = span();
    if (end - reached >= GAP_ROW_MS)
      out.push({ kind: "gap", startMs: reached, endMs: end });
    return out;
  });

  const tracks = () => rows().filter((r) => r.kind === "track");

  /** The track under a moment; the latest-starting one where two overlap in a blend. */
  function trackAt(ms: number) {
    let found: Row | null = null;
    for (const r of tracks()) {
      if (ms >= r.startMs && ms < r.endMs) found = r;
    }
    return found as Extract<Row, { kind: "track" }> | null;
  }

  const current = createMemo(() => {
    // A previewed set is another recording: nothing in it is playing here.
    if (preview()) return -1;
    const pos = props.positionMs;
    const list = rows();
    for (let i = list.length - 1; i >= 0; i--) {
      if (pos >= list[i].startMs && pos < list[i].endMs) return i;
    }
    return -1;
  });

  /** Share of the set's length TrackID put a name to. */
  const coverage = createMemo(() => {
    const total = span();
    if (total <= 0) return 0;
    let covered = 0;
    let reached = 0;
    for (const t of tracks()) {
      const from = Math.max(t.startMs, reached);
      if (t.endMs > from) covered += t.endMs - from;
      reached = Math.max(reached, t.endMs);
    }
    return Math.min(1, covered / total);
  });

  // ── The strip ─────────────────────────────────────────────────────────────

  /** A moment as a share of the set's length, for placing marks. */
  const pct = (ms: number) =>
    props.durationMs > 0 ? `${Math.min(100, Math.max(0, (ms / props.durationMs) * 100))}%` : "0%";

  const now = createMemo(() => {
    const i = current();
    return i >= 0 ? rows()[i] : null;
  });

  /** A click on a marked stretch starts that track; anywhere else, that moment. */
  function stripClick(e: MouseEvent) {
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const ms = Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)) * props.durationMs;
    props.onSeek(Math.round(trackAt(ms)?.startMs ?? ms));
  }

  // ── Height ────────────────────────────────────────────────────────────────

  const [height, setHeight] = createSignal(storedHeight());

  function startResize(e: PointerEvent) {
    e.preventDefault();
    const grip = e.currentTarget as HTMLElement;
    grip.setPointerCapture(e.pointerId);
    const startY = e.clientY;
    const startH = height();
    const max = Math.round(window.innerHeight * 0.7);
    const move = (ev: PointerEvent) =>
      setHeight(Math.round(Math.min(max, Math.max(MIN_HEIGHT, startH + startY - ev.clientY))));
    const up = () => {
      grip.removeEventListener("pointermove", move);
      grip.removeEventListener("pointerup", up);
      try {
        localStorage.setItem(HEIGHT_KEY, String(height()));
      } catch {
        /* remembered for this session only */
      }
    };
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", up);
  }

  // ── The list ──────────────────────────────────────────────────────────────

  let list: HTMLDivElement | undefined;
  let pointerIn = false;
  createEffect(
    on(current, (i) => {
      // Follows the playhead, except under the pointer: a list that scrolls
      // while it is being aimed at moves the row about to be clicked.
      if (i < 0 || !list || pointerIn || !props.playing) return;
      const row = list.children[i] as HTMLElement | undefined;
      row?.scrollIntoView({ block: "nearest" });
    }),
  );

  // ── Linking ───────────────────────────────────────────────────────────────

  /**
   * The query, then shorter ones. TrackID wants every word to match, so a full
   * file name — DJ, venue, city, date — finds nothing even when the DJ has ten
   * sets there; the part before ` - ` or ` @ ` usually is the DJ.
   */
  function attempts(q: string): string[] {
    const out = [q];
    const head = q.split(/\s+[-–@|]\s+|\s+(?:live|b2b)\s+/i)[0].trim();
    if (head && head !== q) out.push(head);
    const two = q.split(/\s+/).slice(0, 2).join(" ");
    if (two && !out.includes(two)) out.push(two);
    return out;
  }

  async function firstHit(
    q: string,
  ): Promise<{ query: string; found: SetSummary[] }> {
    let last = { query: q, found: [] as SetSummary[] };
    for (const attempt of attempts(q)) {
      last = { query: attempt, found: await searchSets(attempt) };
      if (last.found.length > 0) break;
    }
    return last;
  }

  /** The search's result that is this file's own upload, if there is one. */
  function ownUpload(found: SetSummary[]): SetSummary | undefined {
    const title = stem(props.path);
    const mine = found.filter((s) => sameTitle(s.title, title));
    // Uploaded twice is common; the one TrackID has finished with wins.
    return mine.find((s) => s.status === "ready") ?? mine[0];
  }

  /**
   * Searches, and links the file's own upload when it is ready. `quiet` keeps a
   * failed lookup on opening from showing an error nobody asked for.
   */
  function lookFor(id: number, quiet = false) {
    const q = query().trim();
    if (!q) return;
    setBusy("search");
    if (!quiet) setError("");
    firstHit(q)
      .then(({ found }) => {
        if (props.fileId !== id) return;
        show(found);
        const own = ownUpload(found);
        if (own?.status === "ready") choose(own);
        else if (own) setWaiting(own.title);
      })
      .catch((e) => {
        if (!quiet) setError(String(e));
      })
      .finally(() => setBusy((b) => (b === "search" ? "" : b)));
  }

  // While an upload is being listened to, ask now and then whether it is done.
  const poll = setInterval(() => {
    if (waiting() && linked() === null && busy() === "") lookFor(props.fileId, true);
  }, POLL_MS);
  onCleanup(() => clearInterval(poll));

  function upload() {
    const id = props.fileId;
    setBusy("upload");
    setError("");
    setSending({ sent: 0, total: 1 });
    uploadSet(id)
      .then((title) => {
        if (props.fileId !== id) return;
        setWaiting(title);
        setQuery(searchFrom(props.path));
      })
      .catch((e) => setError(String(e)))
      .finally(() => {
        setSending(null);
        setBusy("");
      });
  }

  /** The entries the length of this file first: those are almost certainly it. */
  function show(found: SetSummary[]) {
    setResults([...found.filter(sameLength), ...found.filter((s) => !sameLength(s))]);
  }

  const isMp3 = () => /\.mp3$/i.test(props.path);
  /** Offered when this file is not on TrackID under its own name. */
  const uploadable = () =>
    canUpload() && isMp3() && !waiting() && !ownUpload(results() ?? []);

  function search() {
    const q = query().trim();
    if (!q) return;
    setBusy("search");
    setError("");
    firstHit(q)
      .then(({ query: used, found }) => {
        // Shown in the box, so it is clear which words found these.
        if (used !== q) setQuery(used);
        show(found);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(""));
  }

  function sameLength(s: SetSummary) {
    const len = props.durationMs;
    return (
      s.durationMs != null &&
      len > 0 &&
      Math.abs(s.durationMs - len) <= 3_000
    );
  }

  function choose(s: SetSummary) {
    const id = props.fileId;
    setBusy(s.slug);
    setError("");
    linkSet(id, s.slug)
      .then((set) => {
        if (props.fileId !== id) return;
        setLinked(set);
        setChoosing(false);
        setResults(null);
        props.onLinked();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(""));
  }

  function unlink() {
    const id = props.fileId;
    unlinkSet(id)
      .then(() => {
        if (props.fileId === id) setLinked(null);
        props.onLinked();
      })
      .catch((e) => setError(String(e)));
  }

  function nudge(by: number) {
    const id = props.fileId;
    const next = offset() + by;
    // Shown at once; the stored value follows.
    setLinked((set) =>
      set ? { ...set, link: { ...set.link, offsetMs: next } } : set,
    );
    setOffset(id, next).catch((e) => setError(String(e)));
  }

  const needsKey = () => /Settings/.test(error());
  const searchShown = () => !preview() && (linked() === null || choosing());

  function look(s: SetSummary) {
    setBusy(s.slug);
    setError("");
    previewSet(s.slug)
      .then((set) => {
        setStreaming(null);
        setPreview(set);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(""));
  }

  /** Links the previewed set to this file. */
  function usePreview() {
    const set = preview();
    if (!set) return;
    setStreaming(null);
    choose({ slug: set.link.slug } as SetSummary);
    setPreview(null);
  }

  return (
    <div class="setp" style={{ height: `${height()}px` }}>
      <div class="setpgrip" onPointerDown={startResize} />
      <Show
        when={shown()}
        fallback={
          <div class="setphead">
            <span class="setptitle dim">
              {props.fileId > 0 ? "Tracklist" : "Play a set to see its tracklist"}
            </span>
            <span class="setpsep" />
            <Show when={choosing()}>
              <button class="setplink" onClick={() => setChoosing(false)}>
                Keep current
              </button>
            </Show>
            <button class="setpclose" aria-label="Close tracklist" onClick={props.onClose}>
              ×
            </button>
          </div>
        }
      >
        {(set) => (
          <div class="setphead tall">
            <Show when={preview()}>
              <button
                class="setplink setpback"
                onClick={() => {
                  setStreaming(null);
                  setPreview(null);
                }}
              >
                ‹ Back
              </button>
            </Show>
            <Show when={set().link.artwork} fallback={<span class="setpart blank" />}>
              <img
                class="setpart"
                src={set().link.artwork}
                alt=""
                draggable={false}
                onError={(e) => e.currentTarget.classList.add("blank")}
              />
            </Show>
            <div class="setpwho">
              <span class="setptitle" title={set().link.title}>
                {set().link.title}
              </span>
              <span class="setpmeta">
                {[set().link.channel, set().link.published, clock(span())]
                  .filter(Boolean)
                  .join(" · ")}
                {` · ${tracks().length} tracks, ${Math.round(coverage() * 100)}% identified`}
              </span>
            </div>
            <span class="setpsep" />
            <Show when={preview()}>
              <span class="setpmeta">Not linked</span>
              <button class="btn primary setpuse" onClick={usePreview}>
                Use for this file
              </button>
            </Show>
            <Show when={!preview()}>
            <span class="setpmeta">Offset</span>
            <button
              class="setpnudge"
              aria-label="Earlier"
              onClick={(e) => nudge(e.shiftKey ? -NUDGE_MS * 10 : -NUDGE_MS)}
            >
              ‹
            </button>
            <span
              class="setpoffset mono"
              data-tip="Double-click to reset"
              onDblClick={() => offset() !== 0 && nudge(-offset())}
            >
              {seconds(offset())}
            </span>
            <button
              class="setpnudge"
              aria-label="Later"
              onClick={(e) => nudge(e.shiftKey ? NUDGE_MS * 10 : NUDGE_MS)}
            >
              ›
            </button>
            <Show when={set().link.url}>
              <button class="setplink" onClick={() => openUrl(set().link.url).catch(() => {})}>
                Source
              </button>
            </Show>
            <button class="setplink" onClick={() => setChoosing(true)}>
              Change
            </button>
            <button class="setplink" onClick={unlink}>
              Remove
            </button>
            </Show>
            <button class="setpclose" aria-label="Close tracklist" onClick={props.onClose}>
              ×
            </button>
          </div>
        )}
      </Show>

      <Show when={props.fileId > 0 && searchShown()}>
        <div class="setpsearch">
          <label class="setpfield">
            <SearchGlyph />
            <input
              value={query()}
              placeholder="Set title, DJ or venue"
              spellcheck={false}
              onInput={(e) => setQuery(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") search();
              }}
              ref={(el) => queueMicrotask(() => el.focus())}
            />
          </label>
          <button class="btn" disabled={busy() !== "" || !query().trim()} onClick={search}>
            {busy() === "search" ? "Searching…" : "Find"}
          </button>
          <Show when={uploadable() || sending()}>
            <button
              class="btn primary"
              disabled={busy() !== ""}
              onClick={upload}
              data-tip="Sends this file to TrackID, privately, to be listened to"
            >
              {sending()
                ? `Uploading ${Math.floor((sending()!.sent / Math.max(1, sending()!.total)) * 100)}%`
                : "Upload to TrackID"}
            </button>
          </Show>
        </div>
      </Show>

      <Show when={waiting() && linked() === null}>
        <div class="setpwait">
          TrackID is listening to this set. It links here when it's done, usually within the
          hour.
        </div>
      </Show>

      <Show when={error()}>
        <div class="setperror">
          {error()}
          <Show when={needsKey()}>
            {" "}
            <button class="setplink" onClick={props.onSettings}>
              Open Settings
            </button>
          </Show>
        </div>
      </Show>

      <Show when={searchShown() && results()}>
        {(found) => (
          <Show
            when={found().length > 0}
            fallback={
              <p class="setpempty">
                Not on TrackID.{" "}
                {uploadable()
                  ? "Upload it, and TrackID will make its tracklist."
                  : "Try the DJ's name alone."}
              </p>
            }
          >
            <div class="setpcols results">
              <span />
              <span>Set</span>
              <span>Channel</span>
              <span class="num">Length</span>
              <span class="num">Covered</span>
              <span class="num">Published</span>
            </div>
            <div class="setplist">
              <For each={found()}>
                {(s) => (
                  <button
                    class={`setpresult ${sameLength(s) ? "match" : ""}`}
                    disabled={busy() !== "" || s.status !== "ready"}
                    onClick={() => look(s)}
                  >
                    <Show when={s.artwork} fallback={<span class="setpthumb blank" />}>
                      <img
                        class="setpthumb"
                        src={s.artwork}
                        alt=""
                        loading="lazy"
                        draggable={false}
                        // An upload's artwork address leads nowhere; an empty
                        // square says so better than a broken picture.
                        onError={(e) => e.currentTarget.classList.add("blank")}
                      />
                    </Show>
                    <span class="setpname">{s.title}</span>
                    <span class="setpname dim">
                      <span class="setpsource">{SOURCE_NAMES[s.source] ?? s.source}</span>
                      {/* An upload's channel is a lone dash. */}
                      {s.channel === "-" ? "" : s.channel}
                    </span>
                    <span class="mono num">{s.durationMs != null ? clock(s.durationMs) : "—"}</span>
                    <span class="num">
                      {busy() === s.slug ? (
                        "Linking…"
                      ) : s.status === "queued" ? (
                        <span class="dim">Queued</span>
                      ) : s.status === "processing" ? (
                        <span class="dim">Listening…</span>
                      ) : s.coverage != null ? (
                        `${Math.round(s.coverage * 100)}%`
                      ) : (
                        "—"
                      )}
                    </span>
                    <span class="mono num dim">{s.published || "—"}</span>
                  </button>
                )}
              </For>
            </div>
          </Show>
        )}
      </Show>

      <Show when={preview() && streaming()}>
        <div class="setpstream">
          <SourcePlayer url={preview()!.link.url} at={streaming()!} />
          <div class="setpwho">
            <span class="setpmeta">
              {playable(preview()!.link.url) === "youtube" ? "YouTube" : "SoundCloud"} · from{" "}
              {clock(streaming()!.ms)}
            </span>
            <span class="setptitle">
              {(() => {
                const t = trackAt(streaming()!.ms);
                return t ? `${t.artist} — ${t.title}` : "Not identified";
              })()}
            </span>
          </div>
          <span class="setpsep" />
          <button class="setplink" onClick={() => setStreaming(null)}>
            Stop
          </button>
        </div>
      </Show>

      <Show when={shown()}>
        <Show when={!preview()}>
        <div class="setpstrip" onClick={stripClick}>
          <For each={tracks()}>
            {(t) => (
              <span
                class={`setpspan ${now() === t ? "on" : ""}`}
                style={{ left: pct(t.startMs), width: pct(t.endMs - t.startMs) }}
                data-tip={`${clock(t.startMs)} · ${t.artist} — ${t.title}`}
              />
            )}
          </For>
          <span class="setppos" style={{ left: pct(props.positionMs) }} />
        </div>
        </Show>

        <Show when={tracks().length === 0}>
          <p class="setpempty">TrackID listened to this set and recognised no tracks in it.</p>
        </Show>
        <div class="setpcols tracks">
          <span />
          <span>Start</span>
          <span>End</span>
          <span>Artist</span>
          <span>Title</span>
          <span>Label</span>
          <span class="num" data-tip="Sets TrackID heard it in">
            Sets
          </span>
          <span />
        </div>
        <div
          class="setplist"
          ref={list}
          onPointerEnter={() => (pointerIn = true)}
          onPointerLeave={() => (pointerIn = false)}
        >
          <For each={rows()}>
            {(r, i) => (
              <div
                class={`setprow ${r.kind === "gap" ? "gap" : ""} ${current() === i() ? "sel" : ""}`}
                onClick={() =>
                  preview() ? playPublished(r.startMs) : props.onSeek(Math.max(0, r.startMs))
                }
              >
                <span class="setpplay" aria-hidden="true">
                  {current() === i() && props.playing ? "❚❚" : "▶"}
                </span>
                <span class="mono">{clock(r.startMs)}</span>
                <span class="mono dim">{clock(r.endMs)}</span>
                <Show
                  when={r.kind === "track" ? r : null}
                  fallback={
                    <>
                      <span class="setpgap">
                        {Math.round((r.endMs - r.startMs) / 60_000)} min not identified
                      </span>
                      <span />
                    </>
                  }
                >
                  {(t) => (
                    <>
                      <span class="setpname">{t().artist}</span>
                      <span class="setpname strong">{t().title}</span>
                      <span class="setpname dim">{t().label || "—"}</span>
                      <span class="mono num dim">{t().plays > 0 ? t().plays : "—"}</span>
                      <button
                        class={`setpcopy ${copied() === i() ? "done" : ""}`}
                        aria-label="Copy artist and title"
                        data-tip={copied() === i() ? "Copied" : "Copy artist — title"}
                        onClick={(e) => {
                          e.stopPropagation();
                          copy(`${t().artist} - ${t().title}`, i());
                        }}
                      >
                        {copied() === i() ? "✓" : <CopyGlyph />}
                      </button>
                    </>
                  )}
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

/**
 * Where the selected track was played, among the sets linked to a tracklist.
 *
 * For the sidebar: a line per play, and a click starts the set at that moment.
 * Says nothing at all when there is nothing to say, so the sidebar of a track
 * nobody played looks as it always did.
 */
export function InYourSets(props: {
  fileId: number | null;
  /** Bumped when a link may have changed. */
  version: number;
  onJump: (setFileId: number, ms: number) => void;
}) {
  const [found, setFound] = createSignal<SetAppearance[]>([]);

  const key = createMemo(() => `${props.fileId}:${props.version}`);
  createEffect(
    on(key, () => {
      const id = props.fileId;
      setFound([]);
      if (!id) return;
      setAppearances(id)
        .then((list) => {
          if (props.fileId === id) setFound(list);
        })
        .catch(() => {});
    }),
  );

  return (
    <Show when={found().length > 0}>
      <div class="sidesets">
        <span class="sidesetshead">In your sets</span>
        <For each={found()}>
          {(a) => (
            <button
              class="sideset"
              disabled={a.fileId == null}
              data-tip={a.fileId == null ? "Not attached" : undefined}
              onClick={() =>
                a.fileId != null && props.onJump(a.fileId, a.startMs)
              }
            >
              <span class="setpplay" aria-hidden="true">
                ▶
              </span>
              <span>{a.setTitle}</span>
              <span class="mono">{clock(a.startMs)}</span>
            </button>
          )}
        </For>
      </div>
    </Show>
  );
}
