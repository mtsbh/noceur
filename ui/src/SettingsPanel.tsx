import { For, Show, createEffect, createSignal, onCleanup, type JSX } from "solid-js";
import { CloseIcon } from "./icons";
import { credentials, openUrl, setDiscogsToken } from "./api";
import type { Column } from "./columns";
import {
  NEW_QUICK_GROUP,
  QUICK_KEYS,
  TAG_COLOURS,
  type QuickGroup,
  type Settings as S,
} from "./settings";

export type ArtSize = "small" | "medium" | "large";

/**
 * One settings section, with its explanation behind a question mark.
 *
 * The panel used to carry its prose in the open — several paragraphs per
 * section, permanently, for settings that are mostly obvious. That is text you
 * read once and then scroll past forever, and it buried the controls it was
 * meant to support. It is all still here; it is just folded away until asked
 * for, which is the difference between a reference and a wall.
 */
function Section(props: {
  title: string;
  info?: JSX.Element;
  children: JSX.Element;
}) {
  const [open, setOpen] = createSignal(false);

  return (
    <section class="setgroup">
      <h3>
        {props.title}
        <Show when={props.info}>
          <button
            class={`setinfo ${open() ? "on" : ""}`}
            aria-label={`About ${props.title.toLowerCase()}`}
            aria-expanded={open()}
            data-tip={open() ? "Hide" : `About ${props.title.toLowerCase()}`}
            onClick={() => setOpen((v) => !v)}
          >
            ?
          </button>
        </Show>
      </h3>
      <Show when={open()}>
        <div class="setabout">{props.info}</div>
      </Show>
      {props.children}
    </section>
  );
}

/**
 * Everything configurable, in one place.
 *
 * The app grew its preferences where they were first needed — cover size on the
 * toolbar, columns behind a right-click on the header, the waveform nowhere at
 * all. That is fine for the one you use constantly and bad for every other:
 * a setting you have to already know about is a setting nobody finds. They stay
 * where they are for the ones worth reaching quickly, and are all repeated here.
 */
