// The assistant section's sample conversations. Pick a question and it plays like the app's
// assistant panel: the question, what the assistant is working on, its answer, and the card it
// proposes, whose Apply, Discard and Undo work the way they do in the app. The first time you
// stop at the section, a cursor asks the first question for you (ghost.js); after that nothing
// plays until you pick one, so it never passes for a video. Without this script the panel shows
// one finished conversation.

import { showOnce } from "./ghost.js";

const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

const ICONS = {
  check: '<path d="M3.5 8.5l3 3 6-7"/>',
  undo: '<path d="M5 3L2 6l3 3"/><path d="M2 6h7.5a4 4 0 010 8H7"/>',
  flag: '<path d="M3 14V2.5M3 3h8l-1.6 3L11 9H3"/>',
  warn: '<path d="M8 2l6.5 11.5h-13z"/><path d="M8 6.5v3M8 11.6v.1"/>',
  rule: '<path d="M2.5 4h11M2.5 8h7M2.5 12h4"/><path d="M11 10.5l1.5 1.5 2.5-3"/>',
  image: '<rect x="2" y="2.5" width="12" height="11" rx="2"/><circle cx="6" cy="6.3" r="1.2"/><path d="M14 10.5l-3.5-3.5L4 13.5"/>',
  pdf: '<path d="M4 1.5h5.5L13 5v9.5H4z"/><path d="M9.5 1.5V5H13"/><path d="M6.3 9h4.4M6.3 11.5h3"/>',
  doc: '<path d="M4 1.5h5.5L13 5v9.5H4z"/><path d="M9.5 1.5V5H13"/>',
};
const icon = (name, size = 13) =>
  `<svg viewBox="0 0 16 16" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`;

// Category colours, as the app's reports draw them.
const C = { dining: "#e5566a", groceries: "#1f9d6b", coffee: "#8b5a2b", household: "#2f7fe0", fuel: "#ef8a2e", fun: "#7c5cff", bills: "#d9a400" };

const SCENARIOS = {
  month: {
    ask: "How is October going?",
    think: "Checking October's categories",
    reply: [
      "Mostly on track. You've spent $3,280.88 of the $10,798.00 you assigned, and the car insurance due on the 20th ($135.00) is already set aside.",
      [
        "Groceries is $64.20 over, after the Costco run on the 12th.",
        "Coffee is $6.57 over.",
        "Dining out still has $115.80, more than you usually spend in the second half of a month.",
      ],
      "Want me to cover the two overspent categories from Dining out?",
    ],
  },
  cover: {
    ask: "Cover the 2 overspent categories",
    think: "Finding money for Groceries and Coffee",
    reply: ["Groceries is $64.20 over and Coffee $6.57. Dining out has $115.80 and no target, so this takes both from there and leaves it $45.03."],
    card: {
      kind: "apply",
      kicker: "Proposal · Budget",
      title: "Cover overspending in October",
      rows: [
        { from: ["Dining out", C.dining], to: ["Groceries", C.groceries], amount: "$64.20" },
        { from: ["Dining out", C.dining], to: ["Coffee", C.coffee], amount: "$6.57" },
      ],
      total: ["Moved", "$70.77"],
    },
  },
  receipt: {
    ask: "Add this receipt to the Visa",
    file: ["costco-receipt.jpg", "image"],
    think: "Reading the receipt",
    reply: ["A Costco receipt from October 16 for $186.40. The food is groceries, and the paper towels and detergent are household, so I split it."],
    card: {
      kind: "apply",
      kicker: "Proposal · Visa Signature",
      title: "Add 1 transaction",
      lead: "Costco · Oct 16",
      rows: [
        { label: ["Groceries", C.groceries], amount: "$142.10" },
        { label: ["Household", C.household], amount: "$44.30" },
      ],
      total: ["Outflow", "$186.40"],
    },
  },
  categorize: {
    ask: "Categorize the 6 new transactions",
    think: "Checking how you categorized these payees before",
    reply: [
      "Five match how you've categorized these payees before. I'm not sure about the Amazon order, so I marked it. Shell always goes to Fuel, so the card also adds a rule for next time.",
    ],
    card: {
      kind: "apply",
      kicker: "Proposal · Transactions",
      title: "Categorize 6 transactions",
      rows: [
        { payee: "Trader Joe's", label: ["Groceries", C.groceries], amount: "$86.14" },
        { payee: "Shell", label: ["Fuel", C.fuel], amount: "$48.90" },
        { payee: "Netflix", label: ["Subscriptions", C.fun], amount: "$17.00" },
        { payee: "Peet's Coffee", label: ["Coffee", C.coffee], amount: "$5.25" },
        { payee: "Planet Fitness", label: ["Fitness", C.fun], amount: "$60.00" },
        { payee: "Amazon", label: ["Household", C.household], amount: "$32.99", unsure: true },
      ],
      note: ["rule", "New payee rule: Shell → Fuel"],
    },
  },
  statement: {
    ask: "Import this statement",
    file: ["amex-october.pdf", "pdf"],
    think: "Reading the statement's 3 pages",
    reply: ["I found 38 transactions, and they add up to the statement's total. You'll see duplicates and matches in the import review before anything is added."],
    card: {
      kind: "import",
      kicker: "Import file · Amex Gold",
      title: "38 transactions from amex-october.pdf",
      stats: [
        ["Rows", "38"],
        ["Money out", "$1,041.42"],
        ["Dates", "Sep 18 – Oct 17"],
      ],
      check: "Matches the statement: 38 rows, $1,041.42",
    },
  },
  report: {
    ask: "Make a PDF of how October went",
    think: "Gathering October's numbers",
    reply: ["Your October review is ready. It covers spending by group, what changed from September, and a few things to do next."],
    card: {
      kind: "report",
      kicker: "Report",
      title: "October 2026 in review",
      sections: ["Overview", "Spending by group", "Compared with September", "What to do next"],
    },
  },
};
const ORDER = ["month", "cover", "receipt", "categorize", "statement", "report"];

