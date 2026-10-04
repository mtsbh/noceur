import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { CloseIcon } from "./icons";
import { createPlaylist, togglePlaylist } from "./api";

/** Names a new playlist and puts the selected tracks in it. */
export default function NewPlaylist(props: {
  fileIds: number[];
  onClose: () => void;
  onCreated: (id: number, name: string, added: number) => void;
}) {
  const [name, setName] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  let input: HTMLInputElement | undefined;

  onMount(() => input?.focus());

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    }
  };
  window.addEventListener("keydown", onKey, true);
  onCleanup(() => window.removeEventListener("keydown", onKey, true));

  async function create() {
    if (busy()) return;
    setBusy(true);
    setError("");
    try {
      const id = await createPlaylist(name());
      const done = props.fileIds.length
        ? await togglePlaylist(id, props.fileIds)
        : { added: 0, removed: 0 };
      props.onCreated(id, name().trim(), done.added);
      props.onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="newfolder" role="dialog" aria-label="New playlist">
        <header class="sethead">
          <span class="settitle">New playlist</span>
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
          <Show when={props.fileIds.length > 0}>
            <p class="newfolderin">
              with {props.fileIds.length} {props.fileIds.length === 1 ? "track" : "tracks"}
            </p>
          </Show>
          <input
            ref={(el) => (input = el)}
            type="text"
            placeholder="Name"
            value={name()}
            spellcheck={false}
            onInput={(e) => {
              setName(e.currentTarget.value);
              setError("");
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void create();
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
          <button class="btn primary" disabled={busy() || !name().trim()} onClick={() => void create()}>
            Create
          </button>
        </footer>
      </div>
    </>
  );
}