export default function Settings(props: {
  settings: S;
  onSettings: (next: S) => void;
  artSize: ArtSize;
  onArtSize: (size: ArtSize) => void;
  columns: Column[];
  onToggleColumn: (key: string) => void;
  onClose: () => void;
}) {
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

  const patch = (next: Partial<S>) => props.onSettings({ ...props.settings, ...next });

  /** Replaces one quick-tag group, leaving the rest alone. */
  function editGroup(index: number, change: Partial<QuickGroup>) {
    patch({
      quick: props.settings.quick.map((g, i) =>
        i === index ? { ...g, ...change } : g,
      ),
    });
  }

  const [token, setToken] = createSignal("");
  const [hasDiscogs, setHasDiscogs] = createSignal(false);
  const [saving, setSaving] = createSignal(false);

  createEffect(() => {
    credentials()
      .then((c) => setHasDiscogs(c.discogs))
      .catch(() => {});
  });

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="settings" role="dialog" aria-label="Settings">
        <header class="sethead">
          <span class="settitle">Settings</span>
          <button class="setclose" onClick={props.onClose} title="Close (Esc)">
            <CloseIcon />
          </button>
        </header>

        <div class="setbody">
          <Section
            title="Waveform"
            info={
              <p>
                All four read the same three frequency bands and only differ in
                how they draw them. <b>RGB</b> blends bass, body and air into one
                colour, as Traktor and Virtual DJ do. <b>3-band</b> draws them as
                separate shapes — blue bass, orange body, pale highs — the way a
                CDJ does. <b>Blue</b> is one colour that pales where the hats
                are. <b>Mono</b> uses the app's own accent.
              </p>
            }
          >
            <div class="setrow">
              <span class="setlabel">Shape</span>
              <div class="setseg" role="group">
                <For
                  each={
                    [
                      ["filled", "Filled"],
                      ["bars", "Bars"],
                    ] as const
                  }
                >
                  {([value, label]) => (
                    <button
                      class={`segbtn ${props.settings.wave.shape === value ? "on" : ""}`}
                      onClick={() =>
                        patch({ wave: { ...props.settings.wave, shape: value } })
                      }
                    >
                      {label}
                    </button>
                  )}
                </For>
              </div>
            </div>

            <div class="setrow">
              <span class="setlabel">Colour</span>
              <div class="setseg wrap" role="group">
                <For
                  each={
                    [
                      ["rgb", "RGB"],
                      ["threeband", "3-band"],
                      ["blue", "Blue"],
                      ["accent", "Mono"],
                    ] as const
                  }
                >
                  {([value, label]) => (
                    <button
                      class={`segbtn ${props.settings.wave.colour === value ? "on" : ""}`}
                      onClick={() =>
                        patch({ wave: { ...props.settings.wave, colour: value } })
                      }
                    >
                      {label}
                    </button>
                  )}
                </For>
              </div>
            </div>

          </Section>

          <Section
            title="Quick tag"
            info={
              <>
                <p>
                  Rows of values you can apply to the playing track by keyboard.
                  Every library has its own vocabulary, so the field names are
                  yours too — <code>ENERGY</code> and <code>MOOD</code> are only
                  defaults.
                </p>
                <p>
                  A name a format already knows goes to its proper home:{" "}
                  <code>MOOD</code> becomes ID3's <code>TMOO</code> frame.
                  Anything else becomes a user-defined field — <code>TXXX</code>{" "}
                  in MP3, WAV and AIFF, a plain key in FLAC, an iTunes atom in
                  M4A — which is where Mp3tag, OneTagger and Serato look.
                </p>
                <p>
                  Rekordbox displays only a handful of fields, so for something
                  visible on a CDJ point a row at <code>COMMENT</code> or{" "}
                  <code>GROUPING</code> rather than a name of your own. The star
                  rating is separate and always goes where Rekordbox reads it.
                </p>
                <p>
                  A field name of exactly four capitals — <code>VIBE</code> — is
                  refused: ID3 reads any four capitals as a frame identifier, so
                  it would be written as something no reader understands.
                </p>
              </>
            }
          >
            <div class="setrow">
              <span class="setlabel">Stars</span>
              <label class="setcheck">
                <input
                  type="checkbox"
                  checked={props.settings.quickRating}
                  onChange={(e) => patch({ quickRating: e.currentTarget.checked })}
                />
                <span>offer a 1–5 rating row (shift+1 … shift+5)</span>
              </label>
            </div>
            <For each={props.settings.quick}>
              {(g, row) => (
                <div class="setquick">
                  <div class="setrow">
                    <input
                      type="text"
                      class="setquickname"
                      value={g.label}
                      placeholder="Label"
                      spellcheck={false}
                      onChange={(e) =>
                        editGroup(row(), { label: e.currentTarget.value })
                      }
                    />
                    <input
                      type="text"
                      class="setquickfield mono"
                      value={g.field}
                      placeholder="FIELD"
                      spellcheck={false}
                      onChange={(e) =>
                        editGroup(row(), { field: e.currentTarget.value })
                      }
                    />
                    <label class="setcheck">
                      <input
                        type="checkbox"
                        checked={g.multi}
                        onChange={(e) =>
                          editGroup(row(), { multi: e.currentTarget.checked })
                        }
                      />
                      <span>many</span>
                    </label>

                    {/* Seven swatches rather than a colour picker: these are
                        chosen to stay readable on this theme, and the point of
                        marking a row is telling it apart at a glance. */}
                    <div class="swatches" role="group" aria-label="Row colour">
                      <For each={TAG_COLOURS}>
                        {(c) => (
                          <button
                            class={`swatch ${g.colour === c.name ? "on" : ""}`}
                            style={{ "--tag": c.line }}
                            data-tip={c.label}
                            aria-label={c.label}
                            onClick={() => editGroup(row(), { colour: c.name })}
                          />
                        )}
                      </For>
                    </div>
                  </div>
                  <div class="setrow">
                    <input
                      type="text"
                      value={g.values.join(", ")}
                      placeholder="comma separated values"
                      spellcheck={false}
                      onChange={(e) =>
                        editGroup(row(), {
                          values: e.currentTarget.value
                            .split(",")
                            .map((v) => v.trim())
                            .filter(Boolean),
                        })
                      }
                    />
                  </div>
                  <p class="sethint">
                    Keys: {(QUICK_KEYS[row()] ?? []).slice(0, g.values.length).join(" ") || "—"}
                    <button
                      class="setremove"
                      title="Remove this row"
                      onClick={() =>
                        patch({
                          quick: props.settings.quick.filter((_, i) => i !== row()),
                        })
                      }
                    >
                      remove
                    </button>
                  </p>
                </div>
              )}
            </For>
            <Show when={props.settings.quick.length < QUICK_KEYS.length}>
              <button
                class="btn"
                onClick={() =>
                  patch({ quick: [...props.settings.quick, { ...NEW_QUICK_GROUP }] })
                }
              >
                Add a row
              </button>
            </Show>

          </Section>

          <Section
            title="Metadata sources"
            info={
              <p>
                Deezer, iTunes and MusicBrainz need nothing. Discogs needs a
                personal access token from your own account, and is the only one
                here that reliably knows a record's label and catalogue number.
                The token is stored in plain text beside the app and is only ever
                sent to Discogs.
              </p>
            }
          >
            <div class="setrow">
              <span class="setlabel">Discogs</span>
              <input
                type="password"
                placeholder={hasDiscogs() ? "— token saved —" : "paste token"}
                value={token()}
                spellcheck={false}
                onInput={(e) => setToken(e.currentTarget.value)}
              />
              <button
                class="btn"
                disabled={saving()}
                onClick={() => {
                  setSaving(true);
                  setDiscogsToken(token())
                    .then((c) => {
                      setHasDiscogs(c.discogs);
                      // Cleared from the box once stored: it is never read back,
                      // so leaving it on screen serves nothing but shoulders.
                      setToken("");
                      setSaving(false);
                    })
                    .catch(() => setSaving(false));
                }}
              >
                {token().trim() ? "Save" : hasDiscogs() ? "Clear" : "Save"}
              </button>
            </div>
            <p class="sethint">
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  openUrl("https://www.discogs.com/settings/developers").catch(
                    () => {},
                  );
                }}
              >
                Generate one on Discogs
              </a>
              , then paste it here.
            </p>
          </Section>

          <Section title="List">
            <div class="setrow">
              <span class="setlabel">Cover art</span>
              <div class="setseg" role="group">
                <For
                  each={
                    [
                      ["small", "Small"],
                      ["medium", "Medium"],
                      ["large", "Large"],
                    ] as const
                  }
                >
                  {([value, label]) => (
                    <button
                      class={`segbtn ${props.artSize === value ? "on" : ""}`}
                      onClick={() => props.onArtSize(value)}
                    >
                      {label}
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Section>

          <Section
            title="Columns"
            info={
              <p>
                Right-clicking the list header opens the same list. Drag a
                heading to move it, or its right edge to change its width.
              </p>
            }
          >
            <div class="setcols">
              <For each={props.columns}>
                {(col) => (
                  <label class={`setcheck ${col.key === "name" ? "off" : ""}`}>
                    <input
                      type="checkbox"
                      checked={col.visible}
                      disabled={col.key === "name"}
                      onChange={() => props.onToggleColumn(col.key)}
                    />
                    <span>{col.label}</span>
                  </label>
                )}
              </For>
            </div>
          </Section>
        </div>
      </div>
    </>
  );
}
