import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { credentials, openUrl, setDiscogsToken } from "./api";
import type { Column } from "./columns";
import {
  NEW_QUICK_GROUP,
  QUICK_KEYS,
  type QuickGroup,
  type Settings as S,
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
              Same three frequency bands in all four; only the drawing differs.
            </p>
          </section>

          <section class="setgroup">
            <h3>Quick tag</h3>
            <p class="sethint">
              Values you can apply to the playing track by keyboard. The field
              names are yours — <code>ENERGY</code> and <code>MOOD</code> are
              only defaults.
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
              Known names go to their proper home — <code>MOOD</code> to ID3's
              <code>TMOO</code>. Anything else becomes a user-defined field,
              where Mp3tag, OneTagger and Serato look. For a value a CDJ will
              show, point a row at <code>COMMENT</code> or{" "}
              <code>GROUPING</code>. Names of exactly four capitals are refused:
              ID3 would read them as a frame id.            </p>
          </section>

          <section class="setgroup">
            <h3>Metadata sources</h3>
            <p class="sethint">
              Deezer, iTunes and MusicBrainz need nothing. Discogs needs a token,
              and is the only one that reliably knows label and catalogue number.
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
              , then paste it here. Stored in plain text beside the app, and
              only ever sent to Discogs.
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
            <p class="sethint">
              Right-click the list header for the same list. Drag a heading to
              reorder, or its edge to resize.
            </p>
          </section>
        </div>
      </div>
    </>
  );
}
