import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { CloseIcon } from "./icons";
import {
  applyCover,
  findCovers,
  type CoverCandidate,
  type WriteReport,
} from "./api";

/**
 * Picks cover art for a selection.
 *
 * Shows candidates rather than guessing. Which pressing a track came from is a
 * judgement only the user can make — a compilation, a remaster and a bootleg all
 * match the same search and carry different sleeves — so nothing is written to
 * any file until one is chosen.
 *
 * Several sources are searched at once and their results interleaved, because
 * their catalogues barely overlap: a twelve-inch missing from one is routinely
 * in another, and the same record can carry a different sleeve depending on who
 * is asked. Candidates whose image fails to load are hidden as they fail, since
 * the databases know about far more releases than anyone holds pictures of.
 */
export default function CoverPicker(props: {
  /** The row the search is based on — normally the first of the selection. */
  fileId: number;
  /** Every file the chosen cover gets written to. */
  fileIds: number[];
  onClose: () => void;
  onApplied: () => void;
}) {
  const [candidates, setCandidates] = createSignal<CoverCandidate[] | null>(null);
  const [missing, setMissing] = createSignal<Set<string>>(new Set());
  /** Which source's results to show, or "" for all of them. */
  const [only, setOnly] = createSignal("");
  const [busy, setBusy] = createSignal("");
  const [error, setError] = createSignal("");
  const [report, setReport] = createSignal<WriteReport | null>(null);

  createEffect(() => {
    const id = props.fileId;
    findCovers(id)
      .then(setCandidates)
      .catch((e) => {
        setCandidates([]);
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

  /** Candidates whose thumbnail actually loaded, in the chosen source. */
  const withArt = () => (candidates() ?? []).filter((c) => !missing().has(c.full));
  const shown = () => withArt().filter((c) => !only() || c.source === only());

  /** Sources that actually returned something with a picture behind it. */
  const sources = () => {
    const seen: string[] = [];
    for (const c of withArt()) if (!seen.includes(c.source)) seen.push(c.source);
    return seen;
  };

  function choose(candidate: CoverCandidate) {
    if (busy()) return;
    setBusy(candidate.full);
    setError("");
    applyCover(props.fileIds, candidate.full)
      .then((r) => {
        setBusy("");
        setReport(r);
        props.onApplied();
        if (r.failures.length === 0) props.onClose();
      })
      .catch((e) => {
        setBusy("");
        setError(String(e));
      });
  }

  const subtitle = () =>
    props.fileIds.length > 1
      ? `Applies to ${props.fileIds.length} tracks`
      : "Applies to this track";

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="covers" role="dialog" aria-label="Find album art">
        <header class="sethead">
          <span class="settitle">Album art</span>
          <span class="tagsubject">{subtitle()}</span>
          <button class="setclose" onClick={props.onClose} title="Close (Esc)">
            <CloseIcon />
          </button>
        </header>

        <div class="coverbody">
          <Show
            when={candidates()}
            fallback={<p class="sethint">Searching…</p>}
          >
            <Show when={sources().length > 1}>
              <div class="setseg sourcetabs" role="group">
                <button
                  class={`segbtn ${only() === "" ? "on" : ""}`}
                  onClick={() => setOnly("")}
                >
                  All
                </button>
                <For each={sources()}>
                  {(name) => (
                    <button
                      class={`segbtn ${only() === name ? "on" : ""}`}
                      onClick={() => setOnly(name)}
                    >
                      {name}
                    </button>
                  )}
                </For>
              </div>
            </Show>

            <Show
              when={shown().length > 0}
              fallback={
                <p class="sethint">
                  Nothing found with artwork. The search uses the track's album
                  and artist tags — filling those in first usually finds it.
                </p>
              }
            >
              <div class="covergrid">
                <For each={shown()}>
                  {(c) => (
                    <button
                      class={`covercard ${busy() === c.full ? "busy" : ""}`}
                      disabled={Boolean(busy())}
                      onClick={() => choose(c)}
                      title={`${c.artist} — ${c.title} · ${c.source}`}
                    >
                      <img
                        src={c.thumbnail}
                        alt=""
                        loading="lazy"
                        draggable={false}
                        // Hidden rather than shown broken: the archive holds art
                        // for only a fraction of what the search returns.
                        onError={() => setMissing(new Set([...missing(), c.full]))}
                      />
                      <span class="covertitle">{c.title || "Untitled"}</span>
                      <span class="covermeta">{c.detail}</span>
                      <span class="coversource">{c.source}</span>
                    </button>
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
          <span class="sethint credit">
            Covers from the Cover Art Archive, Apple and Deezer.
          </span>
          <button class="btn" onClick={props.onClose}>
            Close
          </button>
        </footer>
      </div>
    </>
  );
}
