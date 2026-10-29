// /admin/: accounts and the numbers behind UrBudget. Cloudflare Access signs the admin in before
// this page loads, and the API checks again on every call. Values from the server (emails, notes,
// sources) are untrusted, so every one goes into the page as text, never as HTML.

import { api, ApiError } from "/assets/api.js";
import { copyText, formatBytes, setStatus } from "/assets/site.js";

const $ = (id) => document.getElementById(id);

/* ---------- Building the page ---------- */

/** An element with attributes and children (strings become text nodes): h("td", { class: "num" }, 3) */
function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) {
    if (value === null || value === undefined || value === false) continue;
    if (name.startsWith("on")) el.addEventListener(name.slice(2), value);
    else el.setAttribute(name === "className" ? "class" : name, value === true ? "" : String(value));
  }
  append(el, children);
  return el;
}
function append(el, children) {
  for (const child of children.flat(Infinity)) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
}
const SVG = "http://www.w3.org/2000/svg";
function s(tag, attrs = {}) {
  const el = document.createElementNS(SVG, tag);
  for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, String(value));
  return el;
}

/** Fills a table body; `empty` is shown in one row when there's nothing. */
function fillTable(table, rows, empty) {
  const body = table.tBodies[0];
  const columns = table.tHead?.rows[0]?.cells.length || 1;
  body.replaceChildren(...(rows.length ? rows : [h("tr", { class: "empty" }, h("td", { colspan: columns }, empty))]));
}

/* ---------- Formatting ---------- */

const numberFormat = new Intl.NumberFormat("en-US");
const num = (n) => numberFormat.format(Number(n) || 0);
const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });
const money = (cents) =>
  new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: Math.round(cents) % 100 ? 2 : 0,
    maximumFractionDigits: 2,
  }).format((Number(cents) || 0) / 100);
const percent = (ratio) => `${(ratio * 100).toFixed(ratio > 0 && ratio < 0.1 ? 1 : 0)}%`;
const plural = (n, one, many = `${one}s`) => `${num(n)} ${n === 1 ? one : many}`;
const STORES = { polar: "Polar", google: "Google Play", apple: "App Store", comp: "given" };
/** "3 Polar, 1 App Store" from live subscriptions per store; empty when there are none. */
const storeCounts = (counts = {}) =>
  Object.entries(STORES)
    .filter(([key]) => counts[key])
    .map(([key, name]) => `${num(counts[key])} ${name}`)
    .join(", ");

const thisYear = new Date().getFullYear();
/** "Oct 17, 2:05 PM" (with the year when it isn't this one), in this computer's time zone. */
function when(iso) {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    ...(date.getFullYear() === thisYear ? {} : { year: "numeric" }),
    hour: "numeric",
    minute: "2-digit",
  });
}
/** "Oct 17, 2026" from a timestamp or a YYYY-MM-DD day (read as UTC). */
function dateOnly(iso) {
  if (!iso) return "";
  const date = new Date(iso.length === 10 ? `${iso}T00:00:00Z` : iso);
  return date.toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });
}
/** "Oct 17" for a chart axis. */
const shortDay = (day) => new Date(`${day.slice(0, 10)}T00:00:00Z`).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const sum = (values) => values.reduce((total, v) => total + (Number(v) || 0), 0);

function badge(text, kind = "") {
  return h("span", { class: `badge${kind ? ` badge-${kind}` : ""}` }, text);
}

/* ---------- Stat tiles ---------- */

/** "↑ 25% more than the 30 days before", or nothing when both periods had none. */
function trend([now, before], than = "the 30 days before") {
  if (!now && !before) return null;
  const title = `${num(now)} now, ${num(before)} in ${than}`;
  if (!before) return h("span", { class: "tile-trend is-up", title }, `New: none in ${than}`);
  if (now === before) return h("span", { class: "tile-trend", title }, `Same as ${than}`);
  const up = now > before;
  return h(
    "span",
    { class: `tile-trend is-${up ? "up" : "down"}`, title },
    h("span", { "aria-hidden": "true" }, up ? "↑ " : "↓ "),
    `${percent(Math.abs(now - before) / before)} ${up ? "more" : "fewer"} than ${than}`,
  );
}

/** Tiles of { label, value, sub?, trend?: [now, before], than?: what `before` was (the 30 days before) }. */
function tiles(container, items) {
  container.replaceChildren(
    ...items.map((item) =>
      h(
        "div",
        { class: "tile" },
        h("span", { class: "tile-label" }, item.label),
        h("span", { class: "tile-value" }, item.value),
        item.sub ? h("span", { class: "tile-sub" }, item.sub) : null,
        item.trend ? trend(item.trend, item.than) : null,
      ),
    ),
  );
}

/* ---------- Column chart ---------- */

// Each chart card redraws to its own width.
const resizer = new ResizeObserver((entries) => {
  for (const entry of entries) entry.target._draw?.();
});

/** A round step that splits 0..max into about three gridlines. Counts never step below 1. */
function niceStep(max) {
  const raw = max / 3;
  const power = 10 ** Math.floor(Math.log10(raw));
  const n = raw / power;
  return Math.max(1, (n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10) * power);
}

/** A column for the top of a stack: 4px rounded data end, square at the baseline. */
function columnPath(x, y, width, height, round) {
  const r = round ? Math.min(4, width / 2, height) : 0;
  return `M${x},${y + height}V${y + r}Q${x},${y} ${x + r},${y}H${x + width - r}Q${x + width},${y} ${x + width},${y + r}V${y + height}Z`;
}

/**
 * Stacked columns over time, with a legend (two or more series), a hover/keyboard tooltip and a
 * table view. `labels` are the x positions' names (days, weeks or months); `tips` their long form.
 */
