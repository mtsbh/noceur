import { For, Show, createEffect, createSignal, onCleanup, untrack } from "solid-js";
import { CloseIcon } from "./icons";
import {
  convertFile,
  convertPlan,
  type ConvertDepth,
  type ConvertFormat,
  type ConvertOptions,
  type ConvertPlanned,
  type Converted,
} from "./api";

const FORMATS: [ConvertFormat, string][] = [
  ["wav", "WAV"],
  ["aiff", "AIFF"],
  ["flac", "FLAC"],
  ["alac", "ALAC"],
];

const DEPTHS: [ConvertDepth, string][] = [
  ["source", "Same as source"],
  ["24", "24-bit"],
  ["16", "16-bit"],
];

/** Choices worth keeping between conversions: one person converts one way. */
const KEY = "noceur.convert";

function loadOptions(): ConvertOptions {
  const fallback: ConvertOptions = {
    format: "wav",
    depth: "source",
    folder: "",
    replace: false,
    verify: true,
  };
  try {
    return { ...fallback, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return fallback;
  }
}

/**
 * Converts a selection between WAV, AIFF, FLAC and ALAC through ffmpeg.
 *
 * Lists what each file will become before anything is written, then converts
 * one file at a time so progress is visible and Stop takes effect between
 * files. With "check" on, each output is decoded and compared with its source
 * sample by sample, and one that does not match is thrown away.
 */
export default function Converter(props: {
  fileIds: number[];
  /** The other pane's folder, offered as a destination when split. */
  otherFolder?: string;
  onClose: () => void;
  onDone: () => void;
}) {
  const [options, setOptions] = createSignal<ConvertOptions>(loadOptions());
  const [plan, setPlan] = createSignal<ConvertPlanned[] | null>(null);
  const [results, setResults] = createSignal<Record<number, Converted>>({});
  const [running, setRunning] = createSignal(false);
  const [error, setError] = createSignal("");
  let stopped = false;
  let closed = false;
  onCleanup(() => {
    closed = true;
    stopped = true;
  });

  const patch = (next: Partial<ConvertOptions>) => {
    const merged = { ...options(), ...next };
    // New choices mean a new plan; the last run's results no longer describe it.
    setResults({});
    setOptions(merged);
    try {
      localStorage.setItem(KEY, JSON.stringify(merged));
    } catch {
      // Remembering the choice is a convenience, not the job.
    }
  };

  // Re-plan whenever the choices change. Not on the end of a run, which would
  // turn every row it just converted into "already converted".
  let generation = 0;
  createEffect(() => {
    const opts = options();
    const ids = props.fileIds;
    if (untrack(running)) return;
    const mine = ++generation;
    setPlan(null);
    setError("");
    convertPlan(ids, opts)
      .then((p) => mine === generation && setPlan(p))
      .catch((e) => mine === generation && setError(String(e)));
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

  const todo = () => (plan() ?? []).filter((p) => !p.problem);
  const done = () => Object.values(results());
  const failed = () => done().filter((r) => !r.ok);

  async function run() {
    const queue = todo();
    if (queue.length === 0) return;
    stopped = false;
    setRunning(true);
    setResults({});
    for (const item of queue) {
      if (stopped) break;
      try {
        const r = await convertFile(item.fileId, options());
        if (closed) return;
        setResults((all) => ({ ...all, [item.fileId]: r }));
      } catch (e) {
        setResults((all) => ({
          ...all,
          [item.fileId]: {
            fileId: item.fileId,
            output: item.output,
            ok: false,
            verified: null,
            message: String(e),
          },
        }));
      }
    }
    setRunning(false);
    props.onDone();
  }

  /** What the right-hand side of a row says. */
  const status = (p: ConvertPlanned) => {
    const r = results()[p.fileId];
    if (r) {
      if (!r.ok) return r.message || "failed";
      const check = r.verified ? "✓ bit-identical" : "done";
      return r.message ? `${check} · ${r.message}` : check;
    }
    if (p.problem) return p.problem;
    return p.exact ? `${p.to} · lossless` : p.to;
  };

  const rowClass = (p: ConvertPlanned) => {
    const r = results()[p.fileId];
    if (r) return r.ok ? "ok" : "bad";
    return p.problem ? "same" : "";
  };

  return (
    <>
      <div class="scrim" onPointerDown={() => !running() && props.onClose()} />
      <div class="renamer wide" role="dialog" aria-label="Convert">
        <header class="sethead">
          <span class="settitle">Convert</span>
          <span class="tagsubject">
            <Show
              when={running()}
              fallback={`${props.fileIds.length} ${props.fileIds.length === 1 ? "track" : "tracks"}`}
            >
              {done().length} of {todo().length}…
            </Show>
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

        <div class="renamebody">
          <div class="convopts">
            <div class="setrow">
              <span class="setlabel">Format</span>
              <div class="setseg" role="group">
                <For each={FORMATS}>
                  {([value, label]) => (
                    <button
                      class={`segbtn ${options().format === value ? "on" : ""}`}
                      disabled={running()}
                      onClick={() => patch({ format: value })}
                    >
                      {label}
                    </button>
                  )}
                </For>
              </div>
            </div>
            <div class="setrow">
              <span class="setlabel">Bit depth</span>
              <div class="setseg" role="group">
                <For each={DEPTHS}>
                  {([value, label]) => (
                    <button
                      class={`segbtn ${options().depth === value ? "on" : ""}`}
                      disabled={running()}
                      onClick={() => patch({ depth: value })}
                    >
                      {label}
                    </button>
                  )}
                </For>
              </div>
            </div>
            <div class="setrow">
              <span class="setlabel">Save to</span>
              <input
                type="text"
                class="convfolder"
                value={options().folder}
                placeholder="Beside each original"
                spellcheck={false}
                disabled={running()}
                onChange={(e) => patch({ folder: e.currentTarget.value.trim() })}
              />
              <Show when={props.otherFolder}>
                <button
                  class="btn"
                  disabled={running()}
                  data-tip={props.otherFolder}
                  onClick={() => patch({ folder: props.otherFolder! })}
                >
                  Other pane
                </button>
              </Show>
              <Show when={options().folder}>
                <button class="btn" disabled={running()} onClick={() => patch({ folder: "" })}>
                  Beside original
                </button>
              </Show>
            </div>
            <div class="setrow">
              <span class="setlabel" />
              <label class="setcheck">
                <input
                  type="checkbox"
                  checked={options().verify}
                  disabled={running()}
                  onChange={(e) => patch({ verify: e.currentTarget.checked })}
                />
                <span>Check each result is bit-identical to the original</span>
              </label>
              <label class="setcheck">
                <input
                  type="checkbox"
                  checked={options().replace}
                  disabled={running()}
                  onChange={(e) => patch({ replace: e.currentTarget.checked })}
                />
                <span>Replace existing</span>
              </label>
            </div>
          </div>

          <Show when={plan()} fallback={<Show when={!error()}><p class="sethint">Reading…</p></Show>}>
            <div class="renamelist">
              <For each={plan()}>
                {(p) => (
                  <div class={`renamerow conv ${rowClass(p)}`}>
                    <span class="renamefrom" title={p.name}>
                      {p.name}
                      <Show when={p.from}>
                        <span class="convfrom"> {p.from}</span>
                      </Show>
                    </span>
                    <span class="renamearrow">→</span>
                    <span class="renameto" title={p.output}>
                      {status(p)}
                    </span>
                  </div>
                )}
              </For>
            </div>
          </Show>

          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>
        </div>

        <footer class="tagfoot">
          <span class="sethint credit">
            <Show
              when={done().length > 0 && !running()}
              fallback={
                <>
                  <Show when={(plan() ?? []).length - todo().length > 0}>
                    {(plan() ?? []).length - todo().length} skipped ·{" "}
                  </Show>
                  tags & art kept
                </>
              }
            >
              {done().length - failed().length} converted
              <Show when={failed().length > 0}>, {failed().length} failed</Show>.
            </Show>
          </span>
          <button
            class="btn"
            onClick={() => {
              if (running()) stopped = true;
              else props.onClose();
            }}
          >
            {running() ? "Stop" : "Close"}
          </button>
          <button
            class="btn primary"
            disabled={todo().length === 0 || running() || done().length > 0}
            onClick={run}
          >
            {running() ? "Converting…" : `Convert ${todo().length}`}
          </button>
        </footer>
      </div>
    </>
  );
}
