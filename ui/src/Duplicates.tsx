import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import { duplicates, identify, reveal, type DuplicateGroup } from "./api";

/**
 * Files measured per round trip.
 *
 * Each is a full decode, so the count only moves when a batch comes back. Small
 * enough that it moves often; large enough that the round trips are not the
 * cost.
 */
const BATCH = 4;

/**
 * The same track, found twice.
 *
 * A DJ library is the same music at several paths — a copy on the party drive,
 * one in a Rekordbox folder, one where it was downloaded — and each copy gets
 * tagged by whatever opened it. That makes them different *files* and the same
 * *recording*, and only one of those two facts is visible by looking.
 *
 * So this reads the audio rather than the tags. Two files match here when they
 * decode to identical sound, whatever their names, tags, containers or dates
 * say. Nothing is deleted: it says what you have and where, and leaves the
 * decision where it belongs.
 */
export default function Duplicates(props: {
  /** Files to measure — the selection, or the folder when nothing is selected. */
  fileIds: number[];
  /** Folders to report over. */
  scopes: string[];
  onClose: () => void;
}) {
  const [groups, setGroups] = createSignal<DuplicateGroup[] | null>(null);
  const [done, setDone] = createSignal(0);
  const [measuring, setMeasuring] = createSignal(true);
  const [error, setError] = createSignal("");
  let abandoned = false;

  onCleanup(() => {
    abandoned = true;
  });

  createEffect(() => {
    const ids = props.fileIds;

    (async () => {
      for (let at = 0; at < ids.length; at += BATCH) {
        if (abandoned) return;
        try {
          await identify(ids.slice(at, at + BATCH));
          setDone(Math.min(at + BATCH, ids.length));
        } catch (e) {
          setError(String(e));
          break;
        }
      }
      if (abandoned) return;
      setMeasuring(false);

      // Reported only once the measuring is done. A list that grew as files
      // were read would rearrange itself under the pointer, and a group is only
      // meaningful when everything it might contain has been looked at.
      duplicates(props.scopes)
        .then(setGroups)
        .catch((e) => {
          setGroups([]);
          setError(String(e));
        });
    })();
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

  const copies = () =>
    (groups() ?? []).reduce((total, g) => total + g.files.length, 0);

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer wide" role="dialog" aria-label="Duplicates">
        <header class="sethead">
          <span class="settitle">Same audio</span>
          <span class="tagsubject">
            <Show
              when={measuring()}
              fallback={
                groups()
                  ? `${groups()!.length} ${groups()!.length === 1 ? "group" : "groups"}`
                  : ""
              }
            >
              reading {done()} of {props.fileIds.length}…
            </Show>
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
          <Show
            when={groups()}
            fallback={
              <p class="sethint">
                Reading each track end to end — tags are not part of the audio,
                so this is the only way to know.
              </p>
            }
          >
            <Show
              when={groups()!.length > 0}
              fallback={
                <p class="sethint">
                  Nothing here appears twice. Only files that have been read are
                  compared — a copy somewhere you have not looked is not counted.
                </p>
              }
            >
              <For each={groups()}>
                {(group) => (
                  <div class="dupgroup">
                    <div class="duphead">
                      <span class="mono duphash">{group.hash}</span>
                      <span class="sethint">
                        {group.files.length} copies · {group.wasted} beyond the
                        first
                      </span>
                    </div>
                    <For each={group.files}>
                      {(file) => (
                        <div
                          class="duprow"
                          onClick={() => reveal(file.fileId).catch(() => {})}
                          data-tip="Show in Explorer"
                        >
                          <span class="dupname">{file.name}</span>
                          <span class="dupfolder">{file.folder}</span>
                          <span class="dupsize mono">{file.size}</span>
                          <span class="dupdate mono">{file.modified}</span>
                        </div>
                      )}
                    </For>
                  </div>
                )}
              </For>
            </Show>
          </Show>

          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>
        </div>

        <footer class="tagfoot">
          <span class="sethint credit">
            <Show when={groups() && groups()!.length > 0}>
              {copies()} files, {groups()!.length} recordings ·{" "}
            </Show>
            Matched on the audio, not the tags. Click a row to show it in
            Explorer.
          </span>
          <button class="btn" onClick={props.onClose}>
            {measuring() ? "Stop" : "Close"}
          </button>
        </footer>
      </div>
    </>
  );
}
