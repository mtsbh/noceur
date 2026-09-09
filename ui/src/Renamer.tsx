import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import {
  RENAME_FIELDS,
  applyRename,
  previewRename,
  type RenamePreview,
  type WriteReport,
} from "./api";

/** Templates worth starting from. The first is what most libraries already use. */
const PRESETS: [string, string][] = [
  ["%artist% - %title%", "Artist - Title"],
  ["%track2% - %artist% - %title%", "01 - Artist - Title"],
  ["%artist% - %title% (%bpm% %key%)", "With BPM and key"],
  ["%artist% - %title% [%label%]", "With label"],
];

const KEY = "noceur.renameTemplate";

/**
 * Renaming files from a template.
 *
 * Every name is shown before anything moves, and the names that are applied are
 * the ones that were shown rather than recomputed — renaming is the one bulk
 * operation with no real undo inside the app, so there is no version of this
 * that acts first and reports afterwards.
 *
 * The renames themselves go through the shell, which does mean Windows' own
 * undo: a template that turned out wrong is Ctrl+Z in Explorer.
 */
export default function Renamer(props: {
  fileIds: number[];
  onClose: () => void;
  onRenamed: () => void;
}) {
  const [template, setTemplate] = createSignal(
    localStorage.getItem(KEY) ?? PRESETS[0][0],
  );
  const [rows, setRows] = createSignal<RenamePreview[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [report, setReport] = createSignal<WriteReport | null>(null);

  // Re-previewed on every keystroke. It reads tags off disk, which for a
  // selection of a few dozen is fast enough to feel live, and seeing the names
  // change as the template is typed is the entire point of having a preview.
  createEffect(() => {
    const t = template();
    const ids = props.fileIds;
    previewRename(ids, t)
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

  /** The rows that will actually be renamed. */
  const willRename = () =>
    (rows() ?? []).filter((r) => !r.unchanged && !r.problem && r.to);

  const problems = () => (rows() ?? []).filter((r) => r.problem);

  function apply() {
    const targets = willRename();
    if (targets.length === 0 || busy()) return;
    setBusy(true);
    setError("");
    localStorage.setItem(KEY, template());

    applyRename(targets.map((r) => [r.fileId, r.to] as [number, string]))
      .then((r) => {
        setBusy(false);
        setReport(r);
        props.onRenamed();
        if (r.failures.length === 0) props.onClose();
      })
      .catch((e) => {
        setBusy(false);
        setError(String(e));
      });
  }

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer" role="dialog" aria-label="Rename">
        <header class="sethead">
          <span class="settitle">Rename</span>
          <span class="tagsubject">
            {props.fileIds.length} {props.fileIds.length === 1 ? "track" : "tracks"}
          </span>
          <button
            class="setclose"
            onClick={props.onClose}
            data-tip="Close · esc"
            data-tip-at="end"
            aria-label="Close"
          >
            <CloseIcon />
          </button>
        </header>

        <div class="renamebody">
          <input
            type="text"
            class="renameinput mono"
            value={template()}
            spellcheck={false}
            placeholder="%artist% - %title%"
            onInput={(e) => setTemplate(e.currentTarget.value)}
          />

          <div class="setseg wrap renamepresets" role="group">
            <For each={PRESETS}>
              {([value, label]) => (
                <button
                  class={`segbtn ${template() === value ? "on" : ""}`}
                  onClick={() => setTemplate(value)}
                >
                  {label}
                </button>
              )}
            </For>
          </div>

          <p class="sethint">
            <For each={RENAME_FIELDS}>
              {([name, label], i) => (
                <>
                  {i() > 0 ? " · " : ""}
                  <button
                    class="fieldchip mono"
                    title={label}
                    onClick={() => setTemplate(template() + `%${name}%`)}
                  >
                    %{name}%
                  </button>
                </>
              )}
            </For>
          </p>

          <Show when={rows()} fallback={<p class="sethint">Reading…</p>}>
            <div class="renamelist">
              <For each={rows()}>
                {(r) => (
                  <div
                    class={`renamerow ${r.problem ? "bad" : ""} ${
                      r.unchanged ? "same" : ""
                    }`}
                  >
                    <span class="renamefrom">{r.from}</span>
                    <span class="renamearrow">→</span>
                    <span class="renameto">
                      {r.problem ? r.problem : r.unchanged ? "unchanged" : r.to}
                    </span>
                  </div>
                )}
              </For>
            </div>
          </Show>

          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>

          <Show when={report() && report()!.failures.length > 0}>
            <div class="tagfailures">
              <p>
                Renamed {report()!.written}, failed {report()!.failures.length}:
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
            <Show when={problems().length > 0}>
              {problems().length} skipped ·{" "}
            </Show>
            Undo in Explorer with Ctrl+Z.
          </span>
          <button class="btn" onClick={props.onClose}>
            Cancel
          </button>
          <button
            class="btn primary"
            disabled={willRename().length === 0 || busy()}
            onClick={apply}
          >
            {busy() ? "Renaming…" : `Rename ${willRename().length}`}
          </button>
        </footer>
      </div>
    </>
  );
}