function columnChart(card, { title, sub, labels, tips = labels, series, format = num, axis = (v) => compact.format(v), empty }) {
  card.replaceChildren();
  const totals = series.map((one) => sum(one.values));
  const caption = h(
    "figcaption",
    { class: "chart-head" },
    h("div", {}, h("div", { class: "chart-title" }, title), sub ? h("div", { class: "chart-sub" }, sub) : null),
    series.length > 1
      ? h(
          "ul",
          { class: "legend", "aria-label": "Legend" },
          series.map((one, i) =>
            h("li", {}, h("span", { class: "swatch", style: `background:${one.color}` }), one.name, " ", h("b", {}, format(totals[i]))),
          ),
        )
      : null,
  );
  card.append(caption);

  const stacks = labels.map((_, i) => sum(series.map((one) => one.values[i])));
  const max = Math.max(0, ...stacks);
  if (max === 0) {
    card.append(h("p", { class: "chart-empty" }, empty || "Nothing in this period yet."));
    card._draw = null;
    return;
  }

  const host = h("div", { class: "chart-plot" });
  const tooltip = h("div", { class: "chart-tooltip", hidden: true, "aria-live": "polite" });
  card.append(host, tooltip, chartTable({ labels: tips, series, format }));

  const step = niceStep(max);
  const top = Math.ceil(max / step) * step;
  const ticks = Array.from({ length: Math.round(top / step) + 1 }, (_, i) => i * step);
  let active = -1;

  card._draw = () => {
    const W = Math.max(260, Math.floor(host.clientWidth));
    const H = 180;
    const m = { top: 8, right: 2, bottom: 22, left: Math.max(28, 8 + 7 * axis(top).length) };
    const pw = W - m.left - m.right;
    const ph = H - m.top - m.bottom;
    const n = labels.length;
    const slot = pw / n;
    const bar = Math.max(1, Math.min(24, slot - 2));
    const y = (v) => m.top + ph - (v / top) * ph;

    const root = s("svg", { class: "chart-svg", viewBox: `0 0 ${W} ${H}`, width: W, height: H, tabindex: 0, role: "img" });
    root.setAttribute("aria-label", `${title}. ${series.map((one, i) => `${one.name}: ${format(totals[i])}`).join(", ")}. Arrow keys step through the ${n} columns.`);
    for (const t of ticks) {
      root.append(s("line", { class: "grid", x1: m.left, x2: W - m.right, y1: Math.round(y(t)) + 0.5, y2: Math.round(y(t)) + 0.5 }));
      const label = s("text", { class: "tick", x: m.left - 6, y: y(t) + 4, "text-anchor": "end" });
      label.textContent = axis(t);
      root.append(label);
    }
    // About one x label per 80px, never cut off at the edges.
    const every = Math.max(1, Math.ceil(n / Math.max(2, Math.floor(pw / 80))));
    for (let i = n - 1; i >= 0; i -= every) {
      const cx = m.left + slot * (i + 0.5);
      const label = s("text", { class: "tick", x: cx, y: H - 6, "text-anchor": i === n - 1 ? "end" : "middle" });
      if (i === n - 1) label.setAttribute("x", W - m.right);
      label.textContent = labels[i];
      if (cx > m.left + 24 || i === n - 1) root.append(label);
    }

    const band = s("rect", { class: "hover-band", x: 0, y: m.top, width: slot, height: ph, rx: 3 });
    root.append(band);
    for (let i = 0; i < n; i++) {
      const x = m.left + slot * i + (slot - bar) / 2;
      let base = 0;
      const lastSeries = series.findLastIndex((one) => one.values[i] > 0);
      series.forEach((one, k) => {
        const value = one.values[i] || 0;
        if (value <= 0) return;
        const yTop = y(base + value);
        // A 2px surface gap between stacked segments.
        const gap = base > 0 ? 2 : 0;
        const height = Math.max(1, y(base) - yTop - gap);
        root.append(s("path", { d: columnPath(x, yTop, bar, height, k === lastSeries), fill: one.color }));
        base += value;
      });
    }

    const show = (i) => {
      active = Math.max(0, Math.min(n - 1, i));
      band.setAttribute("x", m.left + slot * active);
      band.classList.add("is-on");
      tooltip.replaceChildren();
      append(tooltip, [
        h("div", { class: "tip-date" }, tips[active]),
        series.map((one) =>
          h("div", { class: "tip-row" }, h("span", { class: "tip-key", style: `background:${one.color}` }), h("b", {}, format(one.values[active] || 0)), one.name),
        ),
      ]);
      tooltip.hidden = false;
      const scale = root.getBoundingClientRect().width / W;
      const left = host.offsetLeft + (m.left + slot * (active + 0.5)) * scale;
      const width = tooltip.offsetWidth;
      tooltip.style.left = `${Math.max(4, Math.min(card.clientWidth - width - 4, left - width / 2))}px`;
      tooltip.style.top = `${Math.max(4, host.offsetTop - tooltip.offsetHeight + 8)}px`;
    };
    const hide = () => {
      band.classList.remove("is-on");
      tooltip.hidden = true;
    };
    root.addEventListener("pointermove", (event) => {
      const rect = root.getBoundingClientRect();
      const x = ((event.clientX - rect.left) / rect.width) * W;
      if (x < m.left) return hide();
      show(Math.floor((x - m.left) / slot));
    });
    root.addEventListener("pointerleave", hide);
    root.addEventListener("blur", hide);
    root.addEventListener("focus", () => show(active >= 0 ? active : n - 1));
    root.addEventListener("keydown", (event) => {
      const moves = { ArrowLeft: active - 1, ArrowRight: active + 1, Home: 0, End: n - 1 };
      if (event.key in moves) {
        event.preventDefault();
        show(moves[event.key]);
      } else if (event.key === "Escape") hide();
    });
    host.replaceChildren(root);
  };
  card._draw();
  resizer.observe(card);
}

/** The chart's numbers as a table: every period with anything in it, newest first. */
function chartTable({ labels, series, format }) {
  const rows = [];
  for (let i = labels.length - 1; i >= 0; i--) {
    const values = series.map((one) => one.values[i] || 0);
    if (values.some((v) => v)) rows.push(h("tr", {}, h("td", {}, labels[i]), values.map((v) => h("td", { class: "num" }, format(v)))));
  }
  return h(
    "details",
    { class: "chart-table" },
    h("summary", {}, "Show as a table"),
    h(
      "div",
      { class: "table-wrap" },
      h(
        "table",
        { class: "data-table" },
        h("thead", {}, h("tr", {}, h("th", { scope: "col" }, "Period"), series.map((one) => h("th", { scope: "col", class: "num" }, one.name)))),
        h("tbody", {}, rows),
      ),
    ),
    h("p", { class: "muted" }, "Periods that aren't listed had none."),
  );
}

/* ---------- Tabs ---------- */

const tabButtons = [...document.querySelectorAll('[role="tab"]')];
const tabName = (button) => button.id.replace("tab-", "");
const loaded = new Set();
const loaders = {};
let current = "overview";

async function run(name) {
  const panel = $(`panel-${name}`);
  panel.classList.add("is-loading");
  try {
    await loaders[name]();
    loaded.add(name);
    $("load-error").hidden = true;
  } catch (error) {
    const message =
      error instanceof ApiError && error.code === "not_admin"
        ? "Your Cloudflare Access sign-in isn't on the admin list (ADMIN_EMAILS), or it has expired. Reload the page to sign in again."
        : error.message || "That didn't load.";
    $("load-error").textContent = message;
    $("load-error").hidden = false;
  } finally {
    panel.classList.remove("is-loading");
  }
}

function select(name, { focus = false } = {}) {
  if (!loaders[name]) name = "overview";
  current = name;
  for (const button of tabButtons) {
    const on = tabName(button) === name;
    button.setAttribute("aria-selected", String(on));
    button.tabIndex = on ? 0 : -1;
    $(button.getAttribute("aria-controls")).hidden = !on;
    if (on && focus) button.focus();
  }
  if (location.hash.slice(1) !== name) history.replaceState(null, "", `#${name}`);
  if (!loaded.has(name)) run(name);
}

for (const button of tabButtons) button.addEventListener("click", () => select(tabName(button)));
document.querySelector('[role="tablist"]').addEventListener("keydown", (event) => {
  const index = tabButtons.findIndex((b) => b.getAttribute("aria-selected") === "true");
  const moves = { ArrowRight: index + 1, ArrowLeft: index - 1, Home: 0, End: tabButtons.length - 1 };
  if (!(event.key in moves)) return;
  event.preventDefault();
  select(tabName(tabButtons[(moves[event.key] + tabButtons.length) % tabButtons.length]), { focus: true });
});
$("refresh").addEventListener("click", () => run(current));

/* ---------- Overview ---------- */

function header(data) {
  $("who").textContent = `Signed in as ${data.admin}`;
  const env = $("environment");
  env.hidden = false;
  env.className = `badge ${data.environment === "production" ? "badge-success" : "badge-warn"}`;
  env.textContent = data.environment === "production" ? "Live payments" : "Sandbox payments";
}

function attention(text, kind, label, onclick) {
  return h(
    "div",
    { class: `notice notice-${kind}` },
    h("span", {}, text),
    label ? h("button", { class: "link-button", type: "button", onclick, style: "margin-left:auto" }, label) : null,
  );
}

