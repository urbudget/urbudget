import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { demoBudget } from "../demo";
import { money } from "../format";
import type { CardRow, Proposal, Revision } from "./api";
import { ProposalCard, SETTLE, appliedTime, type CardHandlers } from "./ProposalCard";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const rows: CardRow[] = [
  {
    label: "Careem · Oct 3",
    detail: "Uncategorized → Transport",
    amount: -3800,
    key: "t:a",
    edit: { category: true, categoryId: "transport", amount: false },
  },
  {
    label: "Extra Electronics · Oct 5",
    detail: "Uncategorized → Gifts",
    amount: -89900,
    badge: "Check this",
    tone: "warn",
    key: "t:b",
    edit: { category: true, categoryId: "gifts", amount: false },
  },
  { label: "New rule: payees containing “careem”", detail: "Transport", badge: "rule", key: "r:r1" },
];
/** What a waiting change offers, and what it offers once the user has chosen on it. */
const WAITING = ["apply", "askChanges", "discard"];
const CHOSEN = ["apply", "askChanges", "useOriginal", "discard"];
const draft = (more: Partial<Proposal> = {}): Proposal => ({
  id: "p1",
  title: "Categorize 2 transactions",
  kind: "transactions",
  status: "proposed",
  made: false,
  actions: WAITING,
  rows: 0,
  sheets: 0,
  card: { kicker: "Proposal · Transactions", rows, stats: [], more: 0, highlight: { categories: [], transactions: [] } },
  ...more,
});
/** Every button's doing, each one a mock to ask about. */
const handlers = (over: Partial<CardHandlers> = {}): CardHandlers => ({
  apply: vi.fn(async () => {}),
  revise: vi.fn(async () => draft()),
  discard: vi.fn(async () => {}),
  restore: vi.fn(async () => {}),
  undo: vi.fn(async () => {}),
  askChanges: vi.fn(),
  askAgain: vi.fn(),
  show: vi.fn(),
  answer: vi.fn(),
  review: vi.fn(),
  saveCsv: vi.fn(async () => {}),
  preview: vi.fn(),
  print: vi.fn(async () => {}),
  sheet: vi.fn(async () => {}),
  ...over,
});
const show = (proposal: Proposal, on = handlers(), props: Partial<Parameters<typeof ProposalCard>[0]> = {}) => {
  render(<ProposalCard proposal={proposal} budget={demoBudget()} canUndo on={on} {...props} />);
  return on;
};

/** The card as the panel keeps it: each revision made by `engine`, the card it gives shown. */
function Card({ engine, on = {}, start = draft() }: { engine: (r: Revision, p: Proposal) => Proposal; on?: Partial<CardHandlers>; start?: Proposal }) {
  const [p, setP] = useState(start);
  return (
    <ProposalCard
      proposal={p}
      budget={demoBudget()}
      canUndo
      on={handlers({
        revise: async (r) => {
          const next = engine(r, p);
          setP(next);
          return next;
        },
        ...on,
      })}
    />
  );
}
/** What the engine does with a revision, as far as the card shows it. */
const marking = (r: Revision, p: Proposal): Proposal => ({
  ...p,
  choices: r.skip.length || r.set.length ? r : undefined,
  actions: r.skip.length || r.set.length ? CHOSEN : WAITING,
  card: {
    ...p.card,
    rows: rows.map((row) => {
      const set = r.set.find((s) => s.key === row.key);
      // The category a row gives is the user's once they chose one.
      const edit = row.edit && set?.category !== undefined ? { ...row.edit, categoryId: set.category ?? undefined } : row.edit;
      return { ...row, skipped: r.skip.includes(row.key!), edited: !!set, amount: set?.amount ?? row.amount, edit };
    }),
  },
});
/** Only the pause before a card's choices are sent is faked: everything else keeps real time. */
const fakePause = () => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
/** Enough turns of the queue for a call to the core, and what follows it, to run their course. */
const settled = async () => {
  for (let i = 0; i < 50; i++) await Promise.resolve();
};
/** What a button started has run its course. */
const turn = () => act(settled);
/** The pause after the last choice passes, and what was sent comes back. */
const pause = (ms = SETTLE) =>
  act(async () => {
    vi.advanceTimersByTime(ms);
    await settled();
  });
const box = (name: string) => screen.getByRole("checkbox", { name: `Apply: ${name}` }) as HTMLInputElement;
const button = (name: string | RegExp) => screen.getByRole("button", { name }) as HTMLButtonElement;

