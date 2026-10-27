import { useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { os } from "./platform";
import { t } from "./i18n";

/**
 * On Windows the app draws its own title bar in place of the system one (turned off in
 * tauri.windows.conf.json). Elsewhere the system title bar stays.
 */
export const customTitleBar = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window && os === "windows";

const NAME = "UrBudget";

/** The app's mark (assets/icon.svg): "Ur" in one stroke, the U a pocket with a stack of coins saved in it. */
function AppMark() {
  return (
    <svg className="titlebar-mark" viewBox="40 40 944 944" aria-hidden="true">
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
 * another window has focus.
 */
export function TitleBar({ children }: { children?: ReactNode }) {
  const [title, setTitle] = useState(NAME);
  const [focused, setFocused] = useState(true);
  useEffect(() => {
    const w = getCurrentWindow();
    let stop: (() => void) | undefined;
    w.title()
      .then((t) => t && setTitle(t))
      .catch(() => {});
    w.onFocusChanged(({ payload }) => setFocused(payload))
      .then((unlisten) => (stop = unlisten))
      .catch(() => {});
    return () => stop?.();
  }, []);
  return (
    <div className={`titlebar${focused ? "" : " inactive"}`} data-tauri-drag-region="deep">
      {title.startsWith(NAME) ? (
        // The mark alone stands for the name; anything the window adds after the name (a preview
        // build's label) follows it.
        <div className="titlebar-name wordmark" title={title}>
          <AppMark />
          <span className="sr-only">{NAME}</span>
          {title.length > NAME.length && <span className="titlebar-label">{title.slice(NAME.length)}</span>}
        </div>
      ) : (
        <div className="titlebar-name">
          <AppMark />
          <span>{title}</span>
        </div>
      )}
      {children && <div className="titlebar-actions">{children}</div>}
      <WindowControls />
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