loaders.overview = async () => {
  const o = await api("/admin/overview");
  header(o);
  const p = o.previous30;
  const u = o.usage;
  tiles($("overview-tiles"), [
    {
      label: "Downloads, 30 days",
      value: num(o.downloads.last30),
      sub: `${channelSplit(o.downloads)}${notPeople(o.downloads.bots, o.downloads.team)}`,
      trend: [o.downloads.last30, p.downloads],
    },
    {
      label: "First opens, 30 days",
      value: num(u.opens30),
      sub: u.since ? `Copies installed and opened${o.downloads.last30 ? ` · ${percent(Math.min(1, u.opens30 / o.downloads.last30))} of downloads` : ""}` : "None counted yet: copies count themselves from the next desktop release",
      trend: [u.opens30, p.opens],
    },
    {
      label: "In use last week",
      value: num(u.lastWeek),
      sub: `${num(u.thisWeek)} this week so far · ${num(u.yesterday)} yesterday`,
      trend: [u.lastWeek, u.weekBefore],
      than: "the week before",
    },
    {
      label: "Page views, 30 days",
      value: num(o.pageViews.last30),
      sub: `${num(o.pageViews.home)} of the home page`,
      trend: [o.pageViews.last30, p.pageViews],
    },
    {
      label: "Accounts",
      value: num(o.accounts.total),
      sub: `${num(o.accounts.last30)} new · ${num(o.accounts.activeDesktop)} used the app, last 30 days`,
      trend: [o.accounts.last30, p.accounts],
    },
    {
      label: "Sync subscribers",
      value: num(o.sync.subscribers),
      sub: [storeCounts(o.sync.byProvider), `${plural(o.sync.budgets, "budget")} · ${formatBytes(o.sync.bytes)} stored`]
        .filter(Boolean)
        .join(" · "),
    },
    {
      label: "Sync checkouts, 30 days",
      value: num(o.funnel.checkouts),
      sub: "Opened at Polar, paid or not",
      trend: [o.funnel.checkouts, p.checkouts],
    },
  ]);

  const notes = [];
  if (o.environment !== "production") notes.push(attention("Payments are in Polar's sandbox: checkout takes test cards only.", "warn"));
  const problems = sum(Object.values(o.health.problems30));
  if (problems)
    notes.push(attention(`${plural(problems, "problem")} in the last 30 days, such as emails that didn't send.`, "warn", "Open Health", () => select("health")));
  const mail = o.health.emails;
  if (nearLimit(mail.today, mail.perDay) || nearLimit(mail.month, mail.perMonth))
    notes.push(
      attention(
        `${num(mail.today)} emails sent today and ${num(mail.month)} this month: Resend's free plan stops at ${num(mail.perDay)} a day and ${num(mail.perMonth)} a month, and sign-in codes stop with it.`,
        "warn",
        "Open Health",
        () => select("health"),
      ),
    );
  const cleanup = cleanupLate(o.health.lastCleanup, o.countingSince);
  if (cleanup) notes.push(attention(cleanup, "warn", "Open Health", () => select("health")));
  $("overview-attention").replaceChildren(...notes);

  funnel(o.funnel, o.countingSince, u.since);
  weeks(o.weeks, o.countingSince);

  const days = o.daily.days;
  const labels = days.map(shortDay);
  const tips = days.map(dateOnly);
  columnChart($("chart-downloads"), {
    title: "Downloads per day",
    sub: `Last 90 days · people only · ${o.downloads.storeSource === "clicks" ? "the Store as clicks on the site's Store button, until Partner Center is connected" : "the Store's acquisitions from Partner Center"}`,
    labels,
    tips,
    series: channelSeries(o.daily),
    empty: "No downloads counted yet.",
  });
  columnChart($("chart-in-use"), {
    title: "Copies in use per day",
    sub: "Last 90 days · copies of the app opened that day, as they count themselves",
    labels,
    tips,
    series: [{ name: "In use", values: o.daily.active, color: "var(--series-1)" }],
    empty: "No copies have counted themselves yet. Usage counts start with the next desktop release.",
  });
  columnChart($("chart-checkouts"), {
    title: "Sync checkouts per day",
    sub: "Last 90 days · opened at Polar, paid or not",
    labels,
    tips,
    series: [{ name: "Checkouts", values: o.daily.checkouts, color: "var(--series-1)" }],
    empty: "No Sync checkouts opened yet.",
  });
  columnChart($("chart-signups"), {
    title: "New accounts per day",
    sub: "Last 90 days",
    labels,
    tips,
    series: [{ name: "New accounts", values: o.daily.signups, color: "var(--series-1)" }],
  });
};

/** "urbudget.app 12 · GitHub 3 · Store 20": a period's downloads by channel. */
function channelSplit(d) {
  return `urbudget.app ${num(d.website)} · GitHub ${num(d.github)} · Store ${num(d.store)}${d.storeSource === "clicks" ? " (button clicks)" : ""}`;
}

/** The three channels as a chart's series, in the same colors on every chart. */
function channelSeries({ website, github, store }) {
  return [
    { name: "urbudget.app", values: website, color: "var(--series-1)" },
    { name: "GitHub", values: github, color: "var(--series-2)" },
    { name: "Microsoft Store", values: store, color: "var(--series-3)" },
  ];
}

/** The steps from a visit to a Sync checkout, each with its share of the step before. */
function funnel(f, countingSince, usageSince) {
  const steps = [
    ["Page views", f.views],
    ["Downloads, every channel", f.downloads],
    ["First opens of the app", f.opens],
    ["Sync checkouts opened", f.checkouts],
  ];
  $("funnel").replaceChildren(
    ...steps.map(([label, n], i) => {
      const before = i ? steps[i - 1][1] : 0;
      // Not a share when the step before had fewer: those came in through another door, or earlier.
      const share = before && n <= before ? `${percent(n / before)} of the step before` : "";
      return h("li", {}, h("span", { class: "funnel-value" }, num(n)), h("span", { class: "funnel-label" }, label), share ? h("span", { class: "funnel-share" }, share) : null);
    }),
  );
  const since =
    !countingSince || countingSince > daysAgo(29)
      ? ` Checkouts are counted from ${countingSince ? dateOnly(countingSince) : "this version of the Worker"} on.`
      : "";
  const opens = !usageSince || usageSince > daysAgo(29) ? ` First opens are counted from ${usageSince ? dateOnly(usageSince) : "the next desktop release"} on.` : "";
  $("funnel-note").textContent = `People only, last 30 days. A step can outnumber the one before it: someone who found UrBudget in the Store, or a checkout from someone who downloaded earlier.${since}${opens}`;
}

/** YYYY-MM-DD for `n` days before today (UTC), as the API counts days. */
const daysAgo = (n) => new Date(Date.now() - n * 86_400_000).toISOString().slice(0, 10);

/** Whether emails sent are within 20% of one of Resend's limits. */
const nearLimit = (sent, limit) => sent >= limit * 0.8;

/** Why the daily clean-up looks stuck, or "" when it ran in the last two days (or nothing was counted yet). */
function cleanupLate(last, countingSince) {
  if (last) return last < daysAgo(2) ? `The daily clean-up last ran on ${dateOnly(last)}. Check the Worker's cron trigger.` : "";
  return countingSince && countingSince < daysAgo(2) ? `The daily clean-up hasn't run since counting began on ${dateOnly(countingSince)}. Check the Worker's cron trigger.` : "";
}

/** "hn 12, tool-loan 5, …": the most-used link tags. */
const tagList = (tags, top) => `${tags.slice(0, top).map((t) => `${t.tag} ${num(t.n)}`).join(", ")}${tags.length > top ? ", …" : ""}`;
/** "34 (hn 12, tool-loan 5)": a week's downloads with its top link tags. */
const withTags = (downloads, tags) => `${num(downloads)}${tags.length ? ` (${tagList(tags, 3)})` : ""}`;

/** The week's row for marketing/weekly-log.md: this page's columns filled, the rest blank. */
function weeklyLogRow(week) {
  const cells = [dateOnly(week.start), "", "", "", "", withTags(week.downloads, week.tags), "", "", ""];
  return `| ${cells.map((cell) => cell.replace(/\|/g, "\\|")).join(" | ")} |`.replace(/ {2,}/g, " ");
}

/** YYYY-MM-DD `n` days after a YYYY-MM-DD day. */
const addDays = (day, n) => new Date(Date.parse(`${day}T00:00:00Z`) + n * 86_400_000).toISOString().slice(0, 10);

