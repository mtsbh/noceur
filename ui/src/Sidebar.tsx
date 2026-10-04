import { For, Show, createEffect, createSignal, on } from "solid-js";
import {
  TAG_FIELDS,
  fullArtUrl,
  openUrl,
  readTags,
  writeTags,
  type FileFacts,
  type TagField,
  type TagForm,
  type TagLink,
} from "./api";
import { searchUrl, type Launcher } from "./settings";

/** Fields that share a row in pairs rather than taking the full width. */
const NARROW: TagField[] = ["year", "trackNo", "bpm", "key"];

/** Fields whose label lists the library's tracks with the same value. */
const LINKED: Partial<Record<TagField, TagLink["field"]>> = {
  artist: "artist",
  genre: "genre",
  label: "label",
};

const ART_KEY = "noceur.sideArt";
/** Smallest the cover can be dragged to: still recognisable, not in the way. */
const ART_MIN = 48;

function storedArt(): number | null {
  try {
    const v = Number(localStorage.getItem(ART_KEY));
    return v >= ART_MIN ? v : null;
  } catch {
    return null;
  }
}

/** A site's own icon, by the host its search goes to. */
function favicon(target: string): string {
  try {
    return `https://www.google.com/s2/favicons?domain=${new URL(target).hostname}&sz=32`;
  } catch {
    return "";
  }
}

const FACTS: [keyof FileFacts, string][] = [
  ["format", "Format"],
  ["bitrate", "Bitrate"],
  ["length", "Length"],
  ["sampleRate", "Sample rate"],
  ["bitDepth", "Bit depth"],
  ["size", "Size"],
];

/**
 * The track sidebar: the selected track's cover, what the file is, its tags
 * edited in place, and searches for it on the web.
 *
 * One track, always: the row last clicked or moved to. The tag editor is the
 * place for a selection; this is for going down a list one track at a time,
 * which is why the app plays each track as it is selected while this is open.
 *
 * Edits are held until Apply (or Enter) and dropped by Cancel (or Esc), and
 * dropped too when another track is selected - this is never written behind
 * anyone's back.
 */
