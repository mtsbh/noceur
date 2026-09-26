import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { CloseIcon } from "./icons";
import { createFolder } from "./api";

/**
 * Names a new folder.
 *
 * A small prompt rather than Explorer's in-place rename: the tree and the list
 * are both virtualized, and a text box inside a row that can scroll out of
 * existence is a text box that loses what was typed.
 */
export default function NewFolder(props: {
  parent: string;
  onClose: () => void;
  onCreated: (path: string) => void;
}) {
  const [name, setName] = createSignal("New folder");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  let input: HTMLInputElement | undefined;

  onMount(() => {
    input?.focus();
    input?.select();
  });

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    }
  };
  window.addEventListener("keydown", onKey, true);
  onCleanup(() => window.removeEventListener("keydown", onKey, true));

  function create() {
    if (busy()) return;
    setBusy(true);
    setError("");
    createFolder(props.parent, name())
      .then((path) => {
        props.onCreated(path);
        props.onClose();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  }

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="newfolder" role="dialog" aria-label="New folder">
        <header class="sethead">
          <span class="settitle">New folder</span>
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
        <div class="newfolderbody">
          <p class="newfolderin" title={props.parent}>
            in {props.parent}
          </p>
          <input
            ref={(el) => (input = el)}
            type="text"
            value={name()}
            spellcheck={false}
            onInput={(e) => {
              setName(e.currentTarget.value);
              setError("");
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                create();
              }
            }}
          />
          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>
        </div>
        <footer class="tagfoot">
          <span class="sethint credit" />
          <button class="btn" onClick={props.onClose}>
            Cancel
          </button>
          <button class="btn primary" disabled={busy()} onClick={create}>
            Create
          </button>
        </footer>
      </div>
    </>
  );
}
