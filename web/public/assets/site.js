// Runs on every page: the header menu, the current-page link, the footer year, the report
// tabs and price labels. Also exports small helpers the page scripts share.

import { getConfig } from "./api.js";

/** Shows a message in a live region. kind: "" | "error" | "success". */
export function setStatus(element, message, kind = "") {
  if (!element) return;
  element.textContent = message || "";
  element.classList.toggle("is-error", kind === "error");
  element.classList.toggle("is-success", kind === "success");
}

/** Copies text and briefly relabels the button ("Copied"). */
export async function copyText(text, button) {
  let ok = false;
  try {
    await navigator.clipboard.writeText(text);
    ok = true;
  } catch {
    // Older browsers, or no permission: select a temporary field and copy that.
    const field = document.createElement("textarea");
    field.value = text;
    field.setAttribute("readonly", "");
    field.style.position = "fixed";
    field.style.opacity = "0";
    document.body.append(field);
    field.select();
    try {
      ok = document.execCommand("copy");
    } catch {
      ok = false;
    }
    field.remove();
  }
  if (button) {
    const label = button.querySelector("[data-label]") || button;
    const original = label.dataset.original || label.textContent;
    label.dataset.original = original;
    label.textContent = ok ? "Copied" : "Press Ctrl+C to copy";
    clearTimeout(button._copyTimer);
    button._copyTimer = setTimeout(() => (label.textContent = original), 2000);
  }
  return ok;
}

