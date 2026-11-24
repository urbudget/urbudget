import { useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { os, type Os } from "./platform";
import { t } from "./i18n";

/**
 * On Windows and macOS the app draws its own title bar. Windows' system bar is turned off
 * (tauri.windows.conf.json); macOS's is see-through and keeps only its traffic lights, which sit
 * over the left end of the app's bar (tauri.macos.conf.json). Linux keeps the system title bar.
 */
export const customTitleBar =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window && (os === "windows" || os === "macos");

const NAME = "UrBudget";

/** The app's mark (assets/icon.svg): "Ur" in one stroke, the U a pocket with a stack of coins saved in it. */
export function AppMark({ className = "titlebar-mark" }: { className?: string }) {
  return (
    <svg className={className} viewBox="40 40 944 944" aria-hidden="true">
      <rect x="40" y="40" width="944" height="944" rx="212" fill="#6d4ae8" />
      <g transform="translate(-33 0)">
        <path
          d="M300 330V620a110 110 0 0 0 110 110h120a110 110 0 0 0 110-110V430c0-80 55-130 150-128"
          fill="none"
          stroke="#fff"
          strokeWidth="110"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <g fill="#fbbf24">
          <rect x="382" y="595" width="172" height="60" rx="30" />
          <rect x="396" y="501" width="172" height="60" rx="30" />
          <rect x="374" y="407" width="172" height="60" rx="30" />
        </g>
      </g>
    </svg>
  );
}

/**
 * A strip in the sidebar's color across the top of the window: the window's name on the left,
 * the app's own buttons (`children`) beside the window buttons on the right. Anywhere that isn't
 * a button moves the window; a double-click maximizes. Like the system bar, it dims while
 * another window has focus. On macOS the system's traffic lights take the place of the window
 * buttons, at the bar's left end; full screen hides them, and the bar gives their room back.
 */
export function TitleBar({ children, system = os }: { children?: ReactNode; system?: Os }) {
  const mac = system === "macos";
  const [title, setTitle] = useState(NAME);
  const [focused, setFocused] = useState(true);
  const [fullscreen, setFullscreen] = useState(false);
  useEffect(() => {
    const w = getCurrentWindow();
    const stops: (() => void)[] = [];
    let gone = false;
    const keep = (unlisten: () => void) => (gone ? unlisten() : stops.push(unlisten));
    w.title()
      .then((t) => t && setTitle(t))
      .catch(() => {});
    w.onFocusChanged(({ payload }) => setFocused(payload))
      .then(keep)
      .catch(() => {});
    if (mac) {
      const update = () => void w.isFullscreen().then(setFullscreen).catch(() => {});
      update();
      w.onResized(update)
        .then(keep)
        .catch(() => {});
    }
    return () => {
      gone = true;
      stops.forEach((stop) => stop());
    };
  }, [mac]);
  return (
    <div
      className={`titlebar${mac ? " mac" : ""}${mac && fullscreen ? " fullscreen" : ""}${focused ? "" : " inactive"}`}
      data-tauri-drag-region="deep"
    >
      {title.startsWith(NAME) ? (
        // The mark and the wordmark, its "Ur" in the mark's purple; anything the window adds after
        // the name (a preview build's label) follows them.
        <div className="titlebar-name wordmark" title={title}>
          <AppMark />
          <span className="titlebar-wordmark">
            <span className="titlebar-ur">{NAME.slice(0, 2)}</span>
            {NAME.slice(2)}
          </span>
          {title.length > NAME.length && <span className="titlebar-label">{title.slice(NAME.length)}</span>}
        </div>
      ) : (
        <div className="titlebar-name">
          <AppMark />
          <span>{title}</span>
        </div>
      )}
      {children && <div className="titlebar-actions">{children}</div>}
      {!mac && <WindowControls />}
    </div>
  );
}

/** Minimize, maximize and close, drawn with the system's own glyphs (Segoe Fluent Icons). */
function WindowControls() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const w = getCurrentWindow();
    let stop: (() => void) | undefined;
    const update = () => void w.isMaximized().then(setMaximized).catch(() => {});
    update();
    w.onResized(update)
      .then((unlisten) => (stop = unlisten))
      .catch(() => {});
    return () => stop?.();
  }, []);
  return (
    <div className="window-controls">
      <button aria-label={t("Minimize")} title={t("Minimize")} onClick={() => void getCurrentWindow().minimize()}>
        <span aria-hidden="true">{""}</span>
      </button>
      <button
        aria-label={maximized ? t("Restore") : t("Maximize")}
        title={maximized ? t("Restore") : t("Maximize")}
        onClick={() => void getCurrentWindow().toggleMaximize()}
      >
        <span aria-hidden="true">{maximized ? "" : ""}</span>
      </button>
      {/* Close goes through the same save-first handling as the system close button. */}
      <button className="close" aria-label={t("Close")} title={t("Close")} onClick={() => void getCurrentWindow().close()}>
        <span aria-hidden="true">{""}</span>
      </button>
    </div>
  );
}
