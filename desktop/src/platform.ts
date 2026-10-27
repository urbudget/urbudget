import { msg } from "./i18n";

/** The system the window runs on, from the webview's user agent (WebView2, WKWebView, WebKitGTK). */
export type Os = "macos" | "windows" | "linux" | "other";

export function osFrom(userAgent: string): Os {
  if (/Windows/.test(userAgent)) return "windows";
  if (/Macintosh|Mac OS X/.test(userAgent)) return "macos";
  if (/Linux|X11/.test(userAgent)) return "linux";
  return "other";
}

export const os: Os = typeof navigator === "undefined" ? "other" : osFrom(navigator.userAgent);

const TOKEN_STORES: Partial<Record<Os, string>> = {
  macos: "Keychain",
  // Translated where it's shown; Keychain is Apple's name for it.
  windows: msg("Windows encrypted storage"),
};
/** What protects a remembered YNAB token (see vault.rs), or null where it can't be kept. */
export const tokenStoreFor = (system: Os) => TOKEN_STORES[system] ?? null;
export const tokenStore = tokenStoreFor(os);

const MAC_MODIFIERS: Record<string, string> = { Ctrl: "⌘", Alt: "⌥", Shift: "⇧" };
const MAC_ORDER = "⌥⇧⌘";

/**
 * A shortcut written for Windows ("Ctrl+Shift+Z", "Ctrl+click") as this system shows it. On
 * macOS, where the app takes ⌘ for Ctrl, that is "⇧⌘Z" and "⌘-click"; elsewhere it is unchanged.
 */
export function keyLabel(keys: string, system: Os = os): string {
  if (system !== "macos") return keys;
  return keys.replace(/\b(?:(?:Ctrl|Alt|Shift)\+)+[^\s,/)]+/g, (combo) => {
    const parts = combo.split("+");
    const key = parts.pop()!;
    const mods = parts
      .map((m) => MAC_MODIFIERS[m])
      .sort((a, b) => MAC_ORDER.indexOf(a) - MAC_ORDER.indexOf(b))
      .join("");
    return /^[a-z]/.test(key) ? `${mods}-${key}` : `${mods}${key}`;
  });
}