/** The weeks, newest first. The first is this week so far, which isn't ready for the weekly log yet. */
function weeks(rows, countingSince) {
  const notCounted = () => h("span", { class: "muted", title: "Checkouts weren't counted yet" }, "—");
  fillTable(
    $("weeks-table"),
    rows.map((week, i) =>
      h(
        "tr",
        {},
        h("td", { class: "mono" }, shortDay(week.start), i === 0 ? h("span", { class: "muted" }, " so far") : null),
        h("td", { class: "num" }, num(week.views)),
        h("td", { class: "num", title: channelSplit(week) }, num(week.downloads)),
        h("td", { class: "detail" }, week.tags.length ? tagList(week.tags, 5) : h("span", { class: "muted" }, "None")),
        h("td", { class: "num" }, num(week.opens)),
        h("td", { class: "num" }, num(week.active)),
        h("td", { class: "num" }, countingSince && addDays(week.start, 6) >= countingSince ? num(week.checkouts) : notCounted()),
        h("td", { class: "num" }, num(week.accounts)),
        h(
          "td",
          {},
          i === 0
            ? null
            : h(
                "button",
                {
                  class: "btn btn-sm btn-ghost",
                  type: "button",
                  title: `Copy the week of ${dateOnly(week.start)} as a row for marketing/weekly-log.md`,
                  onclick: (event) => copyText(weeklyLogRow(week), event.currentTarget),
                },
                h("span", { "data-label": true }, "Copy"),
              ),
        ),
      ),
    ),
    "No weeks yet.",
  );
}

/* ---------- Accounts ---------- */

const accountState = { q: "", filter: "all", page: 0 };
const SUBSCRIPTION = { active: "Active", trialing: "Trial", past_due: "Payment due", canceled: "Canceled", unpaid: "Unpaid", incomplete: "Incomplete", paused: "Paused" };
const LIVE = new Set(["active", "trialing", "past_due"]);

function subscriptionBadge(status) {
  return badge(`Sync: ${SUBSCRIPTION[status] || status}`, status === "active" || status === "trialing" ? "success" : LIVE.has(status) ? "warn" : "");
}

loaders.accounts = async () => {
  const { q, filter, page } = accountState;
  const data = await api(`/admin/accounts?${new URLSearchParams({ q, filter, page: String(page) })}`);
  fillTable(
    $("account-table"),
    data.accounts.map((account) =>
      h(
        "tr",
        { onclick: (event) => !event.target.closest("button") && openAccount(account.id) },
        h("td", {}, h("button", { class: "row-button row-button-text", type: "button", onclick: () => openAccount(account.id) }, account.email)),
        h(
          "td",
          {},
          h(
            "span",
            { class: "badges" },
            account.subscription ? subscriptionBadge(account.subscription) : h("span", { class: "muted" }, "Nothing"),
          ),
        ),
        h("td", { class: "num" }, num(account.devices)),
        h("td", { class: "num" }, num(account.budgets)),
        h("td", {}, account.lastSeenAt ? dateOnly(account.lastSeenAt) : h("span", { class: "muted" }, "Signed out")),
        h("td", {}, dateOnly(account.createdAt)),
      ),
    ),
    q || filter !== "all" ? "No accounts match." : "No accounts yet.",
  );
  const from = data.total ? page * data.pageSize + 1 : 0;
  const to = Math.min(data.total, (page + 1) * data.pageSize);
  $("account-count").textContent = data.total ? `${num(from)}–${num(to)} of ${plural(data.total, "account")}` : "";
  $("account-prev").disabled = page === 0;
  $("account-next").disabled = to >= data.total;
};

$("account-search").addEventListener("submit", (event) => {
  event.preventDefault();
  accountState.q = $("account-q").value.trim();
  accountState.filter = $("account-filter").value;
  accountState.page = 0;
  run("accounts");
});
$("account-filter").addEventListener("change", () => $("account-search").requestSubmit());
$("account-prev").addEventListener("click", () => {
  accountState.page = Math.max(0, accountState.page - 1);
  run("accounts");
});
$("account-next").addEventListener("click", () => {
  accountState.page += 1;
  run("accounts");
});

const accountDialog = $("account-dialog");
accountDialog.addEventListener("click", (event) => {
  if (event.target.closest("[data-close]")) return accountDialog.close();
  if (event.target !== accountDialog) return;
  const box = accountDialog.getBoundingClientRect();
  const inside = event.clientX >= box.left && event.clientX <= box.right && event.clientY >= box.top && event.clientY <= box.bottom;
  if (!inside) accountDialog.close();
});

/** Shows an error where the admin is looking: in the account dialog if it's open, or above the tabs. */
function showError(message) {
  if (accountDialog.open) return setStatus($("account-status"), message, "error");
  $("load-error").textContent = message;
  $("load-error").hidden = false;
}

async function openAccount(id) {
  try {
    const data = await api(`/admin/accounts/${encodeURIComponent(id)}`);
    setStatus($("account-status"), "");
    renderAccount(data);
    if (!accountDialog.open) accountDialog.showModal();
  } catch (error) {
    showError(error.message);
  }
}

function renderAccount({ account, subscriptions, sessions, budgets, syncKeySetAt, syncKeySet }) {
  $("account-title").textContent = account.email;
  const fact = (term, ...value) => [h("dt", {}, term), h("dd", {}, ...value)];
  const seen = sessions.map((s) => s.lastSeenAt).sort().at(-1);
  $("account-facts").replaceChildren(
    ...fact("Created", when(account.createdAt)),
    ...fact("Last seen", seen ? when(seen) : h("span", { class: "muted" }, "Signed out everywhere")),
    ...fact("Polar customer", account.customerId ? h("span", { class: "mono" }, account.customerId) : h("span", { class: "muted" }, "None yet")),
    ...fact("Sync passphrase", syncKeySetAt ? `Set ${when(syncKeySetAt)}` : syncKeySet ? "Set" : h("span", { class: "muted" }, "Not set")),
    ...fact("Account id", h("span", { class: "mono" }, account.id)),
  );
  $("account-subscriptions").replaceChildren(
    ...(subscriptions.length
      ? subscriptions.map((sub) =>
          h(
            "li",
            {},
            subscriptionBadge(sub.status),
            " ",
            sub.cancelAt
              ? `Ends ${dateOnly(sub.cancelAt)}`
              : sub.live && sub.currentPeriodEnd
                ? `Renews ${dateOnly(sub.currentPeriodEnd)}`
                : sub.currentPeriodEnd
                  ? `Ended ${dateOnly(sub.currentPeriodEnd)}`
                  : "",
            h("div", { class: "muted" }, `Updated ${when(sub.updatedAt)} · `, h("span", { class: "mono" }, sub.id)),
          ),
        )
      : [h("li", { class: "muted" }, "Never subscribed.")]),
  );
  fillTable(
    $("account-sessions"),
    sessions.map((s) =>
      h(
        "tr",
        {},
        h("td", {}, s.kind === "desktop" ? ["The app", s.deviceName ? h("span", { class: "muted" }, ` · ${s.deviceName}`) : null] : "The website"),
        h("td", {}, when(s.lastSeenAt)),
        h("td", {}, dateOnly(s.createdAt)),
      ),
    ),
    "Not signed in anywhere.",
  );
  fillTable(
    $("account-budgets"),
    budgets.map((b) =>
      h(
        "tr",
        {},
        h("td", { class: "mono" }, b.id, b.state === "trashed" ? h("span", { class: "muted" }, " · In Trash") : null),
        h("td", { class: "num" }, formatBytes(b.size)),
        h("td", { class: "num" }, num(b.revision)),
        h("td", {}, b.device || h("span", { class: "muted" }, "Not known")),
        h("td", {}, when(b.updatedAt)),
      ),
    ),
    "Nothing synced.",
  );
}

