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

export type Tab = "name" | "tags" | "move";

/** A built-in tool, and the tab it lives on. */
export type ToolEntry = { id: ToolId; label: string; tab: Tab };

/** The tools, in the order each tab and the Tools menu list them. */
export const TOOLS: ToolEntry[] = [
  { id: "clean-name", label: "Clean file name", tab: "name" },
  { id: "cut-start", label: "Cut file name", tab: "name" },
  { id: "clean-tags", label: "Clean tags", tab: "tags" },
  { id: "remove-original", label: "Remove (Original Mix)", tab: "tags" },
  { id: "trailing-brackets", label: "Remove brackets at end", tab: "tags" },
  { id: "tidy-feat", label: "Tidy featuring", tab: "tags" },
  { id: "album-artist", label: "Album artist from artist", tab: "tags" },
  { id: "artist-into-title", label: "Artist into title", tab: "tags" },
  { id: "artist-out-of-title", label: "Artist out of title", tab: "tags" },
  { id: "year-only", label: "Year only", tab: "tags" },
];

const TABS: [Tab, string][] = [
  ["name", "Name"],
  ["tags", "Tags"],
  ["move", "Move"],
];

/** What a tool is called on its own tab, where the tab already says what it acts on. */
const SHORT: Partial<Record<ToolId, string>> = {
  "clean-name": "Clean",
  "cut-start": "Cut",
  "clean-tags": "Clean",
};

const CUT_KEY = "noceur.toolCut";
const LAST_KEY = "noceur.toolLast";

/**
 * The tools, on tabs by what they act on: the file name, the tags, the folder.
 *
 * One tool at a time, previewed old → new before anything is written; only the
 * files with something to change are touched.
 */
export default function Tools(props: {
  /** The tool to open on, or a tab to open on its last-used tool. */
  start: ToolId | Tab;
  fileIds: number[];
  onClose: () => void;
  onDone: () => void;
}) {
  const saved = (() => {
    try {
      return JSON.parse(localStorage.getItem(CUT_KEY) ?? "{}");
    } catch {
      return {};
    }
  })();

  const first = (): { tab: Tab; tool: ToolId } => {
    const s = props.start;
    if (s === "move") return { tab: "move", tool: "clean-name" };
    if (s === "name" || s === "tags") {
      const last = localStorage.getItem(`${LAST_KEY}.${s}`) as ToolId | null;
      const tool = TOOLS.find((t) => t.id === last && t.tab === s) ?? TOOLS.find((t) => t.tab === s)!;
      return { tab: s, tool: tool.id };
    }
    return { tab: TOOLS.find((t) => t.id === s)?.tab ?? "name", tool: s };
  };

  const [tab, setTab] = createSignal<Tab>(first().tab);
  const [tool, setTool] = createSignal<ToolId>(first().tool);
  const [fromEnd, setFromEnd] = createSignal<boolean>(Boolean(saved.end));
  const [count, setCount] = createSignal<number>(Number(saved.count) || 3);
  const [rows, setRows] = createSignal<CleanPreview[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [report, setReport] = createSignal<WriteReport | null>(null);

  const cutting = () => tool() === "cut-start";
  /** The tool as the back end knows it: cut is one entry here, two there. */
  const wire = (): ToolId => (cutting() && fromEnd() ? "cut-end" : tool());
  const renames = () => tab() === "name";

  function chooseTab(next: Tab) {
    setTab(next);
    if (next === "move") return;
    if (TOOLS.find((t) => t.id === tool())?.tab === next) return;
    const last = localStorage.getItem(`${LAST_KEY}.${next}`) as ToolId | null;
    setTool((TOOLS.find((t) => t.id === last && t.tab === next) ?? TOOLS.find((t) => t.tab === next)!).id);
  }

  createEffect(() => {
    if (tab() === "move") return;
    const [t, n, ids] = [wire(), count(), props.fileIds];
    setRows(null);
    setError("");
    setReport(null);
    previewTool(t, n, ids)
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
    localStorage.setItem(`${LAST_KEY}.${tab()}`, tool());
    if (cutting()) {
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

        <div class="tooltabs" role="tablist">
          <For each={TABS}>
            {([id, label]) => (
              <button
                role="tab"
                class={`tooltab ${tab() === id ? "on" : ""}`}
                aria-selected={tab() === id}
                onClick={() => chooseTab(id)}
              >
                {label}
              </button>
            )}
          </For>
        </div>

        <Switch>
          <Match when={tab() === "move"}>
            <FolderMover
              fileIds={props.fileIds}
              onClose={props.onClose}
              onDone={props.onDone}
            />
          </Match>
          <Match when={true}>
            <div class="renamebody">
              <div class="toolpick" role="group" aria-label="Tool">
                <For each={TOOLS.filter((t) => t.tab === tab())}>
                  {(t) => (
                    <button
                      class={`chip ${tool() === t.id ? "on" : ""}`}
                      onClick={() => setTool(t.id)}
                    >
                      {SHORT[t.id] ?? t.label}
                    </button>
                  )}
                </For>
              </div>

              <Show when={cutting()}>
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
                    onInput={(e) => setCount(Math.max(0, Number(e.currentTarget.value) || 0))}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") apply();
                    }}
                  />
                  <span class="sethint">characters</span>
                </div>
              </Show>

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
                            when={renames() || r.problem}
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
                <Show when={renames()} fallback="Tag edits cannot be undone">
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
