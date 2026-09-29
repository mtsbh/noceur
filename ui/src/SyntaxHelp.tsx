import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { RECIPES, SAMPLE, SYNTAX, type HelpEntry } from "./syntax";

/**
 * The expression language, in the app rather than in a browser tab.
 *
 * Searchable, and every example is one click from the editor: "Use" replaces
 * the expression with a recipe, "Insert" adds a snippet to the end of it.
 */
export default function SyntaxHelp(props: {
  onUse: (code: string) => void;
  onInsert: (code: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = createSignal("");

  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  const matches = (...texts: (string | undefined)[]) => {
    const q = query().trim().toLowerCase();
    return !q || texts.some((t) => t?.toLowerCase().includes(q));
  };
  const hit = (e: HelpEntry) => matches(e.code, e.about, e.example);

  return (
    <>
      <div class="scrim over" onPointerDown={props.onClose} />
      <div class="renamer syntaxhelp" role="dialog" aria-label="Expression syntax">
        <header class="sethead">
          <span class="settitle">Expression syntax</span>
          <input
            class="syntaxsearch"
            type="text"
            placeholder="Search…"
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            ref={(el) => queueMicrotask(() => el.focus())}
          />
          <button class="setclose" onClick={props.onClose} aria-label="Close">
            <CloseIcon />
          </button>
        </header>

        <div class="renamebody syntaxbody">
          <Show when={!query()}>
            <div class="syntaxsample">
              <span class="syntaxlabel">Examples use</span>
              <For each={SAMPLE}>
                {([field, value]) => (
                  <span class="syntaxpair">
                    <code>{field}</code> {value}
                  </span>
                )}
              </For>
            </div>
          </Show>

          <Show when={RECIPES.some((r) => matches(r.label, r.code))}>
            <section class="syntaxsection">
              <h3>Recipes</h3>
              <For each={RECIPES.filter((r) => matches(r.label, r.code))}>
                {(r) => (
                  <div class="syntaxrecipe">
                    <div class="syntaxrecipehead">
                      <span>{r.label}</span>
                      <button class="actlink" onClick={() => props.onUse(r.code)}>
                        Use
                      </button>
                    </div>
                    <code class="syntaxcode">{r.code}</code>
                    <span class="syntaxresult">→ {r.result}</span>
                  </div>
                )}
              </For>
            </section>
          </Show>

          <For each={SYNTAX.filter((s) => s.entries.some(hit))}>
            {(section) => (
              <section class="syntaxsection">
                <h3>{section.title}</h3>
                <Show when={section.intro && !query()}>
                  <p class="syntaxintro">{section.intro}</p>
                </Show>
                <For each={section.entries.filter(hit)}>
                  {(e) => (
                    <div class="syntaxentry">
                      <button
                        class="syntaxsig"
                        data-tip="Insert"
                        onClick={() => props.onInsert(e.example ?? e.code)}
                      >
                        {e.code}
                      </button>
                      <div class="syntaxabout">
                        <span>{e.about}</span>
                        <Show when={e.example}>
                          <span class="syntaxexample">
                            <code>{e.example}</code> → <b>{e.result}</b>
                          </span>
                        </Show>
                      </div>
                    </div>
                  )}
                </For>
              </section>
            )}
          </For>
        </div>
      </div>
    </>
  );
}
