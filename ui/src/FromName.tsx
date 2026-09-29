import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { applyFromName, previewFromName, type FromNameRow, type WriteReport } from "./api";

/** Patterns worth starting from. The first is how most downloads are named. */
const PRESETS: [string, string][] = [
  ["%artist% - %title%", "Artist - Title"],
  ["%track% - %artist% - %title%", "01 - Artist - Title"],
  ["%track%. %artist% - %title%", "01. Artist - Title"],
  ["%track% %title%", "01 Title"],
  ["%artist% - %album%\\%track% - %title%", "Folder: Artist - Album"],
];

/** Fields a pattern understands, offered as chips. */
const FIELDS = [
  "artist",
  "title",
  "album",
  "albumartist",
  "genre",
  "year",
  "track",
  "bpm",
  "key",
  "comment",
  "dummy",
];

const KEY = "noceur.fromNamePattern";

/**
 * Fills tags from file names - the renamer run backwards.
 *
 * Shows what each file would get before anything is written, and says which
 * names do not fit rather than guessing at them.
 */
export default function FromName(props: {
  fileIds: number[];
  onClose: () => void;
  onDone: () => void;
}) {
  const [pattern, setPattern] = createSignal(localStorage.getItem(KEY) ?? PRESETS[0][0]);
  const [rows, setRows] = createSignal<FromNameRow[] | null>(null);
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [report, setReport] = createSignal<WriteReport | null>(null);
  let input: HTMLInputElement | undefined;

  let generation = 0;
  createEffect(() => {
    const p = pattern();
    const ids = props.fileIds;
    const mine = ++generation;
    setError("");
    // A short pause so typing a pattern does not preview every keystroke.
    const timer = window.setTimeout(() => {
      previewFromName(ids, p)
        .then((r) => mine === generation && setRows(r))
        .catch((e) => {
          if (mine !== generation) return;
          setRows(null);
          setError(String(e));
        });
    }, 150);
    onCleanup(() => window.clearTimeout(timer));
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

  const fits = () => (rows() ?? []).filter((r) => !r.problem);

  /** Puts a field at the cursor, as clicking a chip should. */
  function insert(field: string) {
    const token = `%${field}%`;
    if (!input) return setPattern(pattern() + token);
    const from = input.selectionStart ?? pattern().length;
    const to = input.selectionEnd ?? from;
    const next = pattern().slice(0, from) + token + pattern().slice(to);
    setPattern(next);
    queueMicrotask(() => {
      input?.focus();
      input?.setSelectionRange(from + token.length, from + token.length);
    });
  }

  function apply() {
    if (fits().length === 0 || busy()) return;
    setBusy(true);
    try {
      localStorage.setItem(KEY, pattern());
    } catch {
      // Remembering the pattern is a convenience.
    }
    applyFromName(props.fileIds, pattern())
      .then((r) => {
        props.onDone();
        if (r.failures.length === 0) props.onClose();
        else setReport(r);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  }

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer wide" role="dialog" aria-label="Tags from file name">
        <header class="sethead">
          <span class="settitle">Tags from file name</span>
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

        <div class="renamebody">
          <div class="setrow">
            <span class="setlabel">Pattern</span>
            <input
              ref={(el) => (input = el)}
              type="text"
              class="mono"
              value={pattern()}
              spellcheck={false}
              onInput={(e) => setPattern(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  apply();
                }
              }}
            />
          </div>
          <div class="setrow top">
            <span class="setlabel" />
            <div class="fnchips">
              <For each={PRESETS}>
                {([value, label]) => (
                  <button
                    class={`fnchip preset ${pattern() === value ? "on" : ""}`}
                    onClick={() => setPattern(value)}
                  >
                    {label}
                  </button>
                )}
              </For>
            </div>
          </div>
          <div class="setrow top">
            <span class="setlabel" />
            <div class="fnchips">
              <For each={FIELDS}>
                {(f) => (
                  <button class="fnchip mono" onClick={() => insert(f)} data-tip={`Insert %${f}%`}>
                    %{f}%
                  </button>
                )}
              </For>
            </div>
          </div>
          <p class="acthint">
            Each field runs up to the first text that follows it; the last takes the rest.
            %dummy% skips a piece. A \ reaches into the folder above.
          </p>

          <Show when={rows()}>
            <div class="renamelist">
              <For each={rows()}>
                {(r) => (
                  <div class={`renamerow ${r.problem ? "same" : ""}`}>
                    <span class="renamefrom" title={r.name}>
                      {r.name}
                    </span>
                    <span class="renamearrow">→</span>
                    <span class="renameto fnfields">
                      <Show when={!r.problem} fallback={r.problem}>
                        <For each={r.fields}>
                          {([label, value]) => (
                            <span class="fnfield">
                              <span class="fieldname">{label}</span> {value}
                            </span>
                          )}
                        </For>
                      </Show>
                    </span>
                  </div>
                )}
              </For>
            </div>
          </Show>

          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>
          <Show when={report()}>
            {(r) => (
              <div class="tagfailures">
                <p>
                  Tagged {r().written}, failed {r().failures.length}:
                </p>
                <For each={r().failures}>
                  {(f) => (
                    <p class="tagfail">
                      <b>{f.name}</b> — {f.reason}
                    </p>
                  )}
                </For>
              </div>
            )}
          </Show>
        </div>

        <footer class="tagfoot">
          <span class="sethint credit">
            <Show when={rows() && fits().length < rows()!.length}>
              {rows()!.length - fits().length} skipped
            </Show>
          </span>
          <button class="btn" onClick={props.onClose}>
            Cancel
          </button>
          <button class="btn primary" disabled={fits().length === 0 || busy()} onClick={apply}>
            {busy() ? "Tagging…" : `Tag ${fits().length}`}
          </button>
        </footer>
      </div>
    </>
  );
}
