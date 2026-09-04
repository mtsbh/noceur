import { For, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { CloseIcon } from "./icons";
import {
  applyCleanup,
  cleanupRules,
  previewCleanup,
  type CleanApply,
  type CleanPreview,
  type CleanRule,
  type WriteReport,
} from "./api";

const RULES_KEY = "noceur.cleanRules";
const TARGET_KEY = "noceur.cleanTarget";

/** What the rules are allowed to touch. */
type Target = "name" | "tags" | "both";

/**
 * Cleaning up names and tags with a set of rules.
 *
 * The rename dialog builds a name out of tags; this fixes what is already
 * there — the `(Original Mix)` on every track of a shop haul, the track number
 * a ripper glued to the front, underscores from a filesystem that could not take
 * spaces.
 *
 * Every change is listed before anything is written, for the same reason
 * renaming previews: these rules are guesses about somebody's music, and a guess
 * applied to four hundred files unseen is not a feature. Ticking a rule
 * re-previews rather than acting, so the list can be read as an answer to "what
 * would this do" before it is an instruction.
 */
export default function Cleanup(props: {
  fileIds: number[];
  onClose: () => void;
  onChanged: () => void;
}) {
  const [catalogue, setCatalogue] = createSignal<CleanRule[]>([]);
  const [chosen, setChosen] = createSignal<string[]>(
    JSON.parse(localStorage.getItem(RULES_KEY) ?? "[]"),
  );
  const [target, setTarget] = createSignal<Target>(
    (localStorage.getItem(TARGET_KEY) as Target) ?? "both",
  );
  const [rows, setRows] = createSignal<CleanPreview[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [report, setReport] = createSignal<WriteReport | null>(null);

  onMount(() => {
    cleanupRules().then(setCatalogue).catch((e) => setError(String(e)));
  });

  const wantsName = () => target() === "name" || target() === "both";
  const wantsTags = () => target() === "tags" || target() === "both";

  // Re-previewed whenever a rule or the target changes. It reads tags off disk,
  // which for a selection of a few dozen is fast enough to feel immediate, and
  // watching the list change as rules are ticked is the entire point.
  createEffect(() => {
    const rules = chosen();
    const ids = props.fileIds;
    const [name, tags] = [wantsName(), wantsTags()];
    setReport(null);

    previewCleanup(ids, rules, name, tags)
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

  function toggle(slug: string) {
    setChosen((current) =>
      current.includes(slug)
        ? current.filter((s) => s !== slug)
        : [...current, slug],
    );
  }

  /** The rows that will actually be written. */
  const willChange = () =>
    (rows() ?? []).filter((r) => !r.unchanged && !r.problem);

  const problems = () => (rows() ?? []).filter((r) => r.problem);

  function apply() {
    const targets = willChange();
    if (targets.length === 0 || busy()) return;
    setBusy(true);
    setError("");
    localStorage.setItem(RULES_KEY, JSON.stringify(chosen()));
    localStorage.setItem(TARGET_KEY, target());

    const items: CleanApply[] = targets.map((r) => ({
      fileId: r.fileId,
      // Empty means leave the name: a cleanup of tags only must not rename.
      name: r.to === r.from ? "" : r.to,
      fields: r.fields.map((f) => [f.field, f.to] as [string, string]),
    }));

    applyCleanup(items)
      .then((result) => {
        setBusy(false);
        setReport(result);
        props.onChanged();
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
      <div class="renamer" role="dialog" aria-label="Clean up">
        <header class="sethead">
          <span class="settitle">Clean up</span>
          <span class="tagsubject">
            {props.fileIds.length}{" "}
            {props.fileIds.length === 1 ? "track" : "tracks"}
          </span>
          <button class="setclose" onClick={props.onClose} title="Close (Esc)">
            <CloseIcon />
          </button>
        </header>

        <div class="renamebody">
          <div class="setseg" role="group" aria-label="What to clean">
            <For
              each={
                [
                  ["both", "Name and tags"],
                  ["name", "File name only"],
                  ["tags", "Tags only"],
                ] as [Target, string][]
              }
            >
              {([value, label]) => (
                <button
                  class={`segbtn ${target() === value ? "on" : ""}`}
                  onClick={() => setTarget(value)}
                >
                  {label}
                </button>
              )}
            </For>
          </div>

          <div class="rulelist">
            <For each={catalogue()}>
              {(rule) => (
                <label class="rule">
                  <input
                    type="checkbox"
                    checked={chosen().includes(rule.slug)}
                    onChange={() => toggle(rule.slug)}
                  />
                  <span class="rulelabel">{rule.label}</span>
                  <span class="rulehint">{rule.hint}</span>
                </label>
              )}
            </For>
          </div>

          <Show
            when={chosen().length > 0}
            fallback={
              <p class="sethint">Tick a rule to see what it would change.</p>
            }
          >
            <Show when={rows()} fallback={<p class="sethint">Reading…</p>}>
              <div class="renamelist">
                <For each={rows()}>
                  {(r) => (
                    <div
                      class={`cleanrow ${r.problem ? "bad" : ""} ${
                        r.unchanged ? "same" : ""
                      }`}
                    >
                      <div class="renamerow">
                        <span class="renamefrom">{r.from}</span>
                        <span class="renamearrow">→</span>
                        <span class="renameto">
                          {r.problem
                            ? r.problem
                            : r.unchanged
                              ? "nothing to do"
                              : r.to}
                        </span>
                      </div>
                      {/* Tag changes sit under the name they belong to, so one
                          row is one file however many fields it touches. */}
                      <For each={r.fields}>
                        {(f) => (
                          <div class="renamerow tagchange">
                            <span class="renamefrom">
                              <span class="fieldname">{f.label}</span> {f.from}
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
                Cleaned {report()!.written}, failed {report()!.failures.length}:
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
            <Show when={wantsName()} fallback="Tag edits cannot be undone.">
              Undo renames in Explorer with Ctrl+Z.
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
            {busy() ? "Cleaning…" : `Clean ${willChange().length}`}
          </button>
        </footer>
      </div>
    </>
  );
}