/** "October 16, 2026" from an ISO date or timestamp; "" if it can't be read. */
export function formatDate(value) {
  if (!value) return "";
  const date = new Date(typeof value === "number" && value < 1e12 ? value * 1000 : value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleDateString("en-US", { year: "numeric", month: "long", day: "numeric" });
}

/** "84.2 MB" */
export function formatBytes(bytes) {
  const n = Number(bytes);
  if (!Number.isFinite(n) || n < 0) return "";
  if (n < 1024) return `${n} bytes`;
  const units = ["KB", "MB", "GB"];
  let value = n / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

export const params = new URLSearchParams(location.search);

/** Removes query parameters from the address bar without reloading. */
export function dropParams(...names) {
  const url = new URL(location.href);
  let changed = false;
  for (const name of names) {
    if (url.searchParams.has(name)) {
      url.searchParams.delete(name);
      changed = true;
    }
  }
  if (changed) history.replaceState(history.state, "", url.pathname + url.search + url.hash);
}

/* ---------- Header ---------- */

function header() {
  const bar = document.querySelector(".site-header");
  if (!bar) return;
  const button = bar.querySelector(".menu-button");
  const menu = bar.querySelector(".site-menu");

  const setOpen = (open) => {
    bar.classList.toggle("is-open", open);
    button?.setAttribute("aria-expanded", String(open));
    button?.setAttribute("aria-label", open ? "Close menu" : "Menu");
  };
  button?.addEventListener("click", () => setOpen(!bar.classList.contains("is-open")));
  menu?.addEventListener("click", (event) => {
    if (event.target.closest("a")) setOpen(false);
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && bar.classList.contains("is-open")) {
      setOpen(false);
      button?.focus();
    }
  });
  document.addEventListener("click", (event) => {
    if (bar.classList.contains("is-open") && !bar.contains(event.target)) setOpen(false);
  });
  matchMedia("(min-width: 860px)").addEventListener("change", (event) => {
    if (event.matches) setOpen(false);
  });

  const onScroll = () => bar.classList.toggle("is-scrolled", window.scrollY > 4);
  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  // Mark the link for this page.
  const here = location.pathname.replace(/index\.html$/, "");
  for (const link of document.querySelectorAll(".site-menu a, .footer-nav a, .legal-nav a")) {
    const target = new URL(link.href, location.href);
    if (target.origin === location.origin && !target.hash && target.pathname === here && here !== "/") {
      link.setAttribute("aria-current", "page");
    }
  }
}

/* ---------- Header: the Compare and Learn menus ---------- */

// A button that shows or hides a panel of links. Click, Enter or Space toggles it; with a mouse it
// also opens on hover. Opening by hover then clicking keeps it open rather than closing it. Esc,
// a click elsewhere or focus leaving closes it, and opening one menu closes the other. On narrow
// screens the panel is part of the open menu and always shown (site.css), so the button does
// nothing there.
function navGroups() {
  const narrow = matchMedia("(max-width: 859.98px)");
  const mouse = matchMedia("(hover: hover) and (pointer: fine)");
  const closers = [];
  for (const group of document.querySelectorAll(".nav-group")) {
    const button = group.querySelector(".nav-group-button");
    if (!button) continue;
    let timer = 0;
    let pinned = false;
    const isOpen = () => group.classList.contains("is-open");
    const setOpen = (open, pin = false) => {
      clearTimeout(timer);
      pinned = open && (pin || pinned);
      group.classList.toggle("is-open", open);
      button.setAttribute("aria-expanded", String(open || narrow.matches));
      button.tabIndex = narrow.matches ? -1 : 0;
      if (open) for (const other of closers) if (other !== close) other();
    };
    const close = () => setOpen(false);
    closers.push(close);
    button.addEventListener("click", () => {
      if (narrow.matches) return;
      if (isOpen() && !pinned) setOpen(true, true);
      else setOpen(!isOpen(), true);
    });
    group.addEventListener("mouseenter", () => {
      if (!mouse.matches || narrow.matches) return;
      clearTimeout(timer);
      if (!isOpen()) setOpen(true);
    });
    group.addEventListener("mouseleave", () => {
      if (!mouse.matches || narrow.matches || pinned) return;
      timer = setTimeout(() => setOpen(false), 200);
    });
    group.addEventListener("keydown", (event) => {
      if (event.key !== "Escape" || !isOpen()) return;
      event.stopPropagation();
      setOpen(false);
      button.focus();
    });
    group.addEventListener("focusout", (event) => {
      if (!group.contains(event.relatedTarget)) setOpen(false);
    });
    document.addEventListener("click", (event) => {
      if (isOpen() && !group.contains(event.target)) setOpen(false);
    });
    narrow.addEventListener("change", () => setOpen(false));
    setOpen(false);
  }
}

/* ---------- Tabs ---------- */

function tabs() {
  for (const root of document.querySelectorAll("[data-tabs]")) {
    const list = root.querySelector('[role="tablist"]');
    const buttons = [...root.querySelectorAll('[role="tab"]')];
    const select = (button, focus = false) => {
      for (const other of buttons) {
        const selected = other === button;
        other.setAttribute("aria-selected", String(selected));
        other.tabIndex = selected ? 0 : -1;
        const panel = document.getElementById(other.getAttribute("aria-controls"));
        if (panel) panel.hidden = !selected;
      }
      if (focus) button.focus();
    };
    for (const button of buttons) button.addEventListener("click", () => select(button));
    list?.addEventListener("keydown", (event) => {
      const index = buttons.indexOf(document.activeElement);
      if (index < 0) return;
      let next = null;
      if (event.key === "ArrowRight" || event.key === "ArrowDown") next = buttons[(index + 1) % buttons.length];
      if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = buttons[(index - 1 + buttons.length) % buttons.length];
      if (event.key === "Home") next = buttons[0];
      if (event.key === "End") next = buttons[buttons.length - 1];
      if (next) {
        event.preventDefault();
        select(next, true);
      }
    });
  }
}

/* ---------- Prices ---------- */

async function prices() {
  const slots = document.querySelectorAll("[data-price]");
  if (!slots.length) return;
  const config = await getConfig();
  for (const slot of slots) {
    const label = config.prices[slot.dataset.price];
    if (typeof label === "string" && label.trim()) slot.textContent = label.trim();
  }
}

function year() {
  const now = String(new Date().getFullYear());
  for (const slot of document.querySelectorAll("[data-year]")) {
    if (Number(now) > Number(slot.textContent)) slot.textContent = now;
  }
}

// The header's light/dark switch. The site is light unless the visitor picks dark; the choice is
// kept in localStorage, and each page's head script applies it before anything is drawn (the CSP
// in _headers allows that script by its hash, so change both together).
function themeToggle() {
  const root = document.documentElement;
  const button = document.querySelector("[data-theme-toggle]");
  const color = document.querySelector('meta[name="theme-color"]');
  const show = () => {
    const dark = root.dataset.theme === "dark";
    if (color) color.content = dark ? "#0f0e14" : "#fbfaf7";
    if (!button) return;
    const label = dark ? "Light mode" : "Dark mode";
    button.setAttribute("aria-label", `Switch to ${label.toLowerCase()}`);
    button.title = `Switch to ${label.toLowerCase()}`;
    button.querySelector(".theme-toggle-label").textContent = label;
  };
  const apply = (theme) => {
    root.dataset.theme = theme;
    show();
  };
  button?.addEventListener("click", () => {
    const theme = root.dataset.theme === "dark" ? "light" : "dark";
    apply(theme);
    try {
      if (theme === "dark") localStorage.setItem("theme", "dark");
      else localStorage.removeItem("theme");
    } catch {
      // Storage blocked: the switch still works on this page.
    }
  });
  // Another tab switched.
  addEventListener("storage", (event) => {
    if (event.key === "theme" || event.key === null) apply(event.newValue === "dark" ? "dark" : "light");
  });
  show();
}

// One beacon per page for the admin page's daily page views: the path, where the visitor came
// from and any ?ref= tag. No cookie is set and nothing about who is sent.
function countVisit() {
  try {
    // The site's own Download buttons carry their tag in the fragment (#ref=…), links from elsewhere in the query.
    const ref = params.get("ref") ?? new URLSearchParams(location.hash.slice(1)).get("ref") ?? "";
    const body = JSON.stringify({ path: location.pathname, referrer: document.referrer, ref });
    navigator.sendBeacon?.("/api/v1/visits", body);
  } catch {
    // Counting never gets in the way of the page.
  }
}

/**
 * On an Android device the Windows and Mac installers are no use, and the Android app is in testing:
 * the header's Download button, and any labelled download button (the home page's), lead to the test.
 */
function androidVisitors() {
  if (!/android/i.test(navigator.userAgent)) return;
  for (const link of document.querySelectorAll("a.header-download")) {
    link.href = "/android/";
    link.textContent = "Android test";
  }
  for (const label of document.querySelectorAll("[data-download-label]")) {
    const link = label.closest("a");
    if (!link) continue;
    link.href = "/android/";
    label.textContent = "Join the Android test";
  }
}

// A page that loads this file under two addresses (site.js?v=2 in its HTML, site.js from an
// older cached page script) gets two copies of it; only the first wires up the page.
if (!globalThis.urbudgetSiteReady) {
  globalThis.urbudgetSiteReady = true;
  header();
  androidVisitors();
  themeToggle();
  navGroups();
  tabs();
  year();
  prices();
  countVisit();
}
