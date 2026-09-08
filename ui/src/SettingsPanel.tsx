import { For, Show, createEffect, createSignal, onCleanup, type JSX } from "solid-js";
import { CloseIcon } from "./icons";
import {
  actions,
  credentials,
  openUrl,
  preserveDates,
  setActions,
  setDiscogsToken,
  setPreserveDates,
  type Action,
  type ActionKind,
} from "./api";
import type { Column } from "./columns";
import {
  NEW_QUICK_GROUP,
  QUICK_KEYS,
  TAG_COLOURS,
  valueColour,
  valuesFrom,
  type QuickGroup,
  type Settings as S,
} from "./settings";

export type ArtSize = "small" | "medium" | "large";

/**
 * The kinds of button, and what each one's two boxes are for.
 *
 * An empty `target` means the kind has nothing to point at — a rename and a
 * search are entirely their expression — and the box is not shown for it.
 */
const KINDS: {
  kind: ActionKind;
  label: string;
  target: string;
  script: string;
}[] = [
  { kind: "tag", label: "Set tag", target: "field", script: "$caps2(%artist%)" },
  { kind: "rename", label: "Rename", target: "", script: "%artist% - %title%" },
  { kind: "open", label: "Open with", target: "program", script: "\"%_path%\"" },
  {
    kind: "url",
    label: "Search",
    target: "",
    script: "https://www.google.com/search?q=%artist%+%title%",
  },
  {
    kind: "copy",
    label: "Copy to",
    target: "folder",
    script: String.raw`optional: %artist% - %album%\%filename%`,
  },
  {
    kind: "move",
    label: "Move to",
    target: "folder",
    script: String.raw`optional: %artist% - %album%\%filename%`,
  },
];

const kindOf = (kind: ActionKind) =>
  KINDS.find((k) => k.kind === kind) ?? KINDS[0];

/**
 * Width of the colour palette, in pixels.
 *
 * Only needs to be close: it decides which side of a chip the palette opens
 * from, and being a few pixels out moves that decision for one chip near the
 * boundary rather than breaking anything.
 */