it("shows a tick at once, and sends the choices once, a moment after the last", async () => {
  fakePause();
  const engine = vi.fn(marking);
  render(<Card engine={engine} />);
  expect(button("Apply")).toBeTruthy();
  fireEvent.click(box("Extra Electronics · Oct 5"));
  // At once, from the card itself: nothing has gone to the core, and nothing is busy.
  expect(box("Extra Electronics · Oct 5").checked).toBe(false);
  expect(button("Apply 2 of 3").disabled).toBe(false);
  expect(box("Careem · Oct 3").disabled).toBe(false);
  expect(engine).not.toHaveBeenCalled();
  await pause(SETTLE - 100);
  fireEvent.click(box("Careem · Oct 3"));
  expect(button("Apply 1 of 3")).toBeTruthy();
  // The pause starts again with each choice.
  await pause(SETTLE - 100);
  expect(engine).not.toHaveBeenCalled();
  await pause(100);
  expect(engine).toHaveBeenCalledTimes(1);
  expect(engine).toHaveBeenLastCalledWith({ skip: ["t:b", "t:a"], set: [] }, expect.anything());
  // The core's card says the same, so nothing moves.
  expect(box("Careem · Oct 3").checked).toBe(false);
  expect(button("Apply 1 of 3")).toBeTruthy();
  // Nothing chosen: nothing to apply.
  fireEvent.click(box("New rule: payees containing “careem”"));
  expect(button("Apply 0 of 3").disabled).toBe(true);
  // And all of it again, as the assistant made it: sent without waiting.
  await pause();
  fireEvent.click(button("Use the assistant's version"));
  await pause(0);
  expect(engine).toHaveBeenLastCalledWith({ skip: [], set: [] }, expect.anything());
  expect(button("Apply")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Use the assistant's version" })).toBeNull();
});

it("sends what was chosen before applying, and applies the card the core made of it", async () => {
  fakePause();
  const done: string[] = [];
  const engine = vi.fn((r: Revision, p: Proposal) => {
    done.push("revise");
    return marking(r, p);
  });
  const apply = vi.fn(async (_now?: Proposal) => void done.push("apply"));
  render(<Card engine={engine} on={{ apply }} />);
  fireEvent.click(box("Extra Electronics · Oct 5"));
  // Apply doesn't wait for the pause: the choice goes first, then the change is applied.
  fireEvent.click(button("Apply 2 of 3"));
  await pause(0);
  expect(done).toEqual(["revise", "apply"]);
  expect(engine).toHaveBeenCalledTimes(1);
  expect(apply.mock.calls[0][0]?.choices).toEqual({ skip: ["t:b"], set: [] });
  // Nothing is sent a second time once the pause has passed.
  await pause();
  expect(engine).toHaveBeenCalledTimes(1);
});

it("doesn't apply a card whose choices the core didn't take, and shows it as the core has it", async () => {
  fakePause();
  const on = handlers({ revise: vi.fn(async () => Promise.reject(new Error("This proposal was already handled."))) });
  show(draft(), on);
  fireEvent.click(box("Careem · Oct 3"));
  fireEvent.click(button("Apply 2 of 3"));
  await pause(0);
  expect(on.revise).toHaveBeenCalledTimes(1);
  expect(on.apply).not.toHaveBeenCalled();
  expect(box("Careem · Oct 3").checked).toBe(true);
  expect(button("Apply").disabled).toBe(false);
});

it("applies a card with nothing chosen on it without sending anything first", async () => {
  const on = show(draft());
  fireEvent.click(button("Apply"));
  await turn();
  expect(on.revise).not.toHaveBeenCalled();
  expect(on.apply).toHaveBeenCalledWith(undefined);
});

it("keeps a tick made a moment before the panel closes", async () => {
  fakePause();
  const on = show(draft());
  fireEvent.click(box("Careem · Oct 3"));
  cleanup();
  await pause(0);
  expect(on.revise).toHaveBeenCalledWith({ skip: ["t:a"], set: [] });
});

it("gives a part of a change another category, shown at once", async () => {
  fakePause();
  const engine = vi.fn(marking);
  render(<Card engine={engine} />);
  // Only what can be changed has a pencil.
  expect(screen.queryByRole("button", { name: "Change New rule: payees containing “careem”" })).toBeNull();
  fireEvent.click(button("Change Extra Electronics · Oct 5"));
  const editor = screen.getByRole("group", { name: "Change Extra Electronics · Oct 5" });
  const category = within(editor).getByRole("combobox", { name: "Category for Extra Electronics · Oct 5" }) as HTMLInputElement;
  expect(category.value).toBe("Gifts");
  fireEvent.change(category, { target: { value: "Groc" } });
  fireEvent.click(screen.getByRole("option", { name: /Groceries/ }));
  fireEvent.click(within(editor).getByRole("button", { name: "Save" }));
  // The editor closes and the row names the category it gives now, before the core has answered.
  expect(screen.queryByRole("group", { name: "Change Extra Electronics · Oct 5" })).toBeNull();
  expect(within(box("Extra Electronics · Oct 5").closest(".proposal-row") as HTMLElement).getByText("Groceries")).toBeTruthy();
  expect(engine).not.toHaveBeenCalled();
  await pause();
  expect(engine).toHaveBeenLastCalledWith({ skip: [], set: [{ key: "t:b", category: "groceries" }] }, expect.anything());
  // Ready to Assign is a category to give too.
  fireEvent.click(button("Change Careem · Oct 3"));
  const ready = within(screen.getByRole("group", { name: "Change Careem · Oct 3" })).getByRole("combobox");
  fireEvent.change(ready, { target: { value: "Ready" } });
  fireEvent.click(screen.getByRole("option", { name: /Ready to assign/ }));
  fireEvent.click(button("Save"));
  await pause();
  expect(engine).toHaveBeenLastCalledWith(
    {
      skip: [],
      set: [
        { key: "t:b", category: "groceries" },
        { key: "t:a", category: null },
      ],
    },
    expect.anything(),
  );
});

it("opens a changed part at the category it gives now, so the assistant's can be chosen again", async () => {
  fakePause();
  const engine = vi.fn(marking);
  render(<Card engine={engine} />);
  const choose = (typed: string, name: RegExp) => {
    fireEvent.click(button("Change Extra Electronics · Oct 5"));
    const editor = screen.getByRole("group", { name: "Change Extra Electronics · Oct 5" });
    const category = within(editor).getByRole("combobox") as HTMLInputElement;
    const was = category.value;
    fireEvent.change(category, { target: { value: typed } });
    fireEvent.click(screen.getByRole("option", { name }));
    fireEvent.click(within(editor).getByRole("button", { name: "Save" }));
    expect(screen.queryByRole("group", { name: "Change Extra Electronics · Oct 5" })).toBeNull();
    return was;
  };
  expect(choose("Groc", /Groceries/)).toBe("Gifts");
  // Before the core has answered, and after.
  expect(choose("Gif", /Gifts/)).toBe("Groceries");
  await pause();
  expect(engine).toHaveBeenCalledTimes(1);
  expect(engine).toHaveBeenLastCalledWith({ skip: [], set: [{ key: "t:b", category: "gifts" }] }, expect.anything());
  expect(choose("Groc", /Groceries/)).toBe("Gifts");
});

const later = () => {
  let done!: () => void;
  const promise = new Promise<void>((resolve) => (done = resolve));
  return { promise, done };
};

it("is busy while it's applied, and takes choices made while others are on their way", async () => {
  fakePause();
  const engine = vi.fn(marking);
  const replies: (() => void)[] = [];
  const applying = later();
  function Slow() {
    const [p, setP] = useState(draft());
    return (
      <ProposalCard
        proposal={p}
        budget={demoBudget()}
        canUndo
        on={handlers({
          apply: () => applying.promise,
          revise: (r) =>
            new Promise<Proposal>((made) =>
              replies.push(() => {
                const next = engine(r, p);
                setP(next);
                made(next);
              }),
            ),
        })}
      />
    );
  }
  render(<Slow />);
  // An editor left open while the card is applied can't save onto it, and nothing can be chosen.
  fireEvent.click(button("Change Extra Electronics · Oct 5"));
  fireEvent.click(button("Apply"));
  expect(button("Save").disabled).toBe(true);
  expect(box("Careem · Oct 3").disabled).toBe(true);
  applying.done();
  await turn();
  expect(button("Save").disabled).toBe(false);
  // Leaving a part out closes it. While that is on its way the card isn't busy: more can be chosen.
  fireEvent.click(box("Careem · Oct 3"));
  expect(screen.queryByRole("group", { name: "Change Extra Electronics · Oct 5" })).toBeNull();
  await pause();
  expect(replies).toHaveLength(1);
  expect(box("Extra Electronics · Oct 5").disabled).toBe(false);
  fireEvent.click(box("Extra Electronics · Oct 5"));
  expect(button("Apply 1 of 3").disabled).toBe(false);
  // The first answer doesn't put back what was chosen since; that goes in the next call, whole.
  replies[0]();
  await turn();
  expect(box("Extra Electronics · Oct 5").checked).toBe(false);
  await pause();
  expect(replies).toHaveLength(2);
  replies[1]();
  await turn();
  expect(engine).toHaveBeenLastCalledWith({ skip: ["t:a", "t:b"], set: [] }, expect.anything());
  expect(button("Apply 1 of 3").disabled).toBe(false);
});

it("gives a part another amount, keeping its sign, and Esc leaves it as it was", async () => {
  fakePause();
  const amounts: CardRow[] = [
    { label: "Home", amount: -45900, key: "s:0", edit: { category: true, categoryId: "rent", amount: true } },
    { label: "Groceries", amount: -2400, key: "s:1", edit: { category: true, categoryId: "groceries", amount: true } },
  ];
  const start = { ...draft(), card: { ...draft().card, rows: amounts } };
  const on = show(start, handlers({ revise: vi.fn(async () => start) }));
  fireEvent.click(button("Change Home"));
  const amount = screen.getByLabelText("Amount for Home") as HTMLInputElement;
  expect(amount.value).toBe("-459.00");
  fireEvent.change(amount, { target: { value: "300" } });
  fireEvent.keyDown(amount, { key: "Enter" });
  // At once on the row, marked as the user's.
  expect(screen.getByText(money(-30000)).title).toBe("Changed by you");
  await pause();
  expect(on.revise).toHaveBeenLastCalledWith({ skip: [], set: [{ key: "s:0", amount: -30000 }] });
  // Esc: nothing changes.
  fireEvent.click(button("Change Groceries"));
  fireEvent.change(screen.getByLabelText("Amount for Groceries"), { target: { value: "+5" } });
  fireEvent.keyDown(screen.getByLabelText("Amount for Groceries"), { key: "Escape" });
  expect(screen.queryByLabelText("Amount for Groceries")).toBeNull();
  await pause();
  expect(on.revise).toHaveBeenCalledTimes(1);
  // An amount that doesn't read says so.
  fireEvent.click(button("Change Groceries"));
  fireEvent.change(screen.getByLabelText("Amount for Groceries"), { target: { value: "lots" } });
  fireEvent.keyDown(screen.getByLabelText("Amount for Groceries"), { key: "Enter" });
  expect(screen.getByText(/Enter an amount like/)).toBeTruthy();
  await pause();
  expect(on.revise).toHaveBeenCalledTimes(1);
});

it("gives the keyboard back to a row's pencil when its editor closes", () => {
  fakePause();
  render(<Card engine={vi.fn(marking)} />);
  const pencil = () => button("Change Extra Electronics · Oct 5");
  fireEvent.click(pencil());
  const editor = screen.getByRole("group", { name: "Change Extra Electronics · Oct 5" });
  expect(editor.contains(document.activeElement)).toBe(true);
  fireEvent.click(within(editor).getByRole("button", { name: "Cancel" }));
  expect(document.activeElement).toBe(pencil());
  // After a save too: the pencil is there to use again at once.
  fireEvent.click(pencil());
  const again = screen.getByRole("group", { name: "Change Extra Electronics · Oct 5" });
  fireEvent.change(within(again).getByRole("combobox"), { target: { value: "Groc" } });
  fireEvent.click(screen.getByRole("option", { name: /Groceries/ }));
  fireEvent.click(within(again).getByRole("button", { name: "Save" }));
  expect(document.activeElement).toBe(pencil());
  // Leaving a part out closes an open editor without moving the keyboard to its pencil.
  fireEvent.click(pencil());
  const tick = box("Careem · Oct 3");
  tick.focus();
  fireEvent.click(tick);
  expect(screen.queryByRole("group", { name: "Change Extra Electronics · Oct 5" })).toBeNull();
  expect(document.activeElement).not.toBe(pencil());
});

it("lists every part of a long change, a few until asked for all", () => {
  const many: CardRow[] = Array.from({ length: 30 }, (_, i) => ({ label: `Shop ${i + 1}`, amount: -(i + 1) * 100, key: `t:${i}` }));
  show({ ...draft(), card: { ...draft().card, rows: many } });
  expect(screen.queryByText("Shop 30")).toBeNull();
  fireEvent.click(button("Show all 30"));
  expect(screen.getByText("Shop 30")).toBeTruthy();
  expect(screen.getByText(money(-3000))).toBeTruthy();
  fireEvent.click(button("Show fewer"));
  expect(screen.queryByText("Shop 30")).toBeNull();
});

it("selects all of a long change's parts, none, or only the ones to check", async () => {
  fakePause();
  const many: CardRow[] = Array.from({ length: 7 }, (_, i) => ({
    label: `Shop ${i + 1}`,
    amount: -(i + 1) * 100,
    key: `t:${i}`,
    ...(i === 2 || i === 5 ? { tone: "warn" as const, badge: "Check this" } : {}),
  }));
  const start = { ...draft(), card: { ...draft().card, rows: many } };
  const engine = vi.fn((r: Revision, p: Proposal): Proposal => ({
    ...p,
    choices: r.skip.length ? r : undefined,
    card: { ...p.card, rows: many.map((row) => ({ ...row, skipped: r.skip.includes(row.key!) })) },
  }));
  render(<Card engine={engine} start={start} />);
  const ticked = () => (screen.getAllByRole("checkbox") as HTMLInputElement[]).filter((c) => c.checked).length;
  expect(button("Select all").disabled).toBe(true);
  fireEvent.click(button("Select none"));
  expect(ticked()).toBe(0);
  expect(button("Apply 0 of 7").disabled).toBe(true);
  fireEvent.click(button("Only the ones to check"));
  expect(ticked()).toBe(2);
  expect(box("Shop 3").checked && box("Shop 6").checked).toBe(true);
  expect(button("Apply 2 of 7")).toBeTruthy();
  // One call for all of it, with what was chosen last.
  await pause();
  expect(engine).toHaveBeenCalledTimes(1);
  expect(engine).toHaveBeenLastCalledWith({ skip: ["t:0", "t:1", "t:3", "t:4", "t:6"], set: [] }, expect.anything());
  expect(ticked()).toBe(2);
  fireEvent.click(button("Select all"));
  expect(ticked()).toBe(7);
  expect(button("Apply")).toBeTruthy();
  await pause();
  expect(engine).toHaveBeenLastCalledWith({ skip: [], set: [] }, expect.anything());
  cleanup();
  // A short one has no need of them, and neither has one without a row to check.
  show(draft());
  expect(screen.queryByRole("button", { name: "Select all" })).toBeNull();
  cleanup();
  show({ ...draft(), card: { ...draft().card, rows: many.map(({ tone: _tone, ...row }) => row) } });
  expect(button("Select none")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Only the ones to check" })).toBeNull();
});

it("draws a button for each action the core offers and the app knows, the first as the main one", () => {
  const on = show(draft({ actions: ["apply", "teleport", "askChanges", "discard", "show"] }));
  const card = screen.getByLabelText("Proposal: Categorize 2 transactions");
  expect(within(card).getAllByRole("button").filter((b) => !b.classList.contains("proposal-edit")).map((b) => b.textContent)).toEqual([
    "Apply",
    "Ask for changes",
    "Discard",
    "Show in the budget",
  ]);
  expect(button("Apply").className).toContain("primary");
  expect(button("Ask for changes").className).toContain("secondary");
  // Setting it aside is kept apart from the main button.
  expect(button("Discard").className).toContain("proposal-discard");
  fireEvent.click(button("Ask for changes"));
  expect(on.askChanges).toHaveBeenCalledTimes(1);
  fireEvent.click(button("Show in the budget"));
  expect(on.show).toHaveBeenCalledTimes(1);
  fireEvent.click(button("Discard"));
  expect(on.discard).toHaveBeenCalledTimes(1);
  cleanup();
  // What it offers is the core's to say, never worked out from its kind and status here.
  show(draft({ actions: [] }));
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  expect(screen.queryByRole("checkbox")).toBeNull();
  cleanup();
  // A card that can't be applied any more offers a new version of it.
  const failed = show(draft({ status: "failed", actions: ["askAgain", "discard"], message: { tone: "error", text: "Transport no longer exists." } }));
  expect(button("Ask for a new version").className).toContain("primary");
  expect(screen.queryByRole("checkbox")).toBeNull();
  fireEvent.click(button("Ask for a new version"));
  expect(failed.askAgain).toHaveBeenCalledTimes(1);
});

it("draws a kind it doesn't know as a plain card, with the actions it knows", () => {
  const on = show(draft({ kind: "forecast", actions: ["apply", "simulate", "discard"] }));
  expect(screen.getByText("Update UrBudget to use this card.")).toBeTruthy();
  expect(screen.getByText("Proposal · Transactions")).toBeTruthy();
  // Its rows are there to read, not to choose among or change.
  expect(screen.getByText("Careem · Oct 3")).toBeTruthy();
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(screen.queryByRole("button", { name: /^Change / })).toBeNull();
  fireEvent.click(button("Apply"));
  expect(on.apply).toHaveBeenCalledTimes(1);
  expect(button("Discard")).toBeTruthy();
  cleanup();
  show(draft());
  expect(screen.queryByText("Update UrBudget to use this card.")).toBeNull();
});

it("says what the card has to say in one strip, by its tone, apart from its own warning", () => {
  const withWarning = { ...draft().card, warning: "One of these is a transfer." };
  show(draft({ card: withWarning, message: { tone: "info", text: "Your budget changed." } }));
  const strip = () => screen.getByText(/Your budget changed|Check the amounts|no longer exists/).closest(".proposal-message") as HTMLElement;
  expect(strip().className).toContain("info");
  // Both show: neither stands in for the other.
  expect(screen.getByText("One of these is a transfer.").closest(".proposal-warning")).toBeTruthy();
  cleanup();
  show(draft({ card: withWarning, message: { tone: "warn", text: "Check the amounts." } }));
  expect(strip().className).toContain("warn");
  expect(screen.getByText("One of these is a transfer.")).toBeTruthy();
  cleanup();
  show(draft({ status: "failed", actions: ["askAgain", "discard"], message: { tone: "error", text: "Transport no longer exists." } }));
  expect(strip().className).toContain("error");
  cleanup();
  show(draft());
  expect(document.querySelector(".proposal-message")).toBeNull();
  expect(document.querySelector(".proposal-warning")).toBeNull();
});

it("folds a finished card to one line that opens to its rows, as they were", async () => {
  const applied = draft({
    status: "applied",
    appliedAt: "2026-10-21T10:00:00Z",
    actions: ["undo", "show"],
    card: { ...draft().card, rows: rows.map((r) => (r.key === "t:b" ? { ...r, skipped: true } : r)) },
  });
  const on = show(applied);
  // What, and under it when, in the app's own date and time and no longer than it needs to be.
  const when = appliedTime("2026-10-21T10:00:00Z");
  // (Text is matched with its spaces made plain ones: a time may be written with a narrow one.)
  expect(screen.getByText(`Applied ${when}`.replace(/\s+/g, " "))).toBeTruthy();
  // The state and the card's buttons sit under the title, not beside it.
  expect(document.querySelector(".proposal-fold .proposal-state")).toBeNull();
  expect(document.querySelector(".proposal-done-meta .proposal-state")).toBeTruthy();
  expect(document.querySelectorAll(".proposal-done-meta button").length).toBe(2);
  expect(screen.queryByText("Careem · Oct 3")).toBeNull();
  const line = button(/Categorize 2 transactions/);
  expect(line.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(line);
  expect(line.getAttribute("aria-expanded")).toBe("true");
  // Read-only: nothing to tick or change, and what was left out is shown as left out.
  expect(screen.getByText("Careem · Oct 3")).toBeTruthy();
  expect(screen.getByText("Extra Electronics · Oct 5").closest(".proposal-row")?.className).toContain("skipped");
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(screen.queryByRole("button", { name: /^Change / })).toBeNull();
  fireEvent.click(line);
  expect(screen.queryByText("Careem · Oct 3")).toBeNull();
  // Its actions sit on the line.
  fireEvent.click(button("Undo"));
  expect(on.undo).toHaveBeenCalledTimes(1);
  // (The card is busy until the undo is done.)
  expect(button("Show in the budget").disabled).toBe(true);
  await turn();
  fireEvent.click(button("Show in the budget"));
  expect(on.show).toHaveBeenCalledTimes(1);
  cleanup();
  // Undo is there only while the app can still take the change back.
  show(draft({ status: "applied", actions: ["undo"] }), handlers(), { canUndo: false });
  expect(screen.getByText("Applied")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
});

it("says what became of a card that was set aside, undone, replaced or removed, and puts it back", async () => {
  const on = show(draft({ status: "discarded", actions: ["restore"] }));
  expect(screen.getByText("Discarded")).toBeTruthy();
  fireEvent.click(button("Restore"));
  await turn();
  expect(on.restore).toHaveBeenCalledTimes(1);
  cleanup();
  show(draft({ status: "replaced", replacedBy: "p2", actions: ["restore"] }));
  expect(screen.getByText("Replaced by a newer version")).toBeTruthy();
  expect(button("Restore")).toBeTruthy();
  cleanup();
  // A thing the assistant made is removed, not discarded.
  show(draft({ kind: "report", made: true, status: "discarded", actions: ["restore"], title: "October in review" }));
  expect(screen.getByText("Removed")).toBeTruthy();
  expect(screen.getByText("October in review")).toBeTruthy();
  expect(button("Restore")).toBeTruthy();
  cleanup();
  // Undone, it can be applied again from its line; what changes it or sets it aside is under its rows.
  const undone = show(draft({ status: "undone", actions: ["apply", "askChanges", "discard"] }));
  expect(screen.getByText("Undone")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Discard" })).toBeNull();
  fireEvent.click(button("Apply again"));
  await turn();
  expect(undone.apply).toHaveBeenCalledTimes(1);
  fireEvent.click(button(/Categorize 2 transactions/));
  expect(button("Ask for changes")).toBeTruthy();
  expect(button("Discard")).toBeTruthy();
  expect(screen.queryByRole("checkbox")).toBeNull();
  cleanup();
  // What a finished card has to say shows without opening it.
  show(draft({ kind: "fix", status: "applied", actions: [], message: { tone: "warn", text: "Fixes applied: 2 of 3. Not applied: Line 30 · amount: n/a → -12.40" } }));
  expect(screen.getByText(/Fixes applied: 2 of 3/).closest(".proposal-message")?.className).toContain("warn");
});

it("draws a thing the assistant made with its own buttons, and removes it from its menu", async () => {
  const report = draft({
    kind: "report",
    made: true,
    title: "October in review",
    sheets: 2,
    actions: ["preview", "print", "excel", "remove"],
    card: { kicker: "Report", rows: [{ label: "Spending" }], stats: [], more: 0, highlight: { categories: [], transactions: [] } },
  });
  const on = show(report);
  expect(button("Preview").className).toContain("primary");
  expect(button("Print or save as PDF")).toBeTruthy();
  expect(button("Excel")).toBeTruthy();
  // Nothing to apply or set aside beside them.
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Discard" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
  expect(screen.queryByText("One undo step")).toBeNull();
  fireEvent.click(button("Preview"));
  expect(on.preview).toHaveBeenCalledTimes(1);
  fireEvent.click(button("More actions"));
  fireEvent.click(screen.getByRole("menuitem", { name: "Remove" }));
  await turn();
  expect(on.discard).toHaveBeenCalledTimes(1);
  cleanup();
  // A converted file that was imported keeps its file to save.
  const imported = show(draft({ kind: "import", made: true, status: "applied", actions: ["saveCsv"], title: "Statement for Everyday" }));
  expect(screen.queryByRole("button", { name: "Review import" })).toBeNull();
  fireEvent.click(button("Save as CSV"));
  await turn();
  expect(imported.saveCsv).toHaveBeenCalledTimes(1);
});

it("asks a question with its answers as buttons, and folds to what was asked once it's answered", () => {
  const question = draft({
    kind: "question",
    title: "Which account did you pay from?",
    actions: ["answer"],
    card: { kicker: "Question", rows: [{ label: "Everyday" }, { label: "Card", detail: "Ends in 4410" }], stats: [], more: 0, highlight: { categories: [], transactions: [] } },
  });
  const on = show(question);
  expect(screen.getByText("Which account did you pay from?")).toBeTruthy();
  expect(button(/^Everyday/).className).toContain("proposal-answer");
  // The row's label is what's sent, as the user's message.
  fireEvent.click(button(/^Card/));
  expect(on.answer).toHaveBeenCalledWith("Card");
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  cleanup();
  // While a message can't be sent (a reply is still arriving), an answer can't be pressed.
  show(question, handlers(), { answerable: false });
  expect(button(/^Everyday/).disabled).toBe(true);
  cleanup();
  show({ ...question, status: "applied", actions: [] });
  // "Asked:" and the question, each its own piece of the line, so each is cut at its own end.
  expect(screen.getByText("Asked:").className).toBe("proposal-asked");
  expect(screen.getByText("Which account did you pay from?")).toBeTruthy();
  expect(screen.queryByText("Everyday")).toBeNull();
  fireEvent.click(button(/Asked:\s*Which account/));
  // Its answers are there to read, no longer to press.
  expect(screen.getByText("Everyday").closest("button")).toBeNull();
});

it("draws figures with a bar for each row's share of the largest, and the total without one", () => {
  const figures = draft({
    kind: "figures",
    made: true,
    title: "Spending in October",
    actions: ["remove", "show"],
    card: {
      kicker: "Figures · October 2026",
      rows: [
        { label: "Dining out", amount: -124000 },
        { label: "Coffee", detail: "1 visit", amount: -31000 },
        { label: "Refunds", amount: 62000 },
      ],
      total: { label: "Total", amount: -93000 },
      stats: [],
      more: 0,
      highlight: { month: "2026-10", categories: ["dining", "coffee"], transactions: [] },
    },
  });
  const on = show(figures);
  const bars = [...document.querySelectorAll<HTMLElement>(".proposal-bar i")].map((bar) => bar.style.width);
  // By absolute value, so money in is measured as money out is.
  expect(bars).toEqual(["100%", "25%", "50%"]);
  expect(screen.getByText("1 visit")).toBeTruthy();
  expect(screen.getByText(money(-31000))).toBeTruthy();
  expect(document.querySelector(".proposal-total .proposal-bar")).toBeNull();
  expect(screen.queryByRole("checkbox")).toBeNull();
  fireEvent.click(button("Show in the budget"));
  expect(on.show).toHaveBeenCalledTimes(1);
  fireEvent.click(button("More actions"));
  expect(screen.getByRole("menuitem", { name: "Remove" })).toBeTruthy();
});

it("draws a deletion with the destructive accent and a button that says how many it deletes", async () => {
  fakePause();
  const gone: CardRow[] = ["a", "b", "c"].map((id, i) => ({ label: `Shop ${i + 1} · Oct ${i + 1}`, detail: "Everyday · Dining out", amount: -(i + 1) * 100, key: `t:${id}` }));
  const start = draft({
    kind: "delete",
    title: "Delete 3 duplicates",
    actions: ["apply", "askChanges", "discard", "show"],
    card: { ...draft().card, kicker: "Proposal · Delete", rows: gone, warning: "These leave the budget." },
  });
  const on = show(start, handlers({ revise: vi.fn(async () => start) }));
  expect(screen.getByLabelText("Proposal: Delete 3 duplicates").className).toContain("destructive");
  expect(button("Delete 3 transactions").className).toContain("danger");
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  // It counts what's left to delete, in words that fit the count.
  fireEvent.click(box("Shop 1 · Oct 1"));
  expect(button("Delete 2 transactions")).toBeTruthy();
  fireEvent.click(box("Shop 2 · Oct 2"));
  fireEvent.click(button("Delete 1 transaction"));
  await pause(0);
  expect(on.revise).toHaveBeenCalledWith({ skip: ["t:a", "t:b"], set: [] });
  expect(on.apply).toHaveBeenCalledTimes(1);
});

it("offers an import's fixes only while their import is open", async () => {
  const fix = draft({
    kind: "fix",
    title: "1 fix for the import",
    actions: ["applyToImport", "askChanges", "discard"],
    card: { kicker: "Import fixes", rows: [{ label: "Line 30 · amount: n/a → -12.40", detail: "The statement's total" }], stats: [], more: 0, highlight: { categories: [], transactions: [] } },
  });
  const on = show(fix);
  expect(screen.getByText("The statement's total")).toBeTruthy();
  expect(screen.getByText("Each applies only to a line that hasn't changed")).toBeTruthy();
  fireEvent.click(button("Apply to the import"));
  await turn();
  expect(on.apply).toHaveBeenCalledTimes(1);
  cleanup();
  // With its import closed, the card says what to do instead of offering a button that fails.
  const closed = show(fix, handlers(), { fixable: false });
  expect(button("Apply to the import").disabled).toBe(true);
  expect(screen.getByText("Open the import these fixes are for.")).toBeTruthy();
  expect(screen.queryByText("Each applies only to a line that hasn't changed")).toBeNull();
  // It can still be asked about, or set aside.
  expect(button("Ask for changes").disabled).toBe(false);
  expect(closed.apply).not.toHaveBeenCalled();
});
