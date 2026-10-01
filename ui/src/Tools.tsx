import { For, Match, Show, Switch, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import FolderMover from "./FolderMover";
import {
  applyCleanup,
  previewTool,
  type CleanApply,
  type CleanPreview,
  type ToolId,
  type WriteReport,
} from "./api";

/** Which half of the list an action sits in. */
type Group = "name" | "tags";

/** One action, with the line that says what it does. Ticked ones run in this order. */
type Entry = { id: ToolId; label: string; hint: string; tab: Group };

const ENTRIES: Entry[] = [
  { id: "cut-start", label: "Cut characters", hint: "From the start or the end of the name.", tab: "name" },
  { id: "clean-tags", label: "Clean", hint: "Underscores, site names and doubled spaces out of title, artist and album.", tab: "tags" },
  { id: "remove-original", label: "Remove (Original Mix)", hint: "From title and album. Every other mix stays.", tab: "tags" },
  { id: "trailing-brackets", label: "Remove brackets at end", hint: "Track (Extended Mix) → Track. Only the last ( ) or [ ].", tab: "tags" },
  { id: "tidy-feat", label: "Tidy featuring", hint: "ft, Ft. and featuring become feat.", tab: "tags" },
  { id: "artist-out-of-title", label: "Artist out of title", hint: "Title Artist - Track → artist Artist, title Track.", tab: "tags" },
  { id: "artist-into-title", label: "Artist into title", hint: "Title Track → Artist - Track.", tab: "tags" },
  { id: "album-artist", label: "Album artist from artist", hint: "Only where album artist is empty.", tab: "tags" },
  { id: "year-only", label: "Year only", hint: "2016-03-04 → 2016.", tab: "tags" },
];

const GROUPS: [Group, string][] = [
  ["name", "File name"],
  ["tags", "Tags"],
];

const TICKS_KEY = "noceur.actionTicks";
const CUT_KEY = "noceur.toolCut";

function load<T>(key: string, fallback: T): T {
  try {
    return JSON.parse(localStorage.getItem(key) ?? "") ?? fallback;
  } catch {
    return fallback;
  }
}

/**
 * Actions in one tick list, name first, then tags.
 *
 * Tick any number; they run in list order, each on what the one before left,
 * and the preview shows the combined result old → new. Only files with
 * something to change are written. Moving into folders has a box and preview
 * of its own, so it is a second view rather than a tick.
 */
export default function Tools(props: {
  fileIds: number[];
  onClose: () => void;
  onDone: () => void;
}) {
  const cut = load<{ count?: number; end?: boolean }>(CUT_KEY, {});
  const [moving, setMoving] = createSignal(false);
  const [ticks, setTicks] = createSignal<ToolId[]>(load<ToolId[]>(TICKS_KEY, ["remove-original"]));
  const [fromEnd, setFromEnd] = createSignal<boolean>(Boolean(cut.end));
  const [count, setCount] = createSignal<number>(Number(cut.count) || 3);
  const [rows, setRows] = createSignal<CleanPreview[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [report, setReport] = createSignal<WriteReport | null>(null);

  const ticked = (id: ToolId) => ticks().includes(id);
  /** The ticked actions, in list order, as the back end names them. */
  const chosen = (): ToolId[] =>
    ENTRIES.filter((e) => ticked(e.id)).map((e) =>
      e.id === "cut-start" && fromEnd() ? "cut-end" : e.id,
    );
  /** Whether only the name changes, which Ctrl+Z can put back. */
  const renamesOnly = () => ENTRIES.every((e) => e.tab === "name" || !ticked(e.id));

  function toggle(id: ToolId) {
    const next = ticked(id) ? ticks().filter((t) => t !== id) : [...ticks(), id];
    setTicks(next);
    localStorage.setItem(TICKS_KEY, JSON.stringify(next));
  }

  createEffect(() => {
    if (moving()) return;
    const [tools, n, ids] = [chosen(), count(), props.fileIds] as const;
    setError("");
    setReport(null);
    if (tools.length === 0) {
      setRows([]);
      return;
    }
    setRows(null);
    previewTool(tools, n, ids)
      .then(setRows)
      .catch((e) => {
        setRows([]);
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

  const willChange = () => (rows() ?? []).filter((r) => !r.unchanged && !r.problem);
  const problems = () => (rows() ?? []).filter((r) => r.problem);

  function apply() {
    const targets = willChange();
    if (targets.length === 0 || busy()) return;
    if (ticked("cut-start")) {
      localStorage.setItem(CUT_KEY, JSON.stringify({ count: count(), end: fromEnd() }));
    }
    setBusy(true);
    setError("");

    const items: CleanApply[] = targets.map((r) => ({
      fileId: r.fileId,
      name: r.to === r.from ? "" : r.to,
      fields: r.fields.map((f) => [f.field, f.to] as [string, string]),
    }));
    applyCleanup(items)
      .then((result) => {
        setBusy(false);
        setReport(result);
        props.onDone();
        if (result.failures.length === 0) props.onClose();
      })
      .catch((e) => {
        setBusy(false);
        setError(String(e));
      });
  }

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer" role="dialog" aria-label="Actions">
        <header class="sethead">
          <span class="settitle">Actions</span>
          <span class="tagsubject">
            {props.fileIds.length} {props.fileIds.length === 1 ? "track" : "tracks"}
          </span>
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

        <Switch>
          <Match when={moving()}>
            <FolderMover
              fileIds={props.fileIds}
              onBack={() => setMoving(false)}
              onClose={props.onClose}
              onDone={props.onDone}
            />
          </Match>
          <Match when={true}>
            <div class="renamebody">
              <div class="rulelist">
                <For each={GROUPS}>
                  {([group, title]) => (
                    <>
                      <p class="rulegroup">{title}</p>
                      <For each={ENTRIES.filter((e) => e.tab === group)}>
                        {(e) => (
                          <label class="rule">
                            <input
                              type="checkbox"
                              checked={ticked(e.id)}
                              onChange={() => toggle(e.id)}
                            />
                            <span class="rulelabel">{e.label}</span>
                            <span class="rulehint">{e.hint}</span>
                          </label>
                        )}
                      </For>
                    </>
                  )}
                </For>
                <button class="actlink rulemove" onClick={() => setMoving(true)}>
                  Move into folders…
                </button>
              </div>

              <Show when={ticked("cut-start")}>
                <div class="toolopts">
                  <div class="setseg" role="group" aria-label="Cut from">
                    <button
                      class={`segbtn ${fromEnd() ? "" : "on"}`}
                      onClick={() => setFromEnd(false)}
                    >
                      First
                    </button>
                    <button
                      class={`segbtn ${fromEnd() ? "on" : ""}`}
                      onClick={() => setFromEnd(true)}
                    >
                      Last
                    </button>
                  </div>
                  <input
                    type="number"
                    class="toolcount"
                    min="1"
                    max="200"
                    value={count()}
                    aria-label="Characters"
                    onInput={(ev) => setCount(Math.max(0, Number(ev.currentTarget.value) || 0))}
                    onKeyDown={(ev) => {
                      if (ev.key === "Enter") apply();
                    }}
                  />
                  <span class="sethint">characters</span>
                </div>
              </Show>

              <Show
                when={chosen().length > 0}
                fallback={<p class="sethint">Tick an action to preview</p>}
              >
                <Show when={rows()} fallback={<p class="sethint">Reading…</p>}>
                  <Show
                    when={willChange().length > 0 || problems().length > 0}
                    fallback={<p class="sethint">Nothing to change</p>}
                  >
                    <div class="renamelist">
                      <For each={rows()!.filter((r) => !r.unchanged || r.problem)}>
                        {(r) => (
                          <div class={`cleanrow ${r.problem ? "bad" : ""}`}>
                            <Show
                              when={r.to !== r.from || r.problem}
                              fallback={<div class="renamerow toolfile">{r.from}</div>}
                            >
                              <div class="renamerow">
                                <span class="renamefrom">{r.from}</span>
                                <span class="renamearrow">→</span>
                                <span class="renameto">{r.problem || r.to}</span>
                              </div>
                            </Show>
                            <For each={r.fields}>
                              {(f) => (
                                <div class="renamerow tagchange">
                                  <span class="renamefrom">
                                    <span class="fieldname">{f.label}</span> {f.from || "—"}
                                  </span>
                                  <span class="renamearrow">→</span>
                                  <span class="renameto">{f.to}</span>
                                </div>
                              )}
                            </For>
                          </div>
                        )}
                      </For>
                    </div>
                  </Show>
                </Show>
              </Show>

              <Show when={error()}>
                <p class="tagerror">{error()}</p>
              </Show>

              <Show when={report() && report()!.failures.length > 0}>
                <div class="tagfailures">
                  <p>
                    Done {report()!.written}, failed {report()!.failures.length}:
                  </p>
                  <For each={report()!.failures}>
                    {(f) => (
                      <p class="tagfail">
                        <b>{f.name}</b> — {f.reason}
                      </p>
                    )}
                  </For>
                </div>
              </Show>
            </div>

            <footer class="tagfoot">
              <span class="sethint credit">
                <Show when={problems().length > 0}>{problems().length} skipped · </Show>
                <Show when={renamesOnly()} fallback="Tag edits cannot be undone">
                  Undo: Ctrl+Z
                </Show>
              </span>
              <button class="btn" onClick={props.onClose}>
                Cancel
              </button>
              <button
                class="btn primary"
                disabled={willChange().length === 0 || busy()}
                onClick={apply}
              >
                {busy() ? "Writing…" : `Apply to ${willChange().length}`}
              </button>
            </footer>
          </Match>
        </Switch>
      </div>
    </>
  );
}