/* ---------- Traffic ---------- */

loaders.traffic = async () => {
  const t = await api("/admin/traffic");
  tiles($("traffic-tiles"), [
    { label: "Page views, 30 days", value: num(t.last30.people), sub: `${num(sum(t.views))} in 90 days` },
    {
      label: "Download rate, 30 days",
      value: t.last30.people ? percent(t.last30.downloads / t.last30.people) : "—",
      sub: `${num(t.last30.downloads)} clicks on the site's download buttons for ${num(t.last30.people)} page views`,
    },
    {
      label: "Bots and team, 30 days",
      value: num(t.last30.bots + t.last30.team),
      sub: `${num(t.last30.bots)} bots and agents · ${num(t.last30.team)} from this page's browsers`,
    },
  ]);
  columnChart($("chart-views-90"), {
    title: "Page views per day",
    sub: "Last 90 days · people only; bots, AI agents and the team are counted apart",
    labels: t.days.map(shortDay),
    tips: t.days.map(dateOnly),
    series: [{ name: "Page views", values: t.views, color: "var(--series-1)" }],
    empty: "No page views counted yet.",
  });
  fillTable(
    $("pages-table"),
    t.pages.map((row) => h("tr", {}, h("td", { class: "mono" }, row.path), h("td", { class: "num" }, num(row.n)))),
    "No page views yet.",
  );
  fillTable(
    $("visit-sources-table"),
    t.sources.map((row) => h("tr", {}, h("td", {}, row.source ? row.source : h("span", { class: "muted" }, "Not known")), h("td", { class: "num" }, num(row.n)))),
    "No page views yet.",
  );
  fillTable(
    $("tags-table"),
    t.tags.map((row) =>
      h(
        "tr",
        {},
        h("td", { class: "mono" }, row.tag),
        h("td", { class: "num" }, num(row.views)),
        h("td", { class: "num" }, num(row.downloads)),
        h("td", { class: "num" }, row.views ? percent(row.downloads / row.views) : "—"),
        h("td", {}, dateOnly(row.first)),
        h("td", {}, dateOnly(row.last)),
      ),
    ),
    "No tagged links counted yet. Add ?ref=name to a link you post.",
  );
};

/* ---------- Downloads: every channel ---------- */

/** " · plus 6 from bots and the team", or nothing. Only people count as downloads. */
function notPeople(bots, team) {
  const n = (bots ?? 0) + (team ?? 0);
  return n ? ` · plus ${num(n)} from bots and the team` : "";
}

/** Series of the same length added up, day by day. */
const add = (...series) => series[0].map((_, i) => sum(series.map((values) => values[i])));
/** The last `n` days of a daily series, added up; with `before`, the `n` days before those. */
const lastDaysOf = (values, n, before = false) => sum(before ? values.slice(-2 * n, -n) : values.slice(-n));

/** A source's state: a badge and what it means. */
function statusItem(badgeText, kind, ...text) {
  return h("li", {}, badge(badgeText, kind), h("span", {}, ...text));
}
/** How an outside source last answered: never yet, an error, or fine. */
function fetchedItem(name, fetched, waiting, fine) {
  if (!fetched) return statusItem("Waiting", "", h("b", {}, name), `: ${waiting}`);
  if (fetched.error)
    return statusItem(
      "Error",
      "warn",
      h("b", {}, name),
      `: ${fetched.error} Last tried ${when(fetched.triedAt)}${fetched.okAt ? `; last answered ${when(fetched.okAt)}` : ""}.`,
    );
  return statusItem("Fine", "success", h("b", {}, name), `: ${fine} Last answered ${when(fetched.okAt)}.`);
}

