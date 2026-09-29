import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import {
  TAG_FIELDS,
  applyCover,
  fullArtUrl,
  readTags,
  removeCover,
  writeTags,
  type TagField,
  type TagForm,
  type WriteFailure,
  type WriteReport,
} from "./api";

/** Fields that get a short box on a shared row rather than a full-width one. */
const NARROW: TagField[] = ["year", "trackNo", "bpm", "key"];

/**
 * The tag editor.
 *
 * Edits the selection, however many files that is. The multi-file case is the
 * one that shapes the whole design: a field the selected files disagree about
 * shows a "various" placeholder rather than a value, and stays that way unless
 * it is typed into. Only boxes that were actually edited are sent, so setting
 * the album on twelve tracks cannot quietly overwrite their twelve titles with
 * whichever one happened to load into the form.
 */
export default function TagEditor(props: {
  fileIds: number[];
  onClose: () => void;
  /** Called after a successful write, so the listing can re-read the rows. */
  onWritten: () => void;
  /**
   * Fields to start with already filled in, from a matched release.
   *
   * They arrive marked as edits rather than as loaded values, which is the
   * point: every one is outlined, and the Save button is live, so what an
   * autotagger proposes is visible as a proposal before it is written.
   */
  initial?: Partial<Record<TagField, string>>;
  /**
   * Cover art the matched release came with.
   *
   * Offered here rather than embedded on the spot, because a match is a
   * proposal: the release the fields came from is the release the picture came
   * from, and if one of them is wrong so is the other. It goes in on Save, with
   * everything else, or not at all.
   */
  initialArt?: string;
  /**
   * Opens the cover search for this selection.
   *
   * A callback rather than a dialog of its own, because the search already
   * exists at the top level and two of them open at once would be two dialogs
   * arguing about the same files.
   */
  onFindArt?: () => void;
  /** Bumped by the app when artwork is written from somewhere else. */
  artNonce?: number;
}) {
  const [form, setForm] = createSignal<TagForm | null>(null);
  const [edits, setEdits] = createSignal<Partial<Record<TagField, string>>>(
    props.initial ?? {},
  );
  // Ticked to begin with: someone who took a match wants the release, and the
  // artwork is part of it. Unticking is one click and the picture is right there
  // to judge it by.
  const [embedArt, setEmbedArt] = createSignal(true);
  /** Whether Save will strip the artwork out of the selection. */
  const [dropArt, setDropArt] = createSignal(false);
  /** Whether the file turned out to have artwork to show at all. */
  const [hasArt, setHasArt] = createSignal(true);
  /**
   * Bumped to make the web view fetch the picture again.
   *
   * The cover URL names a file, not a picture, so nothing about it changes when
   * the picture behind it does.
   */
  const [artVersion, setArtVersion] = createSignal(0);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [report, setReport] = createSignal<WriteReport | null>(null);

  createEffect(() => {
    const ids = props.fileIds;
    readTags(ids)
      .then(setForm)
      .catch((e) => setError(String(e)));
  });

  // A file that had no artwork can acquire some while this is open, and the
  // "no artwork" state unmounts the image that would otherwise notice. So every
  // time the picture might have changed, assume there is one and let the load
  // fail again if there is not.
  createEffect(() => {
    props.artNonce;
    artVersion();
    setHasArt(true);
  });

  createEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        props.onClose();
      }
      // Ctrl+Enter saves from anywhere in the form, including a comment box
      // where a plain Enter has to mean a newline.
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        e.stopPropagation();
        save();
      }
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  /** What a box should show: the edit if there is one, else the loaded value. */
  const shown = (key: TagField): string => {
    const edited = edits()[key];
    if (edited !== undefined) return edited;
    return form()?.[key] ?? "";
  };

  /** Whether the selected files disagree about this field and it is untouched. */
  const various = (key: TagField): boolean =>
    edits()[key] === undefined && form() !== null && form()![key] === null;

  /** Whether the cover from the match is going to be written. */
  const artWanted = () => Boolean(props.initialArt) && embedArt();

  const dirty = () => Object.keys(edits()).length > 0 || artWanted() || dropArt();

  async function save() {
    if (!dirty() || busy()) return;
    setBusy(true);
    setError("");

    const fields = edits();
    let written = 0;
    let failures: WriteFailure[] = [];

    try {
      if (Object.keys(fields).length > 0) {
        const r = await writeTags(props.fileIds, fields);
        written = r.written;
        failures = r.failures;
      }

      // Artwork after the text, and only into the files the text reached. A
      // file that refused its tags is locked or read-only, and asking it for a
      // second rewrite produces a second identical complaint about the same
      // file.
      if (failures.length === 0 && artWanted()) {
        const r = await applyCover(props.fileIds, props.initialArt!);
        written = Math.max(written, r.written);
        failures = r.failures;
      } else if (failures.length === 0 && dropArt()) {
        const r = await removeCover(props.fileIds);
        written = Math.max(written, r.written);
        failures = r.failures;
      }
    } catch (e) {
      setError(String(e));
      setBusy(false);
      return;
    }

    setArtVersion((v) => v + 1);
    setDropArt(false);

    const result: WriteReport = { written, failures };
    setReport(result);
    setBusy(false);

    if (failures.length === 0) {
      props.onWritten();
      props.onClose();
      return;
    }
    // Some files were written and some were not. The dialog stays open showing
    // which failed — closing it would leave the user believing the whole
    // selection was retagged.
    props.onWritten();
    setEdits({});
    setEmbedArt(false);
    readTags(props.fileIds).then(setForm).catch(() => {});
  }

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="tagedit" role="dialog" aria-label="Edit metadata">
        <header class="sethead">
          <span class="settitle">Edit metadata</span>
          <span class="tagsubject">{form()?.subject ?? ""}</span>
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

        <div class="tagbody">
          <Show when={form()} fallback={<p class="sethint">Reading tags…</p>}>
            <div class="taggrid">
              <For each={TAG_FIELDS}>
                {([key, label]) => (
                  <label
                    class={`tagrow ${NARROW.includes(key) ? "narrow" : ""} ${
                      key === "comment" ? "tall" : ""
                    }`}
                  >
                    <span class="taglabel">{label}</span>
                    <Show
                      when={key === "comment"}
                      fallback={
                        <input
                          type="text"
                          class={edits()[key] !== undefined ? "edited" : ""}
                          value={shown(key)}
                          placeholder={various(key) ? "— various —" : ""}
                          spellcheck={false}
                          onInput={(e) =>
                            setEdits({ ...edits(), [key]: e.currentTarget.value })
                          }
                        />
                      }
                    >
                      <textarea
                        rows="2"
                        class={edits()[key] !== undefined ? "edited" : ""}
                        value={shown(key)}
                        placeholder={various(key) ? "— various —" : ""}
                        spellcheck={false}
                        onInput={(e) =>
                          setEdits({ ...edits(), [key]: e.currentTarget.value })
                        }
                      />
                    </Show>
                  </label>
                )}
              </For>
            </div>

          </Show>

          {/* Artwork, at a size worth judging — "is this the right cover, and
              is it worth keeping" is not a question a 16-pixel thumbnail in a
              list can answer, which is why it has to be looked at somewhere. */}
          <Show
            when={props.initialArt}
            fallback={
              <div class="artoffer">
                <span class={`artbox ${hasArt() ? "" : "empty"} ${dropArt() ? "going" : ""}`}>
                  <Show when={hasArt()} fallback={<span class="artnone">No artwork</span>}>
                    <img
                      src={fullArtUrl(
                        props.fileIds[0],
                        artVersion() + (props.artNonce ?? 0),
                      )}
                      alt=""
                      draggable={false}
                      onLoad={() => setHasArt(true)}
                      onError={() => setHasArt(false)}
                    />
                  </Show>
                </span>
                <span class="artoffertext">
                  <span class="artoffertitle">Cover art</span>
                  <span class="sethint">
                    <Show
                      when={dropArt()}
                      fallback={
                        <Show
                          when={hasArt()}
                          fallback="This file carries no picture."
                        >
                          <Show when={props.fileIds.length > 1} fallback="Embedded in the file.">
                            Embedded in the first of the {props.fileIds.length}{" "}
                            selected files.
                          </Show>
                        </Show>
                      }
                    >
                      Will be removed from{" "}
                      {props.fileIds.length > 1
                        ? `all ${props.fileIds.length} files`
                        : "the file"}{" "}
                      on Save.
                    </Show>
                  </span>
                  <span class="artbtns">
                    <button class="btn" onClick={() => props.onFindArt?.()}>
                      {hasArt() ? "Replace…" : "Find art…"}
                    </button>
                    <Show when={hasArt() || dropArt()}>
                      <button
                        class={`btn ${dropArt() ? "primary" : ""}`}
                        onClick={() => setDropArt((d) => !d)}
                      >
                        {dropArt() ? "Keep it" : "Remove"}
                      </button>
                    </Show>
                  </span>
                </span>
              </div>
            }
          >
            {/* The cover a match arrived with, which is a proposal rather than
                something the file has. */}
            <label class="artoffer">
              <span class="artbox">
                <img src={props.initialArt!} alt="" draggable={false} />
              </span>
              <span class="artoffertext">
                <span class="artoffertitle">
                  <input
                    type="checkbox"
                    checked={embedArt()}
                    onChange={(e) => setEmbedArt(e.currentTarget.checked)}
                  />
                  Embed this cover art
                </span>
              </span>
            </label>
          </Show>

          <Show when={error()}>
            <p class="tagerror">{error()}</p>
          </Show>

          <Show when={report() && report()!.failures.length > 0}>
            <div class="tagfailures">
              <p>
                Wrote {report()!.written}, failed {report()!.failures.length}:
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
          <button class="btn" onClick={props.onClose}>
            Cancel
          </button>
          <button
            class="btn primary"
            disabled={!dirty() || busy()}
            onClick={save}
            data-tip="ctrl+enter"
            data-tip-at="above end"
          >
            {busy() ? "Saving…" : "Save"}
          </button>
        </footer>
      </div>
    </>
  );
}