/* ---------- Drawing ---------- */

const swatch = ([name, color]) => `<i class="ai-sw" style="--c: ${color}"></i>${esc(name)}`;

function cardRow(row) {
  let label;
  if (row.from) label = `${swatch(row.from)} <span class="ai-arrow">→</span> ${swatch(row.to)}`;
  else if (row.payee) label = `<span class="ai-payee">${esc(row.payee)}</span> <span class="ai-arrow">→</span> ${swatch(row.label)}`;
  else label = swatch(row.label);
  const unsure = row.unsure ? ` <span class="ai-unsure">${icon("warn", 11)}Unsure</span>` : "";
  return `<div class="ai-row"><span class="ai-row-label">${label}${unsure}</span><span class="num">${esc(row.amount)}</span></div>`;
}

const ACTIONS = {
  apply: [["Apply", "apply", true], ["Discard", "discard"]],
  import: [["Review import", "In the app, this opens the import review, where you check duplicates and matches.", true], ["Save as CSV", "In the app, this saves the rows as a CSV file."], ["Discard", "discard", false, "link"]],
  report: [["Preview", "In the app, this opens the report, ready to read or print.", true], ["Print or save as PDF", "In the app, this saves the report as a PDF."], ["Excel", "In the app, this saves each table on its own sheet."], ["Discard", "discard", false, "link"]],
};

function cardHtml(card) {
  const parts = [`<div class="ai-card-head"><small>${esc(card.kicker)}</small><b>${esc(card.title)}</b></div>`];
  if (card.lead) parts.push(`<div class="ai-lead">${esc(card.lead)}</div>`);
  if (card.stats) parts.push(`<div class="ai-stats">${card.stats.map(([k, v]) => `<div><small>${esc(k)}</small><b class="num">${esc(v)}</b></div>`).join("")}</div>`);
  if (card.rows) parts.push(card.rows.map(cardRow).join(""));
  if (card.sections) parts.push(`<ol class="ai-sections">${card.sections.map((s) => `<li>${icon("doc", 12)}${esc(s)}</li>`).join("")}</ol>`);
  if (card.total) parts.push(`<div class="ai-row ai-total"><span>${esc(card.total[0])}</span><span class="num">${esc(card.total[1])}</span></div>`);
  if (card.check) parts.push(`<div class="ai-check">${icon("check")}${esc(card.check)}</div>`);
  if (card.note) parts.push(`<div class="ai-note">${icon(card.note[0])}${esc(card.note[1])}</div>`);
  const buttons = ACTIONS[card.kind]
    .map(([label, act, primary, look]) => {
      const cls = primary ? "ai-btn ai-btn-primary" : look === "link" ? "ai-btn ai-btn-link" : "ai-btn";
      const attr = act === "apply" || act === "discard" ? `data-act="${act}"` : `data-demo="${esc(act)}"`;
      return `<button type="button" class="${cls}" ${attr}>${esc(label)}</button>`;
    })
    .join("");
  const hint = card.kind === "apply" ? '<span class="ai-hint">One undo step</span>' : "";
  parts.push(`<div class="ai-actions">${buttons}${hint}</div><p class="ai-demo-note" hidden></p>`);
  return `<div class="ai-card" data-title="${esc(card.title)}">${parts.join("")}</div>`;
}

function doneHtml(state, title) {
  const label = { applied: "Applied", undone: "Undone", discarded: "Discarded" }[state];
  const button =
    state === "applied"
      ? '<button type="button" class="ai-btn" data-act="undo">Undo</button>'
      : state === "undone"
        ? '<button type="button" class="ai-btn" data-act="apply">Apply again</button>'
        : "";
  return `<span class="ai-done-label">${icon(state === "applied" ? "check" : "undo", 14)}${label}: ${esc(title)}</span>${button}`;
}

