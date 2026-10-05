import { For, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { CloseIcon } from "./icons";
import {
  createPlaylist,
  deletePlaylist,
  playlistTracks,
  playlists,
  removeFromPlaylist,
  renamePlaylist,
  reorderPlaylist,
  reveal,
  sendToRekordbox,
  type Playlist,
  type PlaylistTrack,
  type RowDisplay,
} from "./api";

/** `1:02:15` or `48:10`. */
export function totalTime(ms: number): string {
  const s = Math.round(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}

/**
 * Playlists: sets gathered from anywhere in the library, kept in the index, with
 * nothing copied.
 *
 * The playlist picked on the left is the one P adds to. Tracks play in the
 * order on screen, and are moved by dragging or Alt+↑↓.
 */
export default function Playlists(props: {
  current: number | null;
  onCurrent: (id: number | null) => void;
  onPlay: (row: RowDisplay, queue: RowDisplay[]) => void;
  playing: number | null;
  /** Bumped by the app when it changes a playlist, so the dialog reloads. */
  version: number;
  onChanged: () => void;
  onClose: () => void;
}) {
  const [lists, setLists] = createSignal<Playlist[]>([]);
  const [tracks, setTracks] = createSignal<PlaylistTrack[]>([]);
  const [picked, setPicked] = createSignal<Set<string>>(new Set());
  const [anchor, setAnchor] = createSignal<string | null>(null);
  const [naming, setNaming] = createSignal<{ id: number | null; name: string } | null>(null);
  const [deleting, setDeleting] = createSignal<number | null>(null);
  const [dragOver, setDragOver] = createSignal<number | null>(null);
  const [error, setError] = createSignal("");
  const [sent, setSent] = createSignal("");
  const [sending, setSending] = createSignal(false);
  let dragging: string[] = [];
  let listEl: HTMLDivElement | undefined;

  const current = () => lists().find((l) => l.id === props.current) ?? null;

  function loadLists() {
    playlists()
      .then((all) => {
        setLists(all);
        // No playlist picked yet, or the picked one was deleted: the first.
        if (!all.some((l) => l.id === props.current)) props.onCurrent(all[0]?.id ?? null);
      })
      .catch((e) => setError(String(e)));
  }

  function loadTracks() {
    const id = props.current;
    if (id === null) {
      setTracks([]);
      return;
    }
    playlistTracks(id)
      .then((t) => {
        if (props.current === id) setTracks(t);
      })
      .catch((e) => setError(String(e)));
  }

  createEffect(() => {
    void props.version;
    loadLists();
  });
  createEffect(() => {
    void props.version;
    void props.current;
    loadTracks();
  });

  function changed() {
    loadLists();
    loadTracks();
    props.onChanged();
  }

  function pick(id: number) {
    if (id === props.current) return;
    setPicked(new Set<string>());
    setAnchor(null);
    props.onCurrent(id);
  }

  let saving = false;
  function saveName() {
    const n = naming();
    if (!n || saving) return;
    saving = true;
    const done = n.id === null ? createPlaylist(n.name) : renamePlaylist(n.id, n.name).then(() => n.id!);
    done
      .then((id) => {
        setNaming(null);
        setError("");
        props.onCurrent(id);
        changed();
      })
      .catch((e) => setError(String(e)))
      .finally(() => (saving = false));
  }

  /** Delete is two clicks on the same ×: the first arms it, the second deletes. */
  function remove(id: number) {
    if (deleting() !== id) {
      setDeleting(id);
      window.setTimeout(() => setDeleting((d) => (d === id ? null : d)), 3000);
      return;
    }
    setDeleting(null);
    deletePlaylist(id)
      .then(changed)
      .catch((e) => setError(String(e)));
  }

  function send() {
    if (sending()) return;
    setSending(true);
    setError("");
    setSent("");
    sendToRekordbox()
      .then((s) => {
        const parts = [`Sent ${s.playlists} ${s.playlists === 1 ? "playlist" : "playlists"}`];
        if (s.added > 0) parts.push(`${s.added} new ${s.added === 1 ? "track" : "tracks"}`);
        if (s.cued > 0) parts.push(`${s.cued} cued`);
        if (s.missing > 0) parts.push(`${s.missing} not found`);
        setSent(`${parts.join(" · ")} · Reload in rekordbox`);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setSending(false));
  }

  /* ── Tracks ─────────────────────────────────────────────────────────── */

  const order = () => tracks().map((t) => t.path);
  const playable = () => tracks().flatMap((t) => (t.row ? [t.row] : []));
  /** The picked paths, in playlist order. */
  const pickedInOrder = () => order().filter((p) => picked().has(p));

  function click(e: MouseEvent, path: string) {
    const next = new Set(e.ctrlKey ? picked() : []);
    if (e.shiftKey && anchor()) {
      const all = order();
      const [a, b] = [all.indexOf(anchor()!), all.indexOf(path)].sort((x, y) => x - y);
      all.slice(a, b + 1).forEach((p) => next.add(p));
    } else if (e.ctrlKey && picked().has(path)) {
      next.delete(path);
    } else {
      next.add(path);
      setAnchor(path);
    }
    setPicked(next);
  }

  function play(track: PlaylistTrack) {
    if (track.row) props.onPlay(track.row, playable());
  }

  function takeOut() {
    const id = props.current;
    const paths = pickedInOrder();
    if (id === null || paths.length === 0) return;
    // The row after the last one taken out is picked, so Del can be held.
    const all = order();
    const after = all.slice(all.indexOf(paths[paths.length - 1]) + 1).find((p) => !picked().has(p));
    removeFromPlaylist(id, paths)
      .then(() => {
        setPicked(new Set(after ? [after] : []));
        setAnchor(after ?? null);
        changed();
      })
      .catch((e) => setError(String(e)));
  }

  /** Moves the picked tracks one place up or down, as a block. */
  function nudge(by: -1 | 1) {
    const id = props.current;
    const paths = pickedInOrder();
    if (id === null || paths.length === 0) return;
    const all = order();
    const rest = all.filter((p) => !picked().has(p));
    const first = all.indexOf(paths[0]);
    // Where the block starts among the rest, then one step.
    const before = all.slice(0, first).filter((p) => !picked().has(p)).length;
    const to = Math.max(0, Math.min(rest.length, before + by));
    reorderPlaylist(id, paths, to).then(changed).catch((e) => setError(String(e)));
  }

  function drop(at: number) {
    const id = props.current;
    const paths = dragging;
    dragging = [];
    setDragOver(null);
    if (id === null || paths.length === 0) return;
    // `at` counts every row; the backend counts without the ones moving.
    const to = order().slice(0, at).filter((p) => !paths.includes(p)).length;
    reorderPlaylist(id, paths, to).then(changed).catch((e) => setError(String(e)));
  }

  function moveCursor(by: number, extend: boolean) {
    const all = order();
    if (all.length === 0) return;
    const from = anchor() ? all.indexOf(anchor()!) : -1;
    const to = Math.max(0, Math.min(all.length - 1, from + by));
    const path = all[to];
    if (extend && anchor()) {
      setPicked(new Set([...picked(), path]));
    } else {
      setPicked(new Set([path]));
    }
    setAnchor(path);
    listEl?.querySelector(`[data-at="${to}"]`)?.scrollIntoView({ block: "nearest" });
  }

  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        if (naming()) setNaming(null);
        else props.onClose();
        return;
      }
      if (typing) return;
      // Everything else the list answers to stays here: the browser behind the
      // dialog must not move or delete anything while it is open.
      if (e.key === "Delete") {
        e.preventDefault();
        e.stopPropagation();
        takeOut();
      } else if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
        e.preventDefault();
        e.stopPropagation();
        nudge(e.key === "ArrowUp" ? -1 : 1);
      } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        e.preventDefault();
        e.stopPropagation();
        moveCursor(e.key === "ArrowUp" ? -1 : 1, e.shiftKey);
      } else if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        const t = tracks().find((t) => t.path === anchor());
        if (t) play(t);
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
        e.preventDefault();
        e.stopPropagation();
        setPicked(new Set(order()));
      } else if (e.key === "F2" && current()) {
        e.preventDefault();
        e.stopPropagation();
        setNaming({ id: current()!.id, name: current()!.name });
      }
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  const nameInput = () => (
    <input
      class="renameinput plname"
      type="text"
      spellcheck={false}
      placeholder="Playlist name"
      value={naming()?.name ?? ""}
      ref={(el) =>
        queueMicrotask(() => {
          el.focus();
          el.select();
        })
      }
      onInput={(e) => {
        setNaming({ ...naming()!, name: e.currentTarget.value });
        setError("");
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          saveName();
        }
      }}
      onBlur={() => {
        if (naming()?.name.trim()) saveName();
        else setNaming(null);
      }}
    />
  );

  return (
    <>
      <div class="scrim" onPointerDown={props.onClose} />
      <div class="renamer wide playlists" role="dialog" aria-label="Playlists">
        <header class="sethead">
          <span class="settitle">Playlists</span>
          <span class="tagsubject">
            <Show when={current()}>
              {(c) => (
                <>
                  {c().tracks} {c().tracks === 1 ? "track" : "tracks"} · {totalTime(c().durationMs)}
                </>
              )}
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

        <div class="plbody">
          <nav class="pllists">
            <For each={lists()}>
              {(list) => (
                <Show
                  when={naming()?.id === list.id}
                  fallback={
                    <div
                      class="plitem"
                      classList={{ on: list.id === props.current }}
                      onClick={() => pick(list.id)}
                      onDblClick={() => setNaming({ id: list.id, name: list.name })}
                      data-tip="Double-click to rename"
                    >
                      <span class="plitemname">{list.name}</span>
                      <span class="plcount mono">{list.tracks}</span>
                      <button
                        class="pldel"
                        classList={{ armed: deleting() === list.id }}
                        onClick={(e) => {
                          e.stopPropagation();
                          remove(list.id);
                        }}
                        aria-label={`Delete ${list.name}`}
                      >
                        {deleting() === list.id ? "Delete?" : <CloseIcon />}
                      </button>
                    </div>
                  }
                >
                  {nameInput()}
                </Show>
              )}
            </For>
            <Show
              when={naming()?.id === null}
              fallback={
                <button class="plnew" onClick={() => setNaming({ id: null, name: "" })}>
                  + New playlist
                </button>
              }
            >
              {nameInput()}
            </Show>
          </nav>

          <div class="pltracks" ref={(el) => (listEl = el)}>
            <Show
              when={current()}
              fallback={<p class="sethint plempty">Make a playlist, then press P on tracks to add them.</p>}
            >
              <Show
                when={tracks().length > 0}
                fallback={
                  <p class="sethint plempty">
                    Empty. Select tracks and press P, or right-click → Playlists.
                  </p>
                }
              >
                <For each={tracks()}>
                  {(track, i) => (
                    <div
                      class="plrow"
                      data-at={i()}
                      classList={{
                        sel: picked().has(track.path),
                        missing: !track.row,
                        playing: !!track.row && track.row.fileId === props.playing,
                        dropabove: dragOver() === i(),
                      }}
                      draggable={true}
                      onDragStart={(e) => {
                        if (!picked().has(track.path)) {
                          setPicked(new Set([track.path]));
                          setAnchor(track.path);
                        }
                        dragging = pickedInOrder();
                        e.dataTransfer?.setData("text/plain", "");
                      }}
                      onDragOver={(e) => {
                        if (dragging.length === 0) return;
                        e.preventDefault();
                        const box = e.currentTarget.getBoundingClientRect();
                        setDragOver(e.clientY < box.top + box.height / 2 ? i() : i() + 1);
                      }}
                      onDrop={(e) => {
                        e.preventDefault();
                        drop(dragOver() ?? i());
                      }}
                      onDragEnd={() => {
                        dragging = [];
                        setDragOver(null);
                      }}
                      onClick={(e) => click(e, track.path)}
                      onDblClick={() => play(track)}
                      title={track.path}
                    >
                      <span class="plno mono">{i() + 1}</span>
                      <Show
                        when={track.row}
                        fallback={
                          <span class="pltitle">
                            {track.path.split(/[\\/]/).pop()}
                            <span class="plgone"> · not found</span>
                          </span>
                        }
                      >
                        {(row) => (
                          <span class="pltitle">
                            <Show when={row().artist}>
                              <span class="plartist">{row().artist}</span>
                              {" – "}
                            </Show>
                            {row().title || row().name}
                          </span>
                        )}
                      </Show>
                      <span class="plbpm mono">{track.row?.bpm || track.row?.detectedBpm}</span>
                      <span class="plkey mono">{track.row?.key || track.row?.detectedKey}</span>
                      <span class="pltime mono">{track.row?.duration}</span>
                    </div>
                  )}
                </For>
                <div
                  class="pltail"
                  classList={{ dropabove: dragOver() === tracks().length }}
                  onDragOver={(e) => {
                    if (dragging.length === 0) return;
                    e.preventDefault();
                    setDragOver(tracks().length);
                  }}
                  onDrop={(e) => {
                    e.preventDefault();
                    drop(tracks().length);
                  }}
                />
              </Show>
            </Show>
          </div>
        </div>

        <footer class="tagfoot">
          <span class="sethint credit">
            <Show
              when={error()}
              fallback={sent() || "Enter plays · Del removes · Alt+↑↓ or drag to move"}
            >
              <span class="tagerror">{error()}</span>
            </Show>
          </span>
          <button
            class="btn"
            disabled={sending() || lists().length === 0}
            onClick={send}
            data-tip="Into rekordbox.xml's noceur folder; new tracks get cue points"
          >
            {sending() ? "Cueing and sending…" : "Send to rekordbox"}
          </button>
          <button
            class="btn"
            disabled={pickedInOrder().length !== 1 || !tracks().find((t) => t.path === pickedInOrder()[0])?.row}
            onClick={() => {
              const row = tracks().find((t) => t.path === pickedInOrder()[0])?.row;
              if (row) reveal(row.fileId).catch(() => {});
            }}
          >
            Reveal
          </button>
          <button class="btn" onClick={props.onClose}>
            Close
          </button>
        </footer>
      </div>
    </>
  );
}