loaders.downloads = async () => {
  const d = await api("/admin/downloads");
  const website = add(d.website.windows, d.website.macos);
  const github = add(d.github.windows, d.github.macos);
  const store = d.store.partnerCenter ? d.store.acquisitions : d.website.store;
  const opens = add(d.opens.direct, d.opens.store);
  const total30 = lastDaysOf(d.total, 30);
  tiles($("downloads-tiles"), [
    {
      label: "Downloads, 30 days",
      value: num(total30),
      sub: `Every channel, people only · ${num(sum(d.total))} in 90 days`,
      trend: [total30, lastDaysOf(d.total, 30, true)],
    },
    {
      label: "urbudget.app, 30 days",
      value: num(lastDaysOf(website, 30)),
      sub: `Windows ${num(lastDaysOf(d.website.windows, 30))} · macOS ${num(lastDaysOf(d.website.macos, 30))} · ${num(lastDaysOf(d.website.store, 30))} sent to the Store`,
    },
    {
      label: "GitHub, 30 days",
      value: num(lastDaysOf(github, 30)),
      sub: d.github.since ? `Counted day by day since ${dateOnly(d.github.since)}` : "Not counted yet: the Worker notes GitHub's numbers every hour",
    },
    {
      label: "Microsoft Store, 30 days",
      value: num(lastDaysOf(store, 30)),
      sub: d.store.partnerCenter
        ? `Acquisitions, from Partner Center · ${num(lastDaysOf(d.store.installs, 30))} installs`
        : "Clicks on the site's Store button, until Partner Center is connected",
    },
    {
      label: "First opens, 30 days",
      value: num(lastDaysOf(opens, 30)),
      sub: total30 ? `${percent(Math.min(1, lastDaysOf(opens, 30) / total30))} of downloads became a copy in use` : "Copies installed and opened",
    },
    {
      label: "Bots and team, 30 days",
      value: num(d.others.bots.last30 + d.others.team.last30),
      sub: `Left out of the site's clicks: ${num(d.others.bots.last30)} bots and agents · ${num(d.others.team.last30)} from this page's browsers`,
    },
    d.latest
      ? { label: "Latest release", value: d.latest.version, sub: `Published ${dateOnly(d.latest.date)}` }
      : { label: "Latest release", value: "None", sub: "Nothing published yet" },
  ]);

  $("channel-status").replaceChildren(
    statusItem(
      "Every click",
      "success",
      h("b", {}, "urbudget.app"),
      ": clicks on the installers and on the Store button, counted as they happen. Bots, AI agents' browsers and browsers that opened this page are counted apart.",
    ),
    d.github.configured
      ? fetchedItem(
          "GitHub",
          d.github.fetched,
          "the Worker notes each release file's count every hour. The first note is the starting point; downloads count from then on.",
          "each release file's count, noted every hour; a day's downloads are the rise since the day before.",
        )
      : statusItem("Not set", "warn", h("b", {}, "GitHub"), ": set GITHUB_REPO to count downloads from GitHub's releases."),
    d.store.configured
      ? fetchedItem(
          "Microsoft Store",
          d.store.fetched,
          "the Worker asks Partner Center once a day.",
          "acquisitions and installs from Partner Center, fetched daily for the last 30 days. The latest days fill in a few days late.",
        )
      : statusItem(
          "Not connected",
          "warn",
          h("b", {}, "Microsoft Store"),
          ": set the MSSTORE_TENANT_ID, MSSTORE_CLIENT_ID and MSSTORE_CLIENT_SECRET secrets (the Entra application the Store workflow uses, with the Manager role) for the Store's own acquisitions and installs. Until then, clicks on the site's Store button stand in, which miss everyone who finds UrBudget in the Store itself.",
        ),
    sum(opens)
      ? statusItem("Counting", "success", h("b", {}, "The app"), ": copies count themselves once a day, with nothing that identifies them. First opens are copies installed and opened.")
      : statusItem(
          "None yet",
          "",
          h("b", {}, "The app"),
          ": copies count themselves from the next desktop release on, unless someone turns it off in Settings → Privacy & data.",
        ),
  );

  columnChart($("chart-downloads-90"), {
    title: "Downloads per day",
    sub: `Last 90 days · people only · ${d.store.partnerCenter ? "the Store's acquisitions from Partner Center" : "the Store as clicks on the site's Store button"}`,
    labels: d.days.map(shortDay),
    tips: d.days.map(dateOnly),
    series: channelSeries({ website, github, store }),
    empty: "No downloads counted yet.",
  });

  const opened = (label, downloads, firstOpens) =>
    h(
      "tr",
      {},
      h("td", {}, label),
      h("td", { class: "num" }, num(downloads)),
      h("td", { class: "num" }, num(firstOpens)),
      h("td", { class: "num" }, downloads ? percent(firstOpens / downloads) : "—"),
    );
  fillTable(
    $("opened-table"),
    [
      opened("urbudget.app or GitHub", lastDaysOf(website, 30) + lastDaysOf(github, 30), lastDaysOf(d.opens.direct, 30)),
      opened(d.store.partnerCenter ? "Microsoft Store" : "Microsoft Store (site's button clicks)", lastDaysOf(store, 30), lastDaysOf(d.opens.store, 30)),
    ],
    "",
  );

  const row = (channel, what, values) =>
    h(
      "tr",
      {},
      h("td", {}, channel),
      h("td", {}, what),
      [7, 30, 90].map((n) => h("td", { class: "num" }, num(lastDaysOf(values, n)))),
    );
  fillTable(
    $("channels-table"),
    [
      row("urbudget.app", "Windows installer clicks", d.website.windows),
      row("urbudget.app", "macOS installer clicks", d.website.macos),
      row("urbudget.app", "Clicks through to the Microsoft Store", d.website.store),
      row("GitHub", "Windows installers downloaded", d.github.windows),
      row("GitHub", "macOS installers downloaded", d.github.macos),
      d.store.configured ? row("Microsoft Store", "Acquisitions, from Partner Center", d.store.acquisitions) : null,
      d.store.configured ? row("Microsoft Store", "Installs, from Partner Center", d.store.installs) : null,
      row("The app", "First opens of copies from urbudget.app or GitHub", d.opens.direct),
      row("The app", "First opens of Store copies", d.opens.store),
      h(
        "tr",
        {},
        h("th", { scope: "row" }, "Every channel"),
        h("td", {}, "Downloads, as on the Overview"),
        [7, 30, 90].map((n) => h("td", { class: "num" }, h("b", {}, num(lastDaysOf(d.total, n))))),
      ),
    ].filter(Boolean),
    "",
  );

  fillTable(
    $("sources-table"),
    d.sources.map((r) =>
      h(
        "tr",
        {},
        h("td", {}, r.source ? (r.source.startsWith("/") ? h("span", { class: "mono" }, r.source) : r.source) : h("span", { class: "muted" }, "Not known")),
        h("td", { class: "num" }, num(r.n)),
      ),
    ),
    "No clicks yet.",
  );
  fillTable(
    $("versions-table"),
    d.versions.map((r) =>
      h(
        "tr",
        {},
        h("td", {}, r.version),
        h("td", {}, r.platform === "macos" ? "macOS" : "Windows"),
        h("td", { class: "num" }, num(r.n)),
        h("td", {}, dateOnly(r.last)),
      ),
    ),
    "No clicks yet.",
  );

  const githubPanel = $("github");
  if (!d.github.configured) githubPanel.replaceChildren(h("p", { class: "muted small" }, "Set GITHUB_REPO, and optionally the GITHUB_TOKEN secret, to see these."));
  else if (d.github.error) githubPanel.replaceChildren(h("p", { class: "notice notice-warn" }, d.github.error));
  else if (!d.github.releases.length) githubPanel.replaceChildren(h("p", { class: "muted small" }, "No published releases."));
  else
    githubPanel.replaceChildren(
      ...d.github.releases.map((release) =>
        h(
          "div",
          { class: "github-release" },
          h("h3", {}, release.name, " ", h("span", { class: "muted" }, release.publishedAt ? `· ${dateOnly(release.publishedAt)}` : ""), release.prerelease ? [" ", badge("Pre-release", "warn")] : null),
          h(
            "div",
            { class: "table-wrap" },
            h(
              "table",
              { class: "data-table" },
              h("thead", {}, h("tr", {}, h("th", { scope: "col" }, "File"), h("th", { scope: "col", class: "num" }, "Size"), h("th", { scope: "col", class: "num" }, "Downloads"))),
              h(
                "tbody",
                {},
                release.assets.length
                  ? release.assets.map((asset) =>
                      h("tr", {}, h("td", { class: "mono" }, asset.name), h("td", { class: "num" }, formatBytes(asset.size)), h("td", { class: "num" }, num(asset.downloads))),
                    )
                  : h("tr", { class: "empty" }, h("td", { colspan: 3 }, "No files.")),
              ),
            ),
          ),
        ),
      ),
    );
};

/* ---------- Usage: copies of the app in use ---------- */

const PLATFORM = { windows: "Windows", macos: "macOS", darwin: "macOS", linux: "Linux" };
const CHANNEL = { direct: "urbudget.app or GitHub", store: "Microsoft Store" };
/** Copies by where they were installed from; the Store in its color on every chart. */
const usageSeries = ({ direct, store }) => [
  { name: "urbudget.app or GitHub", values: direct, color: "var(--series-1)" },
  { name: "Microsoft Store", values: store, color: "var(--series-3)" },
];
/** "October 2026" from YYYY-MM. */
const monthName = (month) => new Date(`${month}-01T00:00:00Z`).toLocaleDateString("en-US", { month: "long", year: "numeric", timeZone: "UTC" });

