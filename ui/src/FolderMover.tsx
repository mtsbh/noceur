import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import {
  previewAction,
  runAction,
  type Action,
  type ActionPreview,
  type WriteReport,
} from "./api";

const KEY = "noceur.moveInto";

/** Folder shapes offered under the box. `{root}` is whatever folder is typed. */
const PRESETS = [
  "%artist% - %album% '['%year%']'",
  "%artist% - %album% '('%year%')'",
  "%artist%\\%album%",
  "%label%",
  "%genre%",
  "%year%",
];

/**
 * Splits `E:\music\%artist% - %album%` into the folder that must exist and the
 * part built per file. Written the way Mp3tag's `_DIRECTORY` is, as one path.
 */
function split(path: string): { root: string; pattern: string } {
  const at = path.indexOf("%");
  const head = at < 0 ? path : path.slice(0, at);
  const cut = Math.max(head.lastIndexOf("\\"), head.lastIndexOf("/"));
  if (cut < 0) return { root: "", pattern: path };
  return { root: path.slice(0, cut), pattern: path.slice(cut + 1) };
}

/**
 * Moving a selection into folders built from its tags.
 *
 * One path, like Mp3tag's: the folders named outright must exist, the ones
 * built from tags are made. The file keeps its own name. Every destination is
 * listed before anything moves. The right side of the Actions dialog, which
 * owns the frame and Escape.
 */
export default function FolderMover(props: {
  fileIds: number[];
  onClose: () => void;
  onDone: () => void;
}) {
  const [path, setPath] = createSignal(
    localStorage.getItem(KEY) ?? "E:\\music\\%artist% - %album% '['%year%']'",
  );
  const [rows, setRows] = createSignal<ActionPreview[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [report, setReport] = createSignal<WriteReport | null>(null);

  const action = (): Action => {
    const { root, pattern } = split(path().trim());
    const sep = pattern && !pattern.endsWith("\\") ? "\\" : "";
    return {
      id: "move-into-folders",
      label: "Move into folders",
      kind: "move",
      target: root,
      script: `${pattern}${sep}%filename%`,
      recipe: "custom",
      slots: [],
    };
  };

  let timer: number | undefined;
  createEffect(() => {
    const a = action();
    const ids = props.fileIds;
    clearTimeout(timer);
    timer = window.setTimeout(() => {
      setError("");
      previewAction(a, ids)
        .then(setRows)
        .catch((e) => {
          setRows([]);
          setError(String(e));
        });
    }, 200);
  });
  onCleanup(() => clearTimeout(timer));

  const ready = () => (rows() ?? []).filter((r) => !r.problem);
  const problems = () => (rows() ?? []).filter((r) => r.problem);

  function run() {
    if (ready().length === 0 || busy()) return;
    localStorage.setItem(KEY, path().trim());
    setBusy(true);
    setError("");
    runAction(action(), ready().map((r) => r.fileId))
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

  function preset(shape: string) {
    const { root } = split(path().trim());
    setPath(root ? `${root}\\${shape}` : shape);
  }

  return (
    <div class="actright">
        <div class="renamebody">
          <input
            type="text"
            class="toolpath mono"
            value={path()}
            spellcheck={false}
            aria-label="Folder"
            onInput={(e) => setPath(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") run();
            }}
          />
          <div class="toolpresets">
            <For each={PRESETS}>
              {(p) => (
                <button class="chip" onClick={() => preset(p)}>
                  {p}
                </button>
              )}
            </For>
          </div>

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
                Moved {report()!.written}, failed {report()!.failures.length}:
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
            <Show when={problems().length > 0}>{problems().length} skipped</Show>
          </span>
          <button class="btn" onClick={props.onClose}>
            Cancel
          </button>
          <button class="btn primary" disabled={ready().length === 0 || busy()} onClick={run}>
            {busy() ? "Moving…" : `Move ${ready().length}`}
          </button>
        </footer>
    </div>
  );
}
