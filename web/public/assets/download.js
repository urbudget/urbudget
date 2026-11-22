// /download/: puts the download for this computer first, fills in the newest release from
// GET /api/v1/releases/latest (version, file names, sizes, checksums and what's new), and explains
// ?unavailable=windows|macos|linux (where the Worker sends people when that installer isn't
// published yet). Linux's downloads and steps are in the page but hidden: they show once the
// newest release has a Linux installer. Every other link works without any of this.

import { api } from "./api.js";
import { APP_STORE, formatBytes, formatDate, isApple, params } from "./site.js?v=2";
import { noteSections } from "./release-notes.js";

const $ = (id) => document.getElementById(id);
const NAMES = { windows: "Windows", macos: "macOS" };
/** Every installer a release can have, by its name in the API; Linux has two. */
const FILES = { ...NAMES, linux: "Linux", "linux-deb": "Linux" };
const HINTS = {
  windows: ["Installs and updates through the Store.", "Installer and Mac app"],
  macos: ["For Apple silicon and Intel, macOS 12 or later.", "Windows downloads"],
  linux: ["An AppImage for 64-bit Intel and AMD computers.", "The .deb and other systems"],
  ios: ["For iPhone and iPad, with iOS or iPadOS 17 or later.", "Downloads for your computer"],
};

function detectOS() {
  const ua = navigator.userAgent || "";
  const platform = navigator.userAgentData?.platform || navigator.platform || "";
  if (isApple()) return "ios";
  if (/Android/i.test(ua)) return "android";
  if (/Win/i.test(platform) || /Windows NT/i.test(ua)) return "windows";
  if (/Mac/i.test(platform) || /Mac OS X|Macintosh/i.test(ua)) return "macos";
  if (/Linux|CrOS|X11/i.test(platform + ua)) return "linux";
  return "unknown";
}

/** Shows only this computer's button in the hero, and puts its downloads and steps first. */
function recommend(os) {
  for (const button of document.querySelectorAll("#dl-actions [data-os]")) {
    const mine = button.dataset.os === os;
    button.hidden = !mine;
    button.classList.toggle("btn-primary", mine);
    button.classList.toggle("dl-btn-quiet", !mine);
  }
  const [text, other] = HINTS[os];
  const hint = $("dl-hint");
  const link = hint.querySelector("a");
  link.textContent = other;
  hint.textContent = `${text} `;
  hint.append(link);
  // The one download for this computer (the Store on Windows) is marked, and its button is the
  // main one.
  for (const button of document.querySelectorAll(".dl-item [data-pick]")) {
    const mine = button.dataset.pick === os;
    button.classList.toggle("btn-primary", mine);
    button.closest(".dl-item").classList.toggle("is-here", mine);
  }
  // Moved, not just restyled, so the order is the same for keyboards and screen readers.
  const list = $("dl-list");
  const steps = $("dl-steps");
  for (const el of [...list.querySelectorAll(`.dl-item[data-os="${os}"]`)].reverse()) list.prepend(el);
  for (const el of [...steps.querySelectorAll(`.dl-steps[data-os="${os}"]`)].reverse()) steps.prepend(el);
}

/** One run of text with a link in it, in one box so the notice's flex layout keeps it a sentence. */
function sentence(before, text, href, after) {
  const span = document.createElement("span");
  span.append(before, Object.assign(document.createElement("a"), { href, textContent: text }), after);
  return span;
}

function osNote(os) {
  const note = $("os-note");
  if (os === "ios") {
    note.replaceChildren(sentence("On an iPhone or iPad, UrBudget comes from the ", "App Store", APP_STORE, ". The rest of this page is for your computer."));
  } else if (os === "android") {
    note.replaceChildren(sentence("UrBudget for Android is in testing on Google Play: ", "join the test", "/android/", ". The rest of this page is for your computer."));
  } else if (os === "linux") {
    note.textContent = "UrBudget runs on Windows and macOS. The Linux version isn't published yet.";
  } else {
    return;
  }
  note.hidden = false;
}

/** Shows Linux's downloads, steps and answers, and on a Linux computer puts them first. */
function offerLinux() {
  for (const el of document.querySelectorAll("[data-linux]")) el.hidden = false;
  for (const el of document.querySelectorAll("[data-no-linux]")) el.hidden = true;
  if (os === "linux") recommend("linux");
}

function unavailable() {
  const platform = params.get("unavailable");
  if (!FILES[platform]) return null;
  $("unavailable-text").textContent =
    `The ${FILES[platform]} version isn't published yet. Please check back soon. If you have questions, write to support@urbudget.app.`;
  $("unavailable-notice").hidden = false;
  return platform;
}

const el = (tag, text) => {
  const node = document.createElement(tag);
  if (text) node.textContent = text;
  return node;
};

/**
 * Plain-text blocks and Markdown sections each become a card. Headings and "- " items
 * are recognized; all content stays text rather than HTML.
 */
function showNotes(latest, date) {
  const text = String(latest.notes ?? "").trim();
  if (!text) return;
  const box = $("notes-body");
  box.replaceChildren();
  for (const lines of noteSections(text)) {
    const card = el("div");
    card.className = "dl-note";
    let list = null;
    for (const line of lines) {
      const item = line.match(/^[-*•]\s+(.*)$/);
      if (item) {
        list ??= card.appendChild(el("ul"));
        list.append(el("li", item[1]));
      } else {
        list = null;
        card.append(el(card.childElementCount ? "p" : "h3", line));
      }
    }
    box.append(card);
  }
  $("notes-title").textContent = `What's new in ${latest.version}`;
  $("notes-date").textContent = date ? `Released ${date}. Updates are free.` : "Updates are free.";
  $("whats-new").hidden = false;
  const link = el("a", "What's new");
  link.href = "#whats-new";
  $("dl-release").append(" · ", link);
}

async function release() {
  try {
    const latest = await api("/releases/latest");
    const date = formatDate(latest.date);
    // "October 20", with the year only when it isn't this one.
    const day = date.replace(`, ${new Date().getFullYear()}`, "");
    $("dl-release").textContent = `Version ${latest.version}${day ? `, updated ${day}` : ""}`;
    $("release-line").textContent = `Direct installers are version ${latest.version}. Microsoft Store updates follow Store certification and may arrive later.`;
    // On a Linux computer, the note that there's no Linux version waits for this answer.
    if (latest.platforms?.linux) offerLinux();
    else if (os === "linux") osNote(os);
    const sums = [];
    for (const key of Object.keys(FILES)) {
      const p = latest.platforms?.[key];
      const meta = $(`meta-${key}`);
      if (!p) {
        meta.textContent = "Not published yet";
        continue;
      }
      if (p.file) meta.textContent = p.file;
      const size = formatBytes(p.size);
      if (size) Object.assign($(`size-${key}`), { textContent: size, hidden: false });
      if (p.file) for (const name of document.querySelectorAll(`[data-file="${key}"]`)) name.textContent = p.file;
      if (p.sha256) sums.push([p.file || FILES[key], p.sha256]);
    }
    if (sums.length) {
      $("checksums").replaceChildren(
        ...sums.map(([file, sum]) => {
          const row = el("div");
          const dd = el("dd");
          dd.append(el("code", sum));
          row.append(el("dt", file), dd);
          return row;
        }),
      );
    }
    showNotes(latest, date);
  } catch {
    // Static text distinguishes direct releases from Store certification.
    if (os === "linux") osNote(os);
  }
}

const os = detectOS();
const missing = unavailable();
if (NAMES[os] || os === "ios") recommend(os);
else if (NAMES[missing]) recommend(missing);
if (os !== "linux") osNote(os);
void release();
