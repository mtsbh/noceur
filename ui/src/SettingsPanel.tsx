import { For, Show, createEffect, createMemo, createSignal, onCleanup, type JSX } from "solid-js";
import { CloseIcon, GlobeIcon, OpenWithIcon, actionIcon } from "./icons";
import {
  actions,
  credentials,
  folderIconUrl,
  openUrl,
  preserveDates,
  setActions,
  setDiscogsToken,
  setShazam,
  setTidal,
  setPreserveDates,
  ffmpegInfo,
  setFfmpegPath,
  rekordboxTarget,
  setRekordboxXml,
  analysisSettings,
  setAnalysisSettings,
  type AnalysisMode,
  type AnalysisSettings,
  type FfmpegInfo,
  type RekordboxTarget,
  type Action,
  type ActionKind,
} from "./api";
import type { Column } from "./columns";
import { CUSTOM, UI_FONTS, cleanFamily, head, installed } from "./fonts";
import { recipesFor } from "./recipes";
import ActionEditor from "./ActionEditor";
import { THEMES } from "./themes";
import { METER_MODES } from "./Meters";
import {
  NEW_QUICK_GROUP,
  QUICK_KEYS,
  TAG_COLOURS,
  valueColour,
  valuesFrom,
  newLauncherId,
  type Launcher,
  type QuickGroup,
  type Settings as S,
} from "./settings";

export type ArtSize = "small" | "medium" | "large";

/** The settings box's tabs, in the order the sidebar lists them. */
const TABS = [
  { id: "look", label: "Appearance" },
  { id: "list", label: "Library" },
  { id: "quick", label: "Tagging" },
  { id: "actions", label: "Actions" },
  { id: "files", label: "Files" },
  { id: "sources", label: "Sources" },
] as const;

export type SettingsTab = (typeof TABS)[number]["id"];

/**
 * The open tab. Outside the component, so settings reopen where you left them
 * and the rest of the app can open them on a particular tab.
 */
const [settingsTab, setSettingsTabSignal] = createSignal<SettingsTab>("look");
export const setSettingsTab = (tab: SettingsTab) => setSettingsTabSignal(tab);

/**
 * The kinds of button, and what each one's two boxes are for.
 *
 * An empty `target` means the kind has nothing to point at — a rename and a
 * search are entirely their expression — and the box is not shown for it.
 */
/** What each kind is called in the list. The editor owns everything else. */
const KIND_LABELS: Record<ActionKind, string> = {
  tag: "Sets a tag",
  rename: "Renames",
  open: "Opens in a program",
  url: "Looks it up",
  copy: "Copies",
  move: "Moves",
};

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
/** What background analysis may do to a file, as the setting offers it. */
const ANALYSIS_MODES: [AnalysisMode, string][] = [
  ["fill", "Empty fields"],
  ["show", "Show only"],
  ["overwrite", "Overwrite"],
];

/** The tempo ranges the Detect dialog offers too. */
const TEMPO_RANGES: [number, number][] = [
  [70, 180],
  [60, 120],
  [85, 175],
  [100, 200],
];

function Section(props: {
  title: string;
  tab: SettingsTab;
  info?: JSX.Element;
  children: JSX.Element;
}) {
  const [open, setOpen] = createSignal(false);

  return (
    <Show when={settingsTab() === props.tab}>
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
    </Show>
  );
}

/**
 * The list behind "Open with" or "Search in": a name, a program or an address,
 * and a switch for a toolbar button of its own.
 */