loaders.usage = async () => {
  const u = await api("/admin/usage");
  $("usage-note").textContent = u.since
    ? `Counted since ${dateOnly(u.since)}. Each copy says once a day only whether it's new today, this week or this month, with nothing that identifies it, so these are copies rather than people: someone with two computers is two. Copies from before the usage counts, and people who turned them off in Settings → Privacy & data, aren't in these numbers; the update checks below include the older copies.`
    : "No copy has counted itself yet. Copies count themselves from the next desktop release on, once a day, with nothing that identifies them; people can turn it off in Settings → Privacy & data. Until then, the update checks below are the only sign of use.";
  const [thisWeek, lastWeek, weekBefore] = u.weeks;
  const daily = add(u.daily.direct, u.daily.store);
  const opens = add(u.opens.direct, u.opens.store);
  const currentMonth = new Date().toISOString().slice(0, 7);
  const month = u.months.find((m) => m.month === currentMonth);
  const previousMonth = u.months.find((m) => m.month < currentMonth);
  // Came back the week after: copies first opened in a week whose next week is over.
  const settled = u.cohorts.filter((c) => c.back.length >= 2).slice(0, 4);
  const settledOpens = sum(settled.map((c) => c.opens));
  const cameBack = sum(settled.map((c) => c.back[0]));
  tiles($("usage-tiles"), [
    { label: "In use this week", value: num(thisWeek?.active), sub: "Copies used since Monday (UTC)" },
    {
      label: "In use last week",
      value: num(lastWeek?.active),
      sub: `Windows ${num(lastWeek?.windows)} · macOS ${num(lastWeek?.macos)} · Store ${num(lastWeek?.store)}`,
      trend: [lastWeek?.active ?? 0, weekBefore?.active ?? 0],
      than: "the week before",
    },
    { label: "In use yesterday", value: num(daily.at(-2)), sub: `${num(daily.at(-1))} today so far` },
    {
      label: "In use this month",
      value: num(month?.active),
      sub: previousMonth ? `${num(previousMonth.active)} in ${monthName(previousMonth.month)}` : "Copies used since the 1st (UTC)",
    },
    {
      label: "First opens, 30 days",
      value: num(lastDaysOf(opens, 30)),
      sub: "Copies installed and opened",
      trend: [lastDaysOf(opens, 30), lastDaysOf(opens, 30, true)],
    },
    {
      label: "Came back the week after",
      value: settledOpens ? percent(cameBack / settledOpens) : "—",
      sub: settledOpens ? `${num(cameBack)} of ${plural(settledOpens, "copy", "copies")} first opened in the last ${plural(settled.length, "finished week")}` : "Needs a week of first opens and the whole week after it",
    },
  ]);

  const labels = u.days.map(shortDay);
  const tips = u.days.map(dateOnly);
  const none = "No copies have counted themselves yet.";
  columnChart($("chart-active-90"), {
    title: "Copies in use per day",
    sub: "Last 90 days · by where they were installed from",
    labels,
    tips,
    series: usageSeries(u.daily),
    empty: none,
  });
  columnChart($("chart-opens-90"), {
    title: "First opens per day",
    sub: "Last 90 days · copies installed and opened for the first time",
    labels,
    tips,
    series: usageSeries(u.opens),
    empty: none,
  });
  const oldestFirst = [...u.weeks].reverse();
  columnChart($("chart-weekly"), {
    title: "Copies in use per week",
    sub: "Last 12 weeks, Monday to Sunday (UTC) · the last is this week so far",
    labels: oldestFirst.map((w) => shortDay(w.start)),
    tips: oldestFirst.map((w, i) => `Week of ${dateOnly(w.start)}${i === oldestFirst.length - 1 ? " (so far)" : ""}`),
    series: usageSeries({ direct: oldestFirst.map((w) => w.direct), store: oldestFirst.map((w) => w.store) }),
    empty: none,
  });

  retention(u.cohorts, thisWeek?.start);
  fillTable(
    $("usage-weeks-table"),
    u.weeks.map((week, i) =>
      h(
        "tr",
        {},
        h("td", { class: "mono" }, shortDay(week.start), i === 0 ? h("span", { class: "muted" }, " so far") : null),
        h("td", { class: "num" }, h("b", {}, num(week.active))),
        h("td", { class: "num" }, num(week.windows)),
        h("td", { class: "num" }, num(week.macos)),
        h("td", { class: "num" }, num(week.direct)),
        h("td", { class: "num" }, num(week.store)),
        h("td", { class: "num" }, num(week.opens)),
      ),
    ),
    "No weeks yet.",
  );
  fillTable(
    $("usage-versions-table"),
    u.versions.map((r) =>
      h(
        "tr",
        {},
        h("td", {}, r.version),
        h("td", {}, PLATFORM[r.platform] || r.platform),
        h("td", {}, CHANNEL[r.channel] || r.channel),
        h("td", { class: "num" }, num(r.thisWeek)),
        h("td", { class: "num" }, num(r.lastWeek)),
      ),
    ),
    none,
  );

  columnChart($("chart-launches-90"), {
    title: "Update checks per day",
    sub: "Last 90 days · copies of 1.0.2 and later, outside the Store · about one a launch, not users or installs",
    labels,
    tips,
    series: [
      { name: "Windows", values: u.launches.windows, color: "var(--series-1)" },
      { name: "macOS", values: u.launches.macos, color: "var(--series-2)" },
    ],
    empty: "No update checks yet. Copies from 1.0.2 on are counted.",
  });
  fillTable(
    $("launch-versions-table"),
    u.launches.versions.map((r) =>
      h(
        "tr",
        {},
        h("td", {}, r.version),
        h("td", {}, PLATFORM[r.platform] || r.platform),
        h("td", { class: "num" }, num(r.n)),
        h("td", {}, dateOnly(r.last)),
      ),
    ),
    "No update checks yet.",
  );
};

/** Each week's new copies, and the share used again in each week after. */
function retention(cohorts, thisWeek) {
  const table = $("retention-table");
  const weeksAfter = Math.max(0, ...cohorts.map((c) => c.back.length));
  table.tHead.replaceChildren(
    h(
      "tr",
      {},
      h("th", { scope: "col" }, "First opened"),
      h("th", { scope: "col", class: "num" }, "Copies"),
      Array.from({ length: weeksAfter }, (_, k) => h("th", { scope: "col", class: "num" }, `Week ${k + 1}`)),
    ),
  );
  fillTable(
    table,
    cohorts.map((cohort) =>
      h(
        "tr",
        {},
        h("td", { class: "mono" }, `Week of ${shortDay(cohort.week)}`),
        h("td", { class: "num" }, num(cohort.opens)),
        Array.from({ length: weeksAfter }, (_, k) => {
          if (k >= cohort.back.length) return h("td", {});
          const share = cohort.opens ? Math.min(1, cohort.back[k] / cohort.opens) : 0;
          const current = addDays(cohort.week, 7 * (k + 1)) === thisWeek;
          return h(
            "td",
            {
              class: "num share",
              style: `--share:${share.toFixed(2)}`,
              title: `${num(cohort.back[k])} of ${plural(cohort.opens, "copy", "copies")} used in week ${k + 1}${current ? " (this week, so far)" : ""}`,
            },
            percent(share),
            h("small", {}, `${num(cohort.back[k])}${current ? " so far" : ""}`),
          );
        }),
      ),
    ),
    "No copies first opened yet.",
  );
}

/* ---------- Revenue ---------- */

let revenueDays = "30";
// The order tiles appear in; stock numbers (MRR, active subscriptions) show their latest value.
const TILES = [
  ["revenue", "total"],
  ["net_revenue", "total"],
  ["orders", "total"],
  ["one_time_products", "total"],
  ["monthly_recurring_revenue", "latest"],
  ["active_subscriptions", "latest"],
  ["new_subscriptions", "total"],
  ["churned_subscriptions", "total"],
  ["checkouts_conversion", "total"],
];
const PERIOD = { day: "day", week: "week", month: "month" };

function metricFormat(type) {
  if (type === "currency") return money;
  if (type === "percentage") return (v) => percent(v);
  return num;
}
function periodLabel(start, interval, long = false) {
  const date = new Date(start);
  if (interval === "month") return date.toLocaleDateString("en-US", { month: long ? "long" : "short", year: "numeric", timeZone: "UTC" });
  const day = date.toLocaleDateString("en-US", { month: "short", day: "numeric", ...(long ? { year: "numeric" } : {}), timeZone: "UTC" });
  return interval === "week" && long ? `Week of ${day}` : day;
}

for (const button of document.querySelectorAll(".segmented [data-days]")) {
  button.addEventListener("click", () => {
    revenueDays = button.dataset.days;
    for (const other of document.querySelectorAll(".segmented [data-days]")) other.setAttribute("aria-checked", String(other === button));
    run("revenue");
  });
}