const POPOVER = 150;

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
        if (picking()) setPicking(null);
        else props.onClose();
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

  /** The value whose colour is being chosen, or null when none is. */
  const [picking, setPicking] = createSignal<{
    row: number;
    at: number;
    /** Opens leftwards from the chip's right edge, for chips near the panel's. */
    flip: boolean;
  } | null>(null);

  const [buttons, setButtons] = createSignal<Action[]>([]);
  const [actionError, setActionError] = createSignal("");

  createEffect(() => {
    actions().then(setButtons).catch(() => {});
  });

  /**
   * Saves the list, keeping what is on screen either way.
   *
   * A refusal means one expression will not parse. Clearing the editor because
   * of that would throw away the work rather than the mistake, so the list stays
   * as typed and the reason is shown under it.
   */
  function saveActions(next: Action[]) {
    setButtons(next);
    setActions(next)
      .then(() => setActionError(""))
      .catch((e) => setActionError(String(e)));
  }

  const editAction = (at: number, change: Partial<Action>) =>
    saveActions(buttons().map((a, i) => (i === at ? { ...a, ...change } : a)));

  const [keepDates, setKeepDates] = createSignal(true);

  createEffect(() => {
    preserveDates().then(setKeepDates).catch(() => {});
  });

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

        {/* A click anywhere else puts the palette away, which is what anyone
            expects of something that opened over the page. */}
        <div
          class="setbody"
          onClick={(e) => {
            if (!(e.target as HTMLElement).closest(".setchipwrap")) {
              setPicking(null);
            }
          }}
        >
          <Section
            title="Waveform"
            info={
              <p>
                All four read the same three frequency bands and only differ in
                how they draw them. <b>RGB</b> blends bass, body and air into one
                colour, as Traktor and Virtual DJ do. <b>3-band</b> draws them as
                separate shapes - blue bass, orange body, pale highs - the way a
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
                  yours too - <code>ENERGY</code> and <code>MOOD</code> are only
                  defaults.
                </p>
                <p>
                  A name a format already knows goes to its proper home:{" "}
                  <code>MOOD</code> becomes ID3's <code>TMOO</code> frame.
                  Anything else becomes a user-defined field - <code>TXXX</code>{" "}
                  in MP3, WAV and AIFF, a plain key in FLAC, an iTunes atom in
                  M4A - which is where Mp3tag, OneTagger and Serato look.
                </p>
                <p>
                  Rekordbox displays only a handful of fields, so for something
                  visible on a CDJ point a row at <code>COMMENT</code> or{" "}
                  <code>GROUPING</code> rather than a name of your own. The star
                  rating is separate and always goes where Rekordbox reads it.
                </p>
                <p>
                  A field name of exactly four capitals - <code>VIBE</code> - is
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
                <span>1-5 rating</span>
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
                      <span>multiple</span>
                    </label>

                  </div>
                  {/* The whole list at once, because typing five values is far
                      quicker than adding them one at a time. Colours survive an
                      edit here by name, so reordering or inserting a value keeps
                      the rest of the row as it was. */}
                  <div class="setrow">
                    <input
                      type="text"
                      value={g.values.map((v) => v.value).join(", ")}
                      placeholder="comma separated values"
                      spellcheck={false}
                      onChange={(e) =>
                        editGroup(row(), {
                          values: valuesFrom(e.currentTarget.value, g.values),
                        })
                      }
                    />
                  </div>

                  {/* The values as they will actually look, one row of them.
                      Clicking one opens the palette under it — a chip that is
                      already the colour it sets needs no label saying so, and a
                      swatch strip per value turned eleven values into seventy
                      seven coloured dots. */}
                  <div class="setchips">
                    <For each={g.values}>
                      {(entry, at) => (
                        <span class="setchipwrap">
                          <button
                            class="setchip"
                            style={{
                              "--tag": valueColour(entry, at()).line,
                              "--tag-fill": valueColour(entry, at()).fill,
                            }}
                            data-tip="Change colour"
                            onClick={(e) => {
                              if (
                                picking()?.row === row() &&
                                picking()?.at === at()
                              ) {
                                setPicking(null);
                                return;
                              }
                              // The panel scrolls, so anything hanging off its
                              // right edge is clipped rather than merely ugly.
                              const box = e.currentTarget.getBoundingClientRect();
                              setPicking({
                                row: row(),
                                at: at(),
                                flip: box.left + POPOVER > window.innerWidth - 14,
                              });
                            }}
                          >
                            {entry.value}
                          </button>

                          <Show
                            when={
                              picking()?.row === row() && picking()?.at === at()
                            }
                          >
                            <span
                              class={`chippop ${picking()?.flip ? "flip" : ""}`}
                            >
                              <For each={TAG_COLOURS}>
                                {(c) => (
                                  <button
                                    class={`swatch ${
                                      valueColour(entry, at()).name === c.name
                                        ? "on"
                                        : ""
                                    }`}
                                    style={{ "--tag": c.line }}
                                    aria-label={c.label}
                                    onClick={() => {
                                      editGroup(row(), {
                                        values: g.values.map((v, i) =>
                                          i === at() ? { ...v, colour: c.name } : v,
                                        ),
                                      });
                                      setPicking(null);
                                    }}
                                  />
                                )}
                              </For>
                            </span>
                          </Show>
                        </span>
                      )}
                    </For>
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
                New custom tag
              </button>
            </Show>

          </Section>

          <Section
            title="Buttons"
            info={
              <>
                <p>
                  Your own actions, run on the selection from the lightning
                  button in the path bar. Each one is a kind, something to point
                  it at, and an expression evaluated once per file.
                </p>
                <p>
                  The expressions are foobar2000's, so anything you already have
                  for foobar, Mp3tag or OneTagger works as written:{" "}
                  <code>$caps2(%artist%)</code>, <code>$left(%date%,4)</code>,{" "}
                  <code>$trim(%title%)</code>. Fields are the tags plus{" "}
                  <code>%_path%</code>, <code>%filename%</code>,{" "}
                  <code>%filename_ext%</code>, <code>%directoryname%</code> and{" "}
                  <code>%ext%</code>.
                </p>
                <p>
                  <code>$replace</code> is literal and takes pairs, exactly as in
                  foobar2000. It does not take a regular expression, and one
                  written as a pattern matches nothing.
                </p>
                <p>
                  Anything that changes a file lists every result before it runs.
                  Opening a program passes your expression as its arguments and
                  never through a shell, so a track named after a command is an
                  argument and nothing else.
                </p>
              </>
            }
          >
            <For each={buttons()}>
              {(a, at) => (
                <div class="setquick">
                  <div class="setrow">
                    <input
                      type="text"
                      class="setquickname"
                      value={a.label}
                      placeholder="Name"
                      spellcheck={false}
                      onChange={(e) =>
                        editAction(at(), { label: e.currentTarget.value })
                      }
                    />
                    <select
                      class="setselect"
                      value={a.kind}
                      onChange={(e) =>
                        editAction(at(), {
                          kind: e.currentTarget.value as ActionKind,
                        })
                      }
                    >
                      <For each={KINDS}>
                        {(k) => <option value={k.kind}>{k.label}</option>}
                      </For>
                    </select>
                    <button
                      class="setremove"
                      title="Remove this button"
                      onClick={() =>
                        saveActions(buttons().filter((_, i) => i !== at()))
                      }
                    >
                      remove
                    </button>
                  </div>

                  <Show when={kindOf(a.kind).target}>
                    <div class="setrow">
                      <input
                        type="text"
                        class="mono"
                        value={a.target}
                        placeholder={kindOf(a.kind).target}
                        spellcheck={false}
                        onChange={(e) =>
                          editAction(at(), { target: e.currentTarget.value })
                        }
                      />
                    </div>
                  </Show>

                  {/* Copy and move take an optional pattern: leave it empty and
                      everything lands in the folder under the name it has; write
                      one and each file is filed apart, folders and all. */}
                  <div class="setrow">
                    <input
                      type="text"
                      class="mono"
                      value={a.script}
                      placeholder={kindOf(a.kind).script}
                      spellcheck={false}
                      onChange={(e) =>
                        editAction(at(), { script: e.currentTarget.value })
                      }
                    />
                  </div>
                </div>
              )}
            </For>

            <Show when={actionError()}>
              <p class="tagerror">{actionError()}</p>
            </Show>

            <button
              class="btn"
              onClick={() =>
                saveActions([
                  ...buttons(),
                  {
                    id: `a${Date.now()}`,
                    label: "New button",
                    kind: "tag",
                    target: "title",
                    script: "%title%",
                  },
                ])
              }
            >
              New button
            </button>
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
            </p>
          </Section>

          <Section
            title="Files"
            info={
              <p>
                Writing a tag rewrites the file, and the filesystem stamps it
                with the time it was rewritten. For a music library that is
                destructive in a quiet way: "date modified" is what you sort by
                to find what arrived last week, and retagging a folder moves
                every track in it to the top of that sort at once. The old dates
                are not recoverable, which is why this starts on.
              </p>
            }
          >
            <div class="setrow">
              <span class="setlabel">Dates</span>
              <label class="setcheck">
                <input
                  type="checkbox"
                  checked={keepDates()}
                  onChange={(e) => {
                    const want = e.currentTarget.checked;
                    setKeepDates(want);
                    setPreserveDates(want).then(setKeepDates).catch(() => {});
                  }}
                />
                <span>Preserve file modification time when retagging</span>
              </label>
            </div>
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
                Right-clicking the list header opens the same list.
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
