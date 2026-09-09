import { createSignal, onCleanup, onMount } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * The window's own title bar, replacing the one Windows draws.
 *
 * The system bar is the wrong height, the wrong colour, and cannot be made to
 * match anything else here — on a dark monochrome window it is a strip of
 * someone else's design across the top. Drawing it means owning what it did:
 * dragging the window, the three buttons, and double-click to maximise.
 *
 * The close button keeps the system's red hover rather than the app accent.
 * That colour is not decoration — it is the one piece of a title bar people hit
 * without looking, and a window whose close button is the same colour as its
 * minimise button is a window you close by accident.
 */
export default function Titlebar() {
  const [maximized, setMaximized] = createSignal(false);

  onMount(() => {
    const win = getCurrentWindow();
    let stop: (() => void) | undefined;

    win.isMaximized().then(setMaximized).catch(() => {});
    // The window can also be maximised by the OS — Win+Up, snapping to an edge,
    // a double-click on the drag region — so the button follows the window
    // rather than only what was clicked here.
    win
      .onResized(() => {
        win.isMaximized().then(setMaximized).catch(() => {});
      })
      .then((unlisten) => {
        stop = unlisten;
      })
      .catch(() => {});

    onCleanup(() => stop?.());
  });

  const win = () => getCurrentWindow();

  return (
    <div class="titlebar" data-tauri-drag-region>
      <span class="tbmark" data-tauri-drag-region>
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
          <path
            fill="var(--acc)"
            d="M8 1.6 14.2 13a.5.5 0 0 1-.44.75H2.24A.5.5 0 0 1 1.8 13Z"
            opacity="0.9"
          />
        </svg>
      </span>
      <span class="tbname" data-tauri-drag-region>
        Noceur
      </span>
      <span class="tbdrag" data-tauri-drag-region />

      <div class="tbbtns">
        <button
          class="tbbtn"
          data-tip="Minimise"
          aria-label="Minimise"
          onClick={() => win().minimize().catch(() => {})}
        >
          <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
            <path d="M0 5h10" stroke="currentColor" stroke-width="1" />
          </svg>
        </button>
        <button
          class="tbbtn"
          data-tip={maximized() ? "Restore" : "Maximise"}
          aria-label={maximized() ? "Restore" : "Maximise"}
          onClick={() => win().toggleMaximize().catch(() => {})}
        >
          <svg viewBox="0 0 10 10" width="10" height="10" fill="none" aria-hidden="true">
            {maximized() ? (
              <>
                <rect x="0.5" y="2.5" width="7" height="7" stroke="currentColor" />
                <path d="M2.5 2.5V0.5h7v7h-2" stroke="currentColor" />
              </>
            ) : (
              <rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" />
            )}
          </svg>
        </button>
        <button
          class="tbbtn close"
          data-tip="Close"
          data-tip-at="end"
          aria-label="Close"
          onClick={() => win().close().catch(() => {})}
        >
          <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
            <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" stroke-width="1" />
          </svg>
        </button>
      </div>
    </div>
  );
}