function LauncherList(props: {
  items: Launcher[];
  kind: "open" | "search";
  onChange: (next: Launcher[]) => void;
}) {
  const edit = (id: string, change: Partial<Launcher>) =>
    props.onChange(props.items.map((l) => (l.id === id ? { ...l, ...change } : l)));
  const icon = (l: Launcher) => {
    if (props.kind === "open") return l.target.trim() ? folderIconUrl(l.target.trim().replace(/^"|"$/g, "")) : "";
    try {
      return `${new URL(l.target.replace(/\{\w+\}/g, "x")).origin}/favicon.ico`;
    } catch {
      return "";
    }
  };

  return (
    <>
      <For each={props.items}>
        {(l) => (
          <div class="setrow launcher">
            <span class="launchicon">
              <LauncherIcon src={icon(l)} kind={props.kind} />
            </span>
            <input
              type="text"
              class="launchname"
              value={l.name}
              placeholder="Name"
              spellcheck={false}
              onChange={(e) => edit(l.id, { name: e.currentTarget.value })}
            />
            <input
              type="text"
              class="launchtarget mono"
              value={l.target}
              placeholder={
                props.kind === "open"
                  ? "C:\\Program Files\\…\\app.exe"
                  : "https://site.com/search?q={query}"
              }
              spellcheck={false}
              onChange={(e) => edit(l.id, { target: e.currentTarget.value.trim() })}
            />
            <label class="setcheck" data-tip="Show in toolbar" data-tip-at="end">
              <input
                type="checkbox"
                checked={l.inBar}
                onChange={(e) => edit(l.id, { inBar: e.currentTarget.checked })}
              />
              <span>Toolbar</span>
            </label>
            <Show when={props.kind === "search"}>
              <label class="setcheck" data-tip="Show in the track sidebar" data-tip-at="end">
                <input
                  type="checkbox"
                  checked={l.inSide}
                  onChange={(e) => edit(l.id, { inSide: e.currentTarget.checked })}
                />
                <span>Sidebar</span>
              </label>
            </Show>
            <button
              class="setremove"
              onClick={() => props.onChange(props.items.filter((x) => x.id !== l.id))}
            >
              remove
            </button>
          </div>
        )}
      </For>
      <button
        class="btn"
        onClick={() =>
          props.onChange([
            ...props.items,
            {
              id: newLauncherId(),
              name: "",
              target: props.kind === "search" ? "https://" : "",
              inBar: false,
              inSide: true,
            },
          ])
        }
      >
        {props.kind === "open" ? "Add program" : "Add site"}
      </button>
    </>
  );
}