function askHtml(s) {
  const file = s.file ? `<span class="ai-file">${icon(s.file[1], 14)}${esc(s.file[0])}</span>` : "";
  return `<div class="ai-msg ai-me">${file}<p>${esc(s.ask)}</p></div>`;
}

/* ---------- The panel ---------- */

export function assistantDemo() {
  const root = document.querySelector("[data-ai]");
  if (!root) return;
  const log = root.querySelector("[data-ai-log]");
  const buttons = [...root.querySelectorAll("[data-ai-play]")];
  let run = 0;
  let current = null;

  const toBottom = () => {
    log.scrollTop = log.scrollHeight;
  };
  const mark = (name) => {
    for (const b of buttons) b.setAttribute("aria-current", String(b.dataset.aiPlay === name));
  };

  // Reveals a paragraph at an even pace, like the app does. The pace follows the clock, so a
  // browser that slows timers down doesn't slow the text.
  const type = async (el, text, id, instant) => {
    if (instant) {
      el.textContent = text;
      return;
    }
    const start = performance.now();
    let shown = 0;
    while (shown < text.length) {
      if (id !== run) return;
      shown = Math.min(text.length, Math.ceil((performance.now() - start) * 0.11));
      el.textContent = text.slice(0, shown);
      toBottom();
      await sleep(16);
    }
  };

  // Plays a sample, or with instant shows it finished.
  const play = async (name, instant = reduceMotion) => {
    const id = ++run;
    const s = SCENARIOS[name];
    current = name;
    mark(name);
    log.setAttribute("aria-busy", "true");
    log.innerHTML = askHtml(s);
    const bot = document.createElement("div");
    bot.className = "ai-msg ai-bot";
    if (!instant) {
      await sleep(420);
      if (id !== run) return;
      bot.innerHTML = `<p class="ai-think">${esc(s.think)}</p>`;
      log.append(bot);
      toBottom();
      await sleep(1300);
      if (id !== run) return;
    } else {
      log.append(bot);
    }
    bot.innerHTML = "";
    for (const part of s.reply) {
      if (Array.isArray(part)) {
        const ul = document.createElement("ul");
        bot.append(ul);
        for (const item of part) {
          const li = document.createElement("li");
          ul.append(li);
          await type(li, item, id, instant);
          if (id !== run) return;
        }
      } else {
        const p = document.createElement("p");
        bot.append(p);
        await type(p, part, id, instant);
        if (id !== run) return;
      }
    }
    if (s.card) {
      if (!instant) await sleep(250);
      if (id !== run) return;
      bot.insertAdjacentHTML("beforeend", cardHtml(s.card));
    }
    bot.insertAdjacentHTML("beforeend", `<button type="button" class="ai-flag" data-demo="Every reply in the app has this link, to report a reply that's wrong or harmful.">${icon("flag", 11)}Report</button>`);
    toBottom();
    log.setAttribute("aria-busy", "false");
  };

  for (const b of buttons) b.addEventListener("click", () => play(b.dataset.aiPlay));

  // The card's buttons.
  log.addEventListener("click", (event) => {
    const button = event.target.closest("button");
    if (!button) return;
    const card = button.closest(".ai-card");
    if (button.dataset.demo) {
      const note = card?.querySelector(".ai-demo-note") ?? null;
      if (note) {
        note.textContent = button.dataset.demo;
        note.hidden = false;
      } else {
        button.title = button.dataset.demo;
        button.classList.add("is-noted");
        button.insertAdjacentHTML("afterend", `<span class="ai-flag-note">${esc(button.dataset.demo)}</span>`);
        button.disabled = true;
      }
      return;
    }
    if (!card) return;
    // Apply (or Apply again after Undo), Discard and Undo leave a one-line card, as in the app.
    const state = { apply: "applied", discard: "discarded", undo: "undone" }[button.dataset.act];
    if (!state) return;
    card.classList.add("is-done");
    card.innerHTML = doneHtml(state, card.dataset.title);
    (card.querySelector("button") ?? log).focus();
  });

  root.classList.add("is-live");
  // The conversation already on the page becomes a working one.
  void play("cover", true);
  // The cursor asks how the month is going. Its answer offers to cover the overspending, and
  // "Your turn" points at the question that does.
  showOnce(root, "assistant", () => {
    if (current !== "cover") return null;
    const pick = (name) => buttons.find((b) => b.dataset.aiPlay === name);
    return { target: pick(ORDER[0]), fx: 0.3, act: () => play(ORDER[0]), pause: 1700, next: pick("cover"), nextFx: 0.3 };
  });
}