loaders.revenue = async () => {
  const error = $("revenue-error");
  let r;
  try {
    r = await api(`/admin/revenue?days=${revenueDays}`);
  } catch (failure) {
    if (!(failure instanceof ApiError) || failure.code === "not_admin" || failure.status === 0) throw failure;
    error.textContent = failure.message;
    error.hidden = false;
    $("revenue-tiles").replaceChildren();
    $("chart-revenue").hidden = true;
    $("revenue-table").tHead.replaceChildren();
    fillTable($("revenue-table"), [], "No numbers.");
    return;
  }
  error.hidden = true;
  $("revenue-source").textContent = `From Polar (${r.environment === "production" ? "live" : "sandbox"}), by ${PERIOD[r.interval]}, since ${dateOnly(r.start)}`;
  const known = new Map(r.metrics.map((m) => [m.slug, m]));
  const latest = r.periods.at(-1) ?? {};
  const items = [];
  for (const [slug, how] of TILES) {
    const metric = known.get(slug);
    if (!metric) continue;
    const value = how === "latest" ? latest[slug] : r.totals[slug];
    items.push({ label: metric.name, value: value === null || value === undefined ? "—" : metricFormat(metric.type)(value), sub: how === "latest" ? "Now" : null });
  }
  tiles($("revenue-tiles"), items);

  const revenue = known.get("revenue");
  $("chart-revenue").hidden = !revenue;
  if (revenue)
    columnChart($("chart-revenue"), {
      title: `Revenue per ${PERIOD[r.interval]}`,
      sub: "Before Polar's fees and refunds",
      labels: r.periods.map((p) => periodLabel(p.start, r.interval)),
      tips: r.periods.map((p) => periodLabel(p.start, r.interval, true)),
      series: [{ name: "Revenue", values: r.periods.map((p) => p.revenue || 0), color: "var(--series-1)" }],
      format: money,
      axis: (cents) => `$${compact.format(cents / 100)}`,
      empty: "No revenue in this range yet.",
    });

  const columns = r.metrics;
  $("revenue-table").tHead.replaceChildren(
    h("tr", {}, h("th", { scope: "col" }, "Period"), columns.map((m) => h("th", { scope: "col", class: "num" }, m.name))),
  );
  fillTable(
    $("revenue-table"),
    [...r.periods].reverse().map((period) =>
      h(
        "tr",
        {},
        h("td", {}, periodLabel(period.start, r.interval, true)),
        columns.map((m) => h("td", { class: "num" }, period[m.slug] === null ? "—" : metricFormat(m.type)(period[m.slug]))),
      ),
    ),
    "No periods.",
  );
};

/* ---------- Health ---------- */

const PROBLEM = { email: "Email not sent", webhook: "Webhook failed", rate_limit: "Rate limit hit" };
// What each counted event is, in the order the Activity table lists them.
const ACTIVITY = {
  "sign_in:web": "Sign-ins on the website",
  "sign_in:desktop": "Sign-ins in the app",
  "sign_in:link": "The app opening the website signed in",
  "email:sign-in code": "Sign-in codes emailed",
  "checkout:sync": "Sync checkouts opened",
  "sync_upload:ok": "Budgets uploaded to sync",
  "sync_upload:conflict": "Sync uploads refused: another computer synced first",
  "account_deleted:": "Accounts deleted by their owners",
};
const activityOrder = Object.keys(ACTIVITY);
const activityRank = (key) => (activityOrder.includes(key) ? activityOrder.indexOf(key) : activityOrder.length);

let reportOffset = 0;
let reportsBusy = false;

async function loadAssistantReports(offset = reportOffset) {
  if (reportsBusy) return;
  reportsBusy = true;
  $("assistant-reports-newer").disabled = true;
  $("assistant-reports-older").disabled = true;
  setStatus($("assistant-reports-status"), "");
  try {
    const data = await api(`/admin/assistant-reports?offset=${offset}`);
    reportOffset = offset;
    $("assistant-reports").replaceChildren(...data.reports.map((report) => {
      const remove = h("button", { class: "btn btn-sm", type: "button", onclick: async () => {
        if (!window.confirm(`Delete report ${report.id}? This cannot be undone.`)) return;
        remove.disabled = true;
        try {
          await api(`/admin/assistant-reports/${encodeURIComponent(report.id)}`, { method: "DELETE" });
          loaded.delete("log");
          await loadAssistantReports();
        } catch (error) {
          setStatus($("assistant-reports-status"), error.message, "error");
          remove.disabled = false;
        }
      } }, "Delete report");
      // Treat submitted text as text, including markup, links and instructions.
      return h("details", { class: "assistant-report" },
        h("summary", {}, `${when(report.createdAt)} · ${report.id}`),
        h("p", {}, h("strong", {}, "Reason: "), report.reason || "No reason supplied."),
        h("pre", {}, report.reply), remove);
    }));
    if (!data.reports.length) $("assistant-reports").append(h("p", { class: "muted" }, "No reports on this page."));
    $("assistant-reports-newer").disabled = reportOffset === 0;
    $("assistant-reports-older").disabled = !data.hasMore;
  } catch (error) {
    setStatus($("assistant-reports-status"), error.message, "error");
    $("assistant-reports-newer").disabled = reportOffset === 0;
  } finally {
    reportsBusy = false;
  }
}

$("assistant-reports-newer").addEventListener("click", () => loadAssistantReports(Math.max(0, reportOffset - 50)));
$("assistant-reports-older").addEventListener("click", () => loadAssistantReports(reportOffset + 50));

loaders.health = async () => {
  await loadAssistantReports();
  const data = await api("/admin/health");
  const mail = data.emails;
  const usage = (sent, limit) => badge(`${num(sent)} of ${num(limit)}`, nearLimit(sent, limit) ? "warn" : "success");
  const late = cleanupLate(data.lastCleanup, data.countingSince);
  $("activity-limits").replaceChildren(
    h("li", {}, usage(mail.today, mail.perDay), "Emails sent today"),
    h("li", {}, usage(mail.month, mail.perMonth), "Emails sent this month"),
    h(
      "li",
      {},
      data.lastCleanup ? badge(dateOnly(data.lastCleanup), late ? "warn" : "success") : badge("Not yet", late ? "warn" : ""),
      "Daily clean-up last ran",
    ),
  );
  $("activity-note").textContent = data.countingSince
    ? `Daily totals counted since ${dateOnly(data.countingSince)}, nothing about who. Email limits are Resend's free plan.`
    : "Nothing counted yet: counting starts with this version of the Worker. Email limits are Resend's free plan.";
  fillTable(
    $("activity-table"),
    [...data.activity]
      .sort((a, b) => activityRank(`${a.name}:${a.detail}`) - activityRank(`${b.name}:${b.detail}`))
      .map((row) =>
        h(
          "tr",
          {},
          h("td", {}, ACTIVITY[`${row.name}:${row.detail}`] || `${row.name}${row.detail ? ` (${row.detail})` : ""}`),
          h("td", { class: "num" }, num(row.last7)),
          h("td", { class: "num" }, num(row.last30)),
        ),
      ),
    "Nothing in the last 30 days.",
  );
  $("settings").replaceChildren(
    ...data.settings.map((setting) =>
      h(
        "li",
        {},
        setting.value ? badge(setting.value, setting.ok ? "success" : "warn") : setting.ok ? badge("Set", "success") : badge("Not set", "warn"),
        h("code", {}, setting.name),
      ),
    ),
  );
  fillTable(
    $("problems-table"),
    data.problems.map((p) =>
      h("tr", {}, h("td", { class: "mono" }, when(p.at)), h("td", {}, PROBLEM[p.kind] || p.kind), h("td", {}, p.subject), h("td", { class: "detail" }, p.detail)),
    ),
    "No problems in the last 90 days.",
  );
  fillTable(
    $("webhook-types"),
    data.webhookTypes.map((w) => h("tr", {}, h("td", { class: "mono" }, w.type), h("td", { class: "num" }, num(w.n)), h("td", {}, when(w.last)))),
    "No webhooks in the last 30 days.",
  );
  fillTable(
    $("webhook-recent"),
    data.webhooks.map((w) => h("tr", {}, h("td", { class: "mono" }, w.type), h("td", {}, when(w.at)))),
    "No webhooks yet.",
  );
};

/* ---------- Log ---------- */

const ACTIONS = { delete_assistant_report: "Deleted assistant report" };

loaders.log = async () => {
  const { entries } = await api("/admin/log");
  fillTable(
    $("log-table"),
    entries.map((e) =>
      h(
        "tr",
        {},
        h("td", { class: "mono" }, when(e.at)),
        h("td", {}, e.admin),
        h("td", {}, ACTIONS[e.action] || e.action),
        h("td", { class: "detail" }, e.detail),
      ),
    ),
    "Nothing changed from this page yet.",
  );
};

/* ---------- Start ---------- */

const first = location.hash.slice(1);
select(loaders[first] ? first : "overview");
// The header (who's signed in, sandbox or live) comes with the overview.
if (current !== "overview") run("overview");