export default function Sidebar(props: {
  /** The audio row to describe, or null when the selection is not one. */
  fileId: number | null;
  sites: Launcher[];
  /** Bumped when tags or art may have changed elsewhere. */
  version: number;
  onWritten: () => void;
  onList: (link: TagLink) => void;
  onArt: (fileId: number) => void;
  /** Opens the settings where the sites are chosen. */
  onSites: () => void;
}) {
  const [form, setForm] = createSignal<TagForm | null>(null);
  const [edits, setEdits] = createSignal<Partial<Record<TagField, string>>>({});
  const [hasArt, setHasArt] = createSignal(true);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  /** Defeats the web view's cache once a write may have changed the picture. */
  const [artVersion, setArtVersion] = createSignal(0);

  /** The cover's height, dragged; null is square, the panel's full width. */
  const [artHeight, setArtHeight] = createSignal(storedArt());
  let artEl: HTMLButtonElement | undefined;

  function startArtDrag(e: PointerEvent) {
    if (!artEl) return;
    e.preventDefault();
    const grip = e.currentTarget as HTMLElement;
    try {
      grip.setPointerCapture(e.pointerId);
    } catch {
      /* the drag still works while the pointer stays over the grip */
    }
    const startY = e.clientY;
    const startH = artEl.getBoundingClientRect().height;
    const max = artEl.getBoundingClientRect().width;
    const move = (ev: PointerEvent) => {
      const h = Math.round(Math.min(max, Math.max(ART_MIN, startH + ev.clientY - startY)));
      setArtHeight(h >= max ? null : h);
    };
    const up = () => {
      grip.removeEventListener("pointermove", move);
      grip.removeEventListener("pointerup", up);
      try {
        const h = artHeight();
        if (h === null) localStorage.removeItem(ART_KEY);
        else localStorage.setItem(ART_KEY, String(h));
      } catch {
        /* kept for this session */
      }
    };
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", up);
  }

  function resetArt() {
    setArtHeight(null);
    try {
      localStorage.removeItem(ART_KEY);
    } catch {
      /* nothing stored */
    }
  }

  const dirty = () => Object.keys(edits()).length > 0;

  function load(id: number | null) {
    setError("");
    if (id === null) {
      setForm(null);
      return;
    }
    readTags([id])
      .then((f) => {
        if (props.fileId === id) setForm(f);
      })
      .catch((e) => {
        if (props.fileId === id) {
          setForm(null);
          setError(String(e));
        }
      });
  }

  // A new track drops whatever was typed for the last one.
  createEffect(
    on(
      () => props.fileId,
      (id) => {
        setEdits({});
        setHasArt(true);
        load(id);
      },
    ),
  );

  // Written elsewhere: read again, unless something here is mid-edit.
  createEffect(
    on(
      () => props.version,
      () => {
        setArtVersion((v) => v + 1);
        setHasArt(true);
        if (!dirty()) load(props.fileId);
      },
      { defer: true },
    ),
  );

  const shown = (key: TagField): string => edits()[key] ?? form()?.[key] ?? "";

  async function apply() {
    const id = props.fileId;
    if (!dirty() || busy() || id === null) return;
    setBusy(true);
    setError("");
    try {
      const r = await writeTags([id], edits());
      if (r.failures.length > 0) {
        setError(r.failures[0].reason);
      } else {
        setEdits({});
        props.onWritten();
        load(id);
      }
    } catch (e) {
      setError(String(e));
    }
    setBusy(false);
  }

  function cancel() {
    setEdits({});
    setError("");
  }

  function onFieldKey(e: KeyboardEvent, multiline: boolean) {
    if (e.key === "Enter" && (!multiline || e.ctrlKey)) {
      e.preventDefault();
      void apply();
    } else if (e.key === "Escape" && dirty()) {
      e.preventDefault();
      e.stopPropagation();
      cancel();
    }
  }

  /** What the search buttons look for: the boxes as they read now. */
  const track = () => ({
    artist: shown("artist"),
    title: shown("title"),
    album: shown("album"),
    name: form()?.subject ?? "",
  });

  return (
    <aside class="sidebar" aria-label="Track">
      <Show
        when={props.fileId !== null}
        fallback={<p class="sideempty">Select a track</p>}
      >
        <button
          ref={artEl}
          class={`sideart ${hasArt() ? "" : "empty"}`}
          style={artHeight() === null ? undefined : { "aspect-ratio": "auto", height: `${artHeight()}px` }}
          onClick={() => props.fileId !== null && props.onArt(props.fileId)}
          data-tip="Edit cover"
        >
          <Show when={hasArt()} fallback={<span>No artwork</span>}>
            <img
              src={fullArtUrl(props.fileId!, artVersion())}
              alt=""
              draggable={false}
              onError={() => setHasArt(false)}
            />
          </Show>
        </button>
        <div
          class="sidegrip"
          onPointerDown={startArtDrag}
          onDblClick={resetArt}
          title="Drag to resize · double-click for full size"
        />

        <div class="sidebody">
          <Show when={form()?.facts}>
            {(facts) => (
              <dl class="sidefacts">
                <For each={FACTS.filter(([k]) => facts()[k])}>
                  {([k, label]) => (
                    <div>
                      <dt>{label}</dt>
                      <dd>{facts()[k]}</dd>
                    </div>
                  )}
                </For>
              </dl>
            )}
          </Show>

          <div class="sidetags">
            <For each={TAG_FIELDS}>
              {([key, label]) => {
                const link = LINKED[key];
                return (
                  // A div: a label would pass its clicks to the link inside it.
                  <div class={`sidetag ${NARROW.includes(key) ? "narrow" : ""}`}>
                    <Show
                      when={link && (form()?.[key] ?? "").trim()}
                      fallback={<span class="sidelabel">{label}</span>}
                    >
                      <button
                        class="sidelabel sidelink"
                        onClick={(e) => {
                          e.preventDefault();
                          props.onList({ field: link!, value: form()![key]!.trim() });
                        }}
                        data-tip={`All ${form()![key]!.trim()}`}
                      >
                        {label}
                      </button>
                    </Show>
                    <Show
                      when={key === "comment"}
                      fallback={
                        <input
                          type="text"
                          class={edits()[key] !== undefined ? "edited" : ""}
                          value={shown(key)}
                          spellcheck={false}
                          disabled={!form()}
                          onInput={(e) => setEdits({ ...edits(), [key]: e.currentTarget.value })}
                          onKeyDown={(e) => onFieldKey(e, false)}
                        />
                      }
                    >
                      <textarea
                        rows="1"
                        class={edits()[key] !== undefined ? "edited" : ""}
                        value={shown(key)}
                        spellcheck={false}
                        disabled={!form()}
                        onInput={(e) => setEdits({ ...edits(), [key]: e.currentTarget.value })}
                        onKeyDown={(e) => onFieldKey(e, true)}
                      />
                    </Show>
                  </div>
                );
              }}
            </For>
          </div>

          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>

          <Show when={dirty()}>
            <div class="sideapply">
              <button class="btn" onClick={cancel}>
                Cancel
              </button>
              <button class="btn primary" disabled={busy()} onClick={() => void apply()}>
                {busy() ? "Saving…" : "Apply"}
              </button>
            </div>
          </Show>

          <div class="sidesearch">
            <For each={props.sites}>
              {(site) => {
                const [broken, setBroken] = createSignal(false);
                return (
                  <button
                    class="sitebtn"
                    disabled={!form()}
                    onClick={() => openUrl(searchUrl(site.target, track())).catch(() => {})}
                    data-tip={site.name}
                    aria-label={`Search ${site.name}`}
                  >
                    <Show
                      when={!broken() && favicon(site.target)}
                      fallback={<span class="siteletter">{site.name.slice(0, 1)}</span>}
                    >
                      <img
                        src={favicon(site.target)}
                        alt=""
                        draggable={false}
                        onError={() => setBroken(true)}
                      />
                    </Show>
                  </button>
                );
              }}
            </For>
            <button
              class="sitebtn siteedit"
              onClick={props.onSites}
              data-tip="Edit sites"
              aria-label="Edit sites"
            >
              +
            </button>
          </div>
        </div>
      </Show>
    </aside>
  );
}
