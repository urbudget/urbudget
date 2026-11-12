// The home page: the budget you can fill, the download button's label, the tour's sticky
// picture and the sealed ledger. The page reads fine without any of it; scroll-linked motion
// is in home.css.

import { assistantDemo } from "./ai-demo.js";
import { showOnce } from "./ghost.js";
import { reports } from "./reports.js";

const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
const verb = matchMedia("(hover: none)").matches ? "Tap" : "Click";
const money = (n, cents = true) =>
  n.toLocaleString("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: cents ? 2 : 0,
    maximumFractionDigits: cents ? 2 : 0,
  });

/* ---------- "Download for Windows" / "Download for Mac" ---------- */

function downloadLabel() {
  const platform = navigator.userAgentData?.platform || navigator.platform || "";
  const phone = /iphone|ipad|android/i.test(navigator.userAgent);
  const label = phone ? "" : /mac/i.test(platform) ? "Download for Mac" : /win/i.test(platform) ? "Download for Windows" : "";
  if (!label) return;
  for (const link of document.querySelectorAll("[data-download-label]")) link.textContent = label;
}

/* ---------- The budget you can fill ---------- */

function demo() {
  const root = document.querySelector("[data-demo]");
  if (!root) return;
  const poolEl = root.querySelector("[data-demo-pool]");
  const label = root.querySelector("[data-demo-label]");
  const hint = root.querySelector("[data-demo-hint]");
  const quick = root.querySelector("[data-demo-quick]");
  const reset = root.querySelector("[data-demo-reset]");
  const envs = [...root.querySelectorAll(".env")];
  const total = envs.reduce((sum, env) => sum + Number(env.dataset.amount), 0);
  let pool = total;
  let shown = total;
  let frame = 0;
  let running = false;

  // The pot counts down instead of jumping.
  const showPool = () => {
    cancelAnimationFrame(frame);
    if (reduceMotion) {
      shown = pool;
      poolEl.textContent = money(shown);
      return;
    }
    const from = shown;
    const to = pool;
    const start = performance.now();
    const tick = (now) => {
      const t = Math.min(1, (now - start) / 650);
      shown = from + (to - from) * (1 - (1 - t) ** 3);
      poolEl.textContent = money(shown);
      if (t < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
  };

  // A dot in the envelope's color flies from the pot to the envelope's bar.
  const flyDot = (env) => {
    if (reduceMotion) return;
    const from = poolEl.getBoundingClientRect();
    const to = env.querySelector(".env-bar").getBoundingClientRect();
    const dot = document.createElement("span");
    dot.className = "fly-dot";
    dot.setAttribute("aria-hidden", "true");
    dot.style.setProperty("--c", env.style.getPropertyValue("--c"));
    dot.style.transform = `translate(${from.right - 10}px, ${from.top + from.height / 2 - 5}px)`;
    document.body.append(dot);
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        dot.style.transform = `translate(${to.left + to.width / 2 - 5}px, ${to.top - 5}px) scale(0.6)`;
        dot.style.opacity = "0";
      }),
    );
    setTimeout(() => dot.remove(), 800);
  };

  const setFill = (env, full) => {
    const amount = money(Number(env.dataset.amount), false);
    env.classList.toggle("is-full", full);
    if (full) env.setAttribute("aria-disabled", "true");
    else env.removeAttribute("aria-disabled");
    env.querySelector(".env-bar i").style.setProperty("--fill", full ? "1" : "0");
    env.querySelector("[data-env-chip]").textContent = full ? amount : `+${amount}`;
  };

  const update = (filled) => {
    const done = pool <= 0;
    root.classList.toggle("is-done", done);
    label.textContent = done ? "Nothing left to assign" : "Ready to assign";
    if (done) hint.textContent = `All ${money(total, false)} has somewhere to go`;
    else if (filled) hint.textContent = `${filled.querySelector(".env-name").textContent}: ${money(Number(filled.dataset.amount), false)} assigned. ${money(pool, false)} left`;
    else hint.textContent = `${verb} an envelope to fill it`;
    const quickHadFocus = document.activeElement === quick;
    quick.hidden = done;
    reset.hidden = pool === total;
    if (quickHadFocus && done) reset.focus();
  };

  const fill = (env) => {
    if (env.classList.contains("is-full")) return false;
    root.classList.add("is-touched");
    setFill(env, true);
    flyDot(env);
    pool -= Number(env.dataset.amount);
    showPool();
    update(env);
    return true;
  };

  for (const env of envs) env.addEventListener("click", () => fill(env));

  quick.addEventListener("click", async () => {
    if (running) return;
    running = true;
    for (const env of envs) {
      if (fill(env) && !reduceMotion) await new Promise((resolve) => setTimeout(resolve, 170));
    }
    running = false;
  });

  reset.addEventListener("click", () => {
    pool = total;
    for (const env of envs) setFill(env, false);
    showPool();
    update(null);
    envs[0].focus();
  });

  root.classList.add("is-live");
  update(null);
  // The cursor fills the first empty envelope, then points at the next one.
  showOnce(root, "budget", () => {
    const open = envs.filter((env) => !env.classList.contains("is-full"));
    if (open.length < 2) return null;
    return {
      target: open[0].querySelector(".env-chip"),
      act: () => fill(open[0]),
      next: open[1].querySelector(".env-chip"),
      nextFx: 0.35,
      nextFy: 0.75,
    };
  });
}

/* ---------- Tour: the step in the middle of the screen picks the picture ---------- */

function tour() {
  const steps = [...document.querySelectorAll("[data-tour-step]")];
  if (!steps.length) return;
  const frames = [...document.querySelectorAll("[data-tour-frame]")];
  const activate = (step) => {
    for (const s of steps) s.classList.toggle("is-active", s === step);
    for (const f of frames) f.classList.toggle("is-active", f.dataset.tourFrame === step.dataset.tourStep);
  };
  activate(steps[0]);
  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) if (entry.isIntersecting) activate(entry.target);
    },
    { rootMargin: "-45% 0px -45% 0px" },
  );
  for (const step of steps) observer.observe(step);
}

/* ---------- The sealed ledger keeps reshuffling while you look at it ---------- */

function cipher() {
  const root = document.querySelector("[data-cipher]");
  if (!root || reduceMotion) return;
  const cells = [...root.querySelectorAll("li span")];
  const glyphs = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz0123456789+/=";
  const scramble = (text) => [...text].map((ch) => (ch === " " ? " " : glyphs[Math.floor(Math.random() * glyphs.length)])).join("");
  let timer = 0;
  const observer = new IntersectionObserver(([entry]) => {
    clearInterval(timer);
    if (entry.isIntersecting) {
      timer = setInterval(() => {
        const cell = cells[Math.floor(Math.random() * cells.length)];
        cell.textContent = scramble(cell.textContent);
      }, 120);
    }
  });
  observer.observe(root);
}

downloadLabel();
demo();
tour();
reports();
assistantDemo();
cipher();