function LauncherIcon(props: { src: string; kind: "open" | "search" }) {
  const [broken, setBroken] = createSignal(false);
  createEffect(() => {
    props.src;
    setBroken(!props.src);
  });
  return (
    <Show when={!broken()} fallback={props.kind === "open" ? <OpenWithIcon /> : <GlobeIcon />}>
      <img class="glyphimg" src={props.src} alt="" onError={() => setBroken(true)} />
    </Show>
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

  /** Which button is open in the editor, by position. */
  const [editingAction, setEditingAction] = createSignal<number | null>(null);

  /**
   * The offered faces this machine does not have.
   *
   * Worked out once when the panel opens rather than per render: the answer
   * cannot change while it is open, and it costs a canvas measurement per font
   * per base.
   */
  const missing = createMemo(
    () => new Set(UI_FONTS.filter((f) => !installed(head(f.stack))).map((f) => f.name)),
  );

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

  const [keepDates, setKeepDates] = createSignal(true);

  createEffect(() => {
    preserveDates().then(setKeepDates).catch(() => {});
  });

  const [analysis, setAnalysis] = createSignal<AnalysisSettings | null>(null);
  createEffect(() => {
    analysisSettings().then(setAnalysis).catch(() => {});
  });
  function patchAnalysis(change: Partial<AnalysisSettings>) {
    const now = analysis();
    if (!now) return;
    const want = { ...now, ...change };
    setAnalysis(want);
    setAnalysisSettings(want).then(setAnalysis).catch(() => setAnalysis(now));
  }

  const [ffmpeg, setFfmpeg] = createSignal<FfmpegInfo | null>(null);
  createEffect(() => {
    ffmpegInfo().then(setFfmpeg).catch(() => {});
  });

  const [rekordbox, setRekordbox] = createSignal<RekordboxTarget | null>(null);
  createEffect(() => {
    rekordboxTarget().then(setRekordbox).catch(() => {});
  });

  const [token, setToken] = createSignal("");
  const [hasDiscogs, setHasDiscogs] = createSignal(false);
  const [saving, setSaving] = createSignal(false);
  const [tidalId, setTidalId] = createSignal("");
  const [tidalSecret, setTidalSecret] = createSignal("");
  const [hasTidal, setHasTidal] = createSignal(false);
  const [shazamOn, setShazamOn] = createSignal(false);

  createEffect(() => {
    credentials()
      .then((c) => {
        setHasDiscogs(c.discogs);
        setHasTidal(c.tidal);
        setShazamOn(c.shazam);
      })
      .catch(() => {});
  });

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="settings" role="dialog" aria-label="Settings">
        <header class="sethead">
          <span class="settitle">Settings</span>
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

        {/* A click anywhere else puts the palette away, which is what anyone
            expects of something that opened over the page. */}
        <div class="setframe">
        <nav class="setnav" aria-label="Settings sections">
          <For each={TABS}>
            {(t) => (
              <button
                class={`setnavbtn ${settingsTab() === t.id ? "on" : ""}`}
                onClick={() => setSettingsTabSignal(t.id)}
              >
                {t.label}
              </button>
            )}
          </For>
        </nav>
        <div
          class="setbody"
          onClick={(e) => {
            if (!(e.target as HTMLElement).closest(".setchipwrap")) {
              setPicking(null);
            }
          }}
        >
          <Section
            tab="look"
            title="Appearance"
            info={<p>Click the meter to cycle modes. It shows file level, not volume.</p>}
          >
            <div class="setrow">
              <span class="setlabel">Theme</span>
              <div class="themes" role="group">
                <For each={THEMES}>
                  {(t) => (
                    <button
                      class={`themebtn ${props.settings.theme === t.name ? "on" : ""}`}
                      onClick={() => patch({ theme: t.name })}
                      aria-pressed={props.settings.theme === t.name}
                    >
                      <span class="themesw" style={{ background: t.swatch[0] }}>
                        <span style={{ background: t.swatch[1] }} />
                        <span style={{ background: t.swatch[2] }} />
                        <span style={{ background: t.swatch[3] }} />
                      </span>
                      {t.label}
                    </button>
                  )}
                </For>
              </div>
            </div>

            <div class="setrow">
              <span class="setlabel">Meter</span>
              <div class="setseg wrap" role="group">
                <For each={[{ value: "off", short: "Off" } as const, ...METER_MODES]}>
                  {(m) => (
                    <button
                      class={`segbtn ${props.settings.meter === m.value ? "on" : ""}`}
                      onClick={() => patch({ meter: m.value })}
                    >
                      {m.short}
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Section>

          <Section
            tab="look"
            title="Waveform"
            info={<p>RGB blends bands · 3-band splits them</p>}
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
            tab="quick"
            title="Quick tag"
            info={<p>Keys tag the playing track. Use COMMENT or GROUPING to see it in Rekordbox.</p>}
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
                      data-tip="Remove"
                      data-tip-at="end"
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
            tab="actions"
            title="Buttons"
          >
            {/* A line each. The builder is a dialog rather than a block per
                button: inline, four boxes times eight buttons turned this panel
                into a wall you had to scroll past to reach anything else. */}
            <For each={buttons()}>
              {(a, at) => (
                <div class="actrow" onClick={() => setEditingAction(at())}>
                  <Show when={actionIcon(a.icon)}>
                    {(icon) => <span class="acticon">{icon()()}</span>}
                  </Show>
                  <span class="actname">{a.label || "Unnamed"}</span>
                  <span class="actkind">{KIND_LABELS[a.kind]}</span>
                  <button
                    class="setremove"
                    data-tip="Remove"
                    data-tip-at="end"
                    onClick={(e) => {
                      e.stopPropagation();
                      setEditingAction(null);
                      saveActions(buttons().filter((_, i) => i !== at()));
                    }}
                  >
                    remove
                  </button>
                </div>
              )}
            </For>

            <Show when={actionError()}>
              <p class="tagerror">{actionError()}</p>
            </Show>

            <button
              class="btn"
              onClick={() => {
                const first = recipesFor("tag")[0];
                saveActions([
                  ...buttons(),
                  {
                    id: `a${Date.now()}`,
                    label: "New button",
                    kind: "tag",
                    target: "title",
                    script: first.build("title", []),
                    recipe: first.id,
                    slots: [],
                  },
                ]);
                // Opens straight into the editor: a button named "New button"
                // that does nothing yet is not a thing anybody wanted to make.
                setEditingAction(buttons().length);
              }}
            >
              New button
            </button>
          </Section>

          <Section tab="actions" title="Open with">
            <LauncherList
              kind="open"
              items={props.settings.openWith}
              onChange={(openWith) => patch({ openWith })}
            />
          </Section>

          <Section tab="actions" title="Search in">
            <p class="sethint">
              Placeholders: <code>{"{artist}"}</code> <code>{"{title}"}</code>{" "}
              <code>{"{album}"}</code> <code>{"{query}"}</code>
            </p>
            <LauncherList
              kind="search"
              items={props.settings.searchIn}
              onChange={(searchIn) => patch({ searchIn })}
            />
          </Section>

          <Section
            tab="sources"
            title="Metadata sources"
            info={
              <>
                <p>Discogs and Tidal need your own keys. Each is sent only to its service.</p>
                <p>
                  Shazam hears twelve seconds of the track, for what AcoustID
                  doesn't know. Unofficial: it can stop working without notice.
                </p>
              </>
            }
          >
            <div class="setrow">
              <span class="setlabel">Shazam</span>
              <label class="setcheck">
                <input
                  type="checkbox"
                  checked={shazamOn()}
                  onChange={(e) => {
                    const want = e.currentTarget.checked;
                    setShazamOn(want);
                    setShazam(want)
                      .then((c) => setShazamOn(c.shazam))
                      .catch(() => setShazamOn(!want));
                  }}
                />
                <span>Identify by sound when AcoustID can't</span>
              </label>
            </div>
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
            <div class="setrow">
              <span class="setlabel">Tidal ID</span>
              <input
                type="text"
                placeholder={hasTidal() ? "— saved —" : "client ID"}
                value={tidalId()}
                spellcheck={false}
                onInput={(e) => setTidalId(e.currentTarget.value)}
              />
            </div>
            <div class="setrow">
              <span class="setlabel">Tidal secret</span>
              <input
                type="password"
                placeholder={hasTidal() ? "— saved —" : "client secret"}
                value={tidalSecret()}
                spellcheck={false}
                onInput={(e) => setTidalSecret(e.currentTarget.value)}
              />
              <button
                class="btn"
                // Both halves or neither: one alone cannot sign in.
                disabled={
                  saving() || (!tidalId().trim() !== !tidalSecret().trim())
                }
                onClick={() => {
                  setSaving(true);
                  setTidal(tidalId(), tidalSecret())
                    .then((c) => {
                      setHasTidal(c.tidal);
                      setTidalId("");
                      setTidalSecret("");
                      setSaving(false);
                    })
                    .catch(() => setSaving(false));
                }}
              >
                {tidalId().trim() || !hasTidal() ? "Save" : "Clear"}
              </button>
            </div>
            <p class="sethint">
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  openUrl("https://developer.tidal.com/dashboard").catch(() => {});
                }}
              >
                Create an app on Tidal
              </a>
            </p>
          </Section>

          <Section
            tab="files"
            title="Files"
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
                <span>Preserve modified date when tagging</span>
              </label>
            </div>
            <div class="setrow">
              <span class="setlabel">Show</span>
              <label class="setcheck">
                <input
                  type="checkbox"
                  checked={props.settings.tracksOnly}
                  onChange={(e) => patch({ tracksOnly: e.currentTarget.checked })}
                />
                <span>Hide non-audio files</span>
              </label>
            </div>
          </Section>

          <Section
            tab="files"
            title="BPM & key"
            info={
              <p>
                Measures each track you play, a second or two after it starts, and nothing
                else. The list shows what was measured beside the tag; click the dot in a
                cell to write it.
              </p>
            }
          >
            <Show when={analysis()}>
              {(a) => (
                <>
                  <div class="setrow">
                    <span class="setlabel">Auto</span>
                    <label class="setcheck">
                      <input
                        type="checkbox"
                        checked={a().enabled}
                        onChange={(e) => patchAnalysis({ enabled: e.currentTarget.checked })}
                      />
                      <span>Add BPM &amp; key automatically to tracks I play</span>
                    </label>
                  </div>
                  <div class="setrow">
                    <span class="setlabel">Write</span>
                    <div class="setseg" role="group" aria-label="What to write">
                      <For each={ANALYSIS_MODES}>
                        {([mode, label]) => (
                          <button
                            class={`segbtn ${a().mode === mode ? "on" : ""}`}
                            disabled={!a().enabled}
                            onClick={() => patchAnalysis({ mode })}
                          >
                            {label}
                          </button>
                        )}
                      </For>
                    </div>
                  </div>
                  <div class="setrow">
                    <span class="setlabel">Range</span>
                    <div class="setseg" role="group" aria-label="BPM range">
                      <For each={TEMPO_RANGES}>
                        {([min, max]) => (
                          <button
                            class={`segbtn ${a().min === min && a().max === max ? "on" : ""}`}
                            disabled={!a().enabled}
                            onClick={() => patchAnalysis({ min, max })}
                          >
                            {min}–{max}
                          </button>
                        )}
                      </For>
                    </div>
                  </div>
                </>
              )}
            </Show>
          </Section>

          <Section
            tab="files"
            title="Converter"
            info={<p>Needs ffmpeg: set a path, or put ffmpeg.exe next to Noceur.</p>}
          >
            <div class="setrow">
              <span class="setlabel">ffmpeg</span>
              <input
                type="text"
                value={ffmpeg()?.configured ?? ""}
                placeholder="Found automatically"
                spellcheck={false}
                onChange={(e) =>
                  setFfmpegPath(e.currentTarget.value).then(setFfmpeg).catch(() => {})
                }
              />
            </div>
            <Show when={ffmpeg()}>
              {(f) => (
                <p class="acthint">
                  <Show
                    when={f().path}
                    fallback={
                      <>
                        Not found.{" "}
                        <a
                          class="setlink"
                          onClick={() =>
                            openUrl("https://www.gyan.dev/ffmpeg/builds/").catch(() => {})
                          }
                        >
                          Get a Windows build
                        </a>{" "}
                        and put ffmpeg.exe beside Noceur.
                      </>
                    }
                  >
                    {f().version || "ffmpeg"} · {f().path}
                  </Show>
                </p>
              )}
            </Show>
          </Section>

          <Section
            tab="files"
            title="rekordbox"
            info={<p>Playlists → Send to rekordbox writes here, into a noceur folder. Backed up first.</p>}
          >
            <div class="setrow">
              <span class="setlabel">rekordbox.xml</span>
              <input
                type="text"
                value={rekordbox()?.set ?? ""}
                placeholder={rekordbox()?.found ?? "Not set in rekordbox"}
                spellcheck={false}
                onChange={(e) =>
                  setRekordboxXml(e.currentTarget.value).then(setRekordbox).catch(() => {})
                }
              />
            </div>
            <p class="acthint">
              <Show
                when={rekordbox()?.path}
                fallback="Set Imported Library in rekordbox (Preferences → Advanced), or a path here."
              >
                {rekordbox()?.set ? "Your path" : "From rekordbox's Imported Library"}
              </Show>
            </p>
          </Section>

          <Section
            tab="look"
            title="Text"
            info={<p>Other: type any installed font's name</p>}
          >
            <div class="setrow">
              <span class="setlabel">Font</span>
              {/* Each choice is drawn in the face it selects, which is the only
                  honest way to show a font and saves a preview row. */}
              <div class="setseg wrap" role="group" aria-label="Interface font">
                <For each={UI_FONTS}>
                  {(font) => (
                    <button
                      class={`segbtn ${props.settings.font === font.name ? "on" : ""} ${
                        missing().has(font.name) ? "miss" : ""
                      }`}
                      style={{ "font-family": font.stack }}
                      data-tip={
                        missing().has(font.name) ? "Not installed" : undefined
                      }
                      onClick={() => patch({ font: font.name })}
                    >
                      {font.label}
                    </button>
                  )}
                </For>
                <button
                  class={`segbtn ${props.settings.font === CUSTOM ? "on" : ""}`}
                  onClick={() => patch({ font: CUSTOM })}
                >
                  Other…
                </button>
              </div>
            </div>
            <Show when={props.settings.font === CUSTOM}>
              <div class="setrow">
                <span class="setlabel">Family</span>
                {/* Committed on blur or Enter rather than per keystroke: typing
                    "Consolas" one letter at a time would restyle the window
                    eight times and land on whatever "Conso" resolves to. */}
                <input
                  type="text"
                  value={props.settings.fontCustom}
                  placeholder="Exact name, as Windows spells it"
                  onChange={(e) => patch({ fontCustom: cleanFamily(e.currentTarget.value) })}
                />
              </div>
              <Show
                when={
                  props.settings.fontCustom.trim() !== "" &&
                  !installed(props.settings.fontCustom)
                }
              >
                <p class="sethint">
                  Not installed · using Segoe UI
                </p>
              </Show>
            </Show>
          </Section>

          <Section
            tab="list"
            title="List"
          >
            <div class="setrow">
              <span class="setlabel">Folders</span>
              <label class="setcheck">
                <input
                  type="checkbox"
                  checked={props.settings.folderGrid}
                  onChange={(e) => patch({ folderGrid: e.currentTarget.checked })}
                />
                <span>Folders as icons</span>
              </label>
            </div>
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
            tab="list"
            title="Columns"
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
      </div>

      <Show when={editingAction() !== null && buttons()[editingAction()!]}>
        <ActionEditor
          action={buttons()[editingAction()!]}
          onChange={(next) =>
            saveActions(buttons().map((a, i) => (i === editingAction() ? next : a)))
          }
          onClose={() => setEditingAction(null)}
        />
      </Show>
    </>
  );
}
