import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import {
  previewAction,
  runAction,
  type Action,
  type ActionPreview,
  type WriteReport,
} from "./api";

/** What each kind calls the thing it produces, for the preview's heading. */
const PRODUCES: Record<Action["kind"], string> = {
  tag: "new value",
  rename: "new name",
  open: "arguments",
  url: "link",
  copy: "file",
  move: "file",
};

/**
 * Running one user-defined action over a selection.
 *
 * Shown before anything happens, for the same reason renaming and cleanup are:
 * an expression is a rule somebody wrote, and a rule applied to two hundred
 * files unseen is not something to offer. Every result is listed as it will be.
 */
export default function ActionRunner(props: {
  action: Action;
  fileIds: number[];
  onClose: () => void;
  onDone: () => void;
}) {
  const [rows, setRows] = createSignal<ActionPreview[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [report, setReport] = createSignal<WriteReport | null>(null);

  createEffect(() => {
    previewAction(props.action, props.fileIds)
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

  const ready = () => (rows() ?? []).filter((r) => !r.problem);
  const problems = () => (rows() ?? []).filter((r) => r.problem);

  function run() {
    if (ready().length === 0 || busy()) return;
    setBusy(true);
    setError("");

    runAction(props.action, ready().map((r) => r.fileId))
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

  /** What the destination or field is, said plainly under the title. */
  const subject = () => {
    switch (props.action.kind) {
      case "tag":
        return `sets ${props.action.target}`;
      case "rename":
        return "renames";
      case "copy":
        return `copies to ${props.action.target}`;
      case "move":
        return `moves to ${props.action.target}`;
      case "open":
        return `opens in ${props.action.target.split(/[\\/]/).pop()}`;
      case "url":
        return "opens a link";
    }
  };

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer" role="dialog" aria-label={props.action.label}>
        <header class="sethead">
          <span class="settitle">{props.action.label}</span>
          <span class="tagsubject">
            {subject()} · {props.fileIds.length}{" "}
            {props.fileIds.length === 1 ? "track" : "tracks"}
          </span>
          <button class="setclose" onClick={props.onClose} title="Close (Esc)">
            <CloseIcon />
          </button>
        </header>

        <div class="renamebody">
          <Show when={rows()} fallback={<p class="sethint">Reading…</p>}>
            <div class="renamelist">
              <For each={rows()}>
                {(r) => (
                  <div class={`renamerow ${r.problem ? "bad" : ""}`}>
                    <span class="renamefrom">{r.name}</span>
                    <span class="renamearrow">→</span>
                    <span class="renameto">{r.problem || r.result}</span>
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
                Did {report()!.written}, failed {report()!.failures.length}:
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
            {PRODUCES[props.action.kind]} shown for every track.
          </span>
          <button class="btn" onClick={props.onClose}>
            Cancel
          </button>
          <button
            class="btn primary"
            disabled={ready().length === 0 || busy()}
            onClick={run}
          >
            {busy() ? "Working…" : `Run on ${ready().length}`}
          </button>
        </footer>
      </div>
    </>
  );
}
