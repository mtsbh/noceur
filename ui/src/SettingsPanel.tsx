import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon, ICON_NAMES, TOOLBAR_ICONS, type IconName } from "./icons";
import { credentials, openUrl, setDiscogsToken } from "./api";
import type { Column } from "./columns";
import {
  ACTIONS,
  NEW_QUICK_GROUP,
  QUICK_KEYS,
  type QuickGroup,
  type Settings as S,
  type ToolbarButton,
} from "./settings";

export type ArtSize = "small" | "medium" | "large";

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

  /** Changes one toolbar button, leaving the order and the others alone. */
  const setButton = (at: number, next: Partial<ToolbarButton>) =>
    patch({
      toolbar: props.settings.toolbar.map((b, i) => (i === at ? { ...b, ...next } : b)),
    });

  /** Moves a button along the bar, which is how the order is chosen. */
  const moveButton = (at: number, by: number) => {
    const to = at + by;
    if (to < 0 || to >= props.settings.toolbar.length) return;
    const next = [...props.settings.toolbar];
    [next[at], next[to]] = [next[to], next[at]];
    patch({ toolbar: next });
  };

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
          <section class="setgroup">
            <h3>Toolbar</h3>
            <p class="sethint">
              Shortcut buttons in the path bar, for what the right-click menu
              does. Everything here stays in that menu whether or not it has a
              button, so turning one off costs nothing but the shortcut.
            </p>

            <For each={props.settings.toolbar}>
              {(button, at) => (
                <div class="toolset">
                  <label class="toolsetname">
                    <input
                      type="checkbox"
                      checked={button.on}
                      onChange={(e) => setButton(at(), { on: e.currentTarget.checked })}
                    />
                    {ACTIONS[button.id].label}
                  </label>

                  {/* Which glyph reads as "find metadata" is a matter of habit,
                      so any icon can go on any button. */}
                  <div class="iconpick" role="group" aria-label="Icon">
                    <For each={ICON_NAMES}>
                      {(name: IconName) => {
                        const Glyph = TOOLBAR_ICONS[name];
                        return (
                          <button
                            class={`iconbtn ${button.icon === name ? "on" : ""}`}
                            title={name}
                            onClick={() => setButton(at(), { icon: name })}
                          >
                            <Glyph />
                          </button>
                        );
                      }}
                    </For>
                  </div>

                  <div class="toolmove">
                    <button
                      class="segbtn"
                      title="Move left"
                      disabled={at() === 0}
                      onClick={() => moveButton(at(), -1)}
                    >
                      ←
                    </button>
                    <button
                      class="segbtn"
                      title="Move right"
                      disabled={at() === props.settings.toolbar.length - 1}
                      onClick={() => moveButton(at(), 1)}
                    >
                      →
                    </button>
                  </div>
                </div>
              )}
            </For>
          </section>

          <section class="setgroup">
            <h3>Waveform</h3>

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

            <p class="sethint">
              All four read the same three frequency bands and only differ in how
              they draw them. <b>RGB</b> blends bass, body and air into one
              colour, as Traktor and Virtual DJ do. <b>3-band</b> draws them as
              separate shapes — blue bass, orange body, pale highs — the way a
              CDJ does. <b>Blue</b> is a single colour that pales where the hats
              are. <b>Mono</b> uses the app's own accent.
            </p>
          </section>

          <section class="setgroup">
            <h3>Quick tag</h3>
            <p class="sethint">
              Rows of values you can apply to the playing track by keyboard.
              Every library has its own vocabulary, so the field names are yours
              too — <code>ENERGY</code> and <code>MOOD</code> are only defaults.
            </p>
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

            <h4 class="setsub">Where these end up</h4>
            <p class="sethint">
              A name that a format already knows goes to its proper home:
              <code>MOOD</code> becomes ID3's <code>TMOO</code> frame, and
              <code>GENRE</code> or <code>COMMENT</code> become the real genre
              and comment. Anything else becomes a user-defined field —
              <code>TXXX</code> in MP3, WAV and AIFF, a plain key in FLAC, an
              iTunes atom in M4A — which is where Mp3tag, OneTagger and Serato
              look for custom values.
            </p>
            <p class="sethint">
              Rekordbox only displays a handful of fields, so if you want energy
              visible on a CDJ, point a row at <code>COMMENT</code> or
              <code>GROUPING</code> rather than a name of your own. The star
              rating above is separate and always goes where Rekordbox reads it.
            </p>
            <p class="sethint">
              ID3 reads any four capitals as a frame identifier, so a field name
              of exactly four — <code>VIBE</code> — is refused rather than
              written as something no reader understands. Use a longer one.
            </p>
          </section>

          <section class="setgroup">
            <h3>Metadata sources</h3>
            <p class="sethint">
              Deezer, iTunes and MusicBrainz need nothing. Discogs needs a
              personal access token from your own account — it is the only
              source here that reliably knows a record's label and catalogue
              number.
            </p>
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
              , then paste it here. It is stored in plain text beside the app, and
              is only ever sent to Discogs.
            </p>
          </section>

          <section class="setgroup">
            <h3>List</h3>
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
          </section>

          <section class="setgroup">
            <h3>Columns</h3>
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
            <Show when={true}>
              <p class="sethint">
                Right-clicking the list header opens the same list. Drag a
                heading's edge to change its width.
              </p>
            </Show>
          </section>
        </div>
      </div>
    </>
  );
}
