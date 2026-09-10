import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { CloseIcon } from "./icons";
import { type Action, type ActionKind } from "./api";
import {
  CUSTOM_RECIPE,
  TAG_FIELDS,
  recipeById,
  recipesFor,
  type Recipe,
} from "./recipes";

/**
 * One button, built rather than written.
 *
 * A dialog rather than a block inside settings: the builder needs a picker, a
 * field, a couple of boxes and the expression it produces, and putting all of
 * that inline turned the settings panel into a wall. Settings keeps a line per
 * button; this is where a line gets edited.
 */

/** The kinds, and what each one's target box is for. An empty target means none. */
const KINDS: { kind: ActionKind; label: string; target: string }[] = [
  { kind: "tag", label: "Set a tag", target: "" },
  { kind: "rename", label: "Rename the file", target: "" },
  { kind: "open", label: "Open in a program", target: "Program (.exe)" },
  { kind: "url", label: "Look it up online", target: "" },
  { kind: "copy", label: "Copy into a folder", target: "Folder" },
  { kind: "move", label: "Move into a folder", target: "Folder" },
];

const kindOf = (kind: ActionKind) => KINDS.find((k) => k.kind === kind) ?? KINDS[0];

export default function ActionEditor(props: {
  action: Action;
  /** Reported on every change, so the list behind this stays in step. */
  onChange: (next: Action) => void;
  onClose: () => void;
}) {
  const [advanced, setAdvanced] = createSignal(props.action.recipe === CUSTOM_RECIPE);

  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") props.onClose();
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  const patch = (next: Partial<Action>) => props.onChange({ ...props.action, ...next });

  const recipe = createMemo<Recipe | undefined>(() => recipeById(props.action.recipe));

  /** The slot values, padded to whatever the current recipe asks for. */
  const slots = () => {
    const want = recipe()?.slots?.length ?? 0;
    const have = props.action.slots ?? [];
    return Array.from({ length: want }, (_, i) => have[i] ?? recipe()!.slots![i].value ?? "");
  };

  /**
   * Regenerates the expression from the recipe.
   *
   * Everything the recipe depends on comes through here — the field, the slots
   * — so the expression can never be left describing an older answer than the
   * boxes above it.
   */
  function rebuild(over: Partial<Action> = {}) {
    const merged = { ...props.action, ...over };
    const r = recipeById(merged.recipe);
    if (!r) return patch(over);

    const values = Array.from(
      { length: r.slots?.length ?? 0 },
      (_, i) => merged.slots?.[i] ?? r.slots![i].value ?? "",
    );
    patch({ ...over, script: r.build(merged.target || "title", values) });
  }

  /** Switching kind resets to that kind's first recipe: a rename is not a tag edit. */
  function chooseKind(kind: ActionKind) {
    const first = recipesFor(kind)[0];
    if (!first) return patch({ kind });
    const target = first.field ? "title" : (first.target ?? "");
    patch({
      kind,
      recipe: first.id,
      target,
      slots: (first.slots ?? []).map((s) => s.value ?? ""),
      script: first.build(target || "title", (first.slots ?? []).map((s) => s.value ?? "")),
    });
  }

  function chooseRecipe(id: string) {
    if (id === CUSTOM_RECIPE) {
      setAdvanced(true);
      return patch({ recipe: CUSTOM_RECIPE });
    }
    const r = recipeById(id);
    if (!r) return;
    const values = (r.slots ?? []).map((s) => s.value ?? "");
    const target = r.field ? props.action.target || "title" : (r.target ?? props.action.target);
    patch({
      recipe: id,
      target,
      slots: values,
      script: r.build(target || "title", values),
    });
  }

  /** Whether this kind wants a target box, and what to call it. */
  const targetLabel = () => (recipe()?.field ? "" : kindOf(props.action.kind).target);

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="acted" role="dialog" aria-label="Button">
        <header class="sethead">
          <span class="settitle">Button</span>
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

        <div class="actbody">
          <div class="setrow">
            <span class="setlabel">Name</span>
            <input
              type="text"
              value={props.action.label}
              placeholder="What the button is called"
              spellcheck={false}
              onInput={(e) => patch({ label: e.currentTarget.value })}
            />
          </div>

          <div class="setrow">
            <span class="setlabel">Does</span>
            <select
              class="setselect"
              value={props.action.kind}
              onChange={(e) => chooseKind(e.currentTarget.value as ActionKind)}
            >
              <For each={KINDS}>{(k) => <option value={k.kind}>{k.label}</option>}</For>
            </select>
          </div>

          <div class="setrow">
            <span class="setlabel">How</span>
            <select
              class="setselect"
              value={props.action.recipe}
              onChange={(e) => chooseRecipe(e.currentTarget.value)}
            >
              <For each={recipesFor(props.action.kind)}>
                {(r) => <option value={r.id}>{r.label}</option>}
              </For>
              <option value={CUSTOM_RECIPE}>Write it myself…</option>
            </select>
          </div>

          <Show when={recipe()?.hint}>
            <p class="acthint">{recipe()!.hint}</p>
          </Show>

          {/* A recipe that writes a tag asks which one. Free text as well as a
              list, because every library has a field somebody else's does not. */}
          <Show when={recipe()?.field}>
            <div class="setrow">
              <span class="setlabel">Field</span>
              <input
                type="text"
                class="mono"
                list="noceur-tag-fields"
                value={props.action.target}
                placeholder="title"
                spellcheck={false}
                onInput={(e) => rebuild({ target: e.currentTarget.value })}
              />
              <datalist id="noceur-tag-fields">
                <For each={TAG_FIELDS}>{(f) => <option value={f} />}</For>
              </datalist>
            </div>
          </Show>

          <Show when={targetLabel()}>
            <div class="setrow">
              <span class="setlabel">{targetLabel()}</span>
              <input
                type="text"
                class="mono"
                value={props.action.target}
                placeholder={targetLabel()}
                spellcheck={false}
                onInput={(e) => rebuild({ target: e.currentTarget.value })}
              />
            </div>
          </Show>

          <For each={recipe()?.slots ?? []}>
            {(slot, at) => (
              <div class="setrow">
                <span class="setlabel">{slot.label}</span>
                <input
                  type="text"
                  value={slots()[at()]}
                  placeholder={slot.placeholder}
                  spellcheck={false}
                  onInput={(e) => {
                    const next = [...slots()];
                    next[at()] = e.currentTarget.value;
                    rebuild({ slots: next });
                  }}
                />
              </div>
            )}
          </For>

          {/* The generated expression, shown rather than hidden. A recipe is a
              starting point: seeing what it produced is how anybody learns the
              language, and editing it is how they leave the recipes behind. */}
          <div class="setrow actadv">
            <button class="actlink" onClick={() => setAdvanced((on) => !on)}>
              {advanced() ? "▾" : "▸"} Expression
            </button>
          </div>
          <Show when={advanced()}>
            <div class="setrow">
              <input
                type="text"
                class="mono"
                value={props.action.script}
                placeholder="%artist% - %title%"
                spellcheck={false}
                onInput={(e) =>
                  patch({ script: e.currentTarget.value, recipe: CUSTOM_RECIPE })
                }
              />
            </div>
            <p class="sethint">
              foobar2000 syntax, so anything you already have for foobar, Mp3tag
              or OneTagger works as written. Editing this makes the button your
              own — the recipe stops rewriting it.
            </p>
          </Show>
        </div>

        <footer class="actfoot">
          <button class="btn" onClick={props.onClose}>
            Done
          </button>
        </footer>
      </div>
    </>
  );
}
