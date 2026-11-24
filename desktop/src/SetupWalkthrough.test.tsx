// The setup walkthrough: nothing saved until it's skipped or assigns the money, what skipping
// at each step makes, the draft that comes back, the same steps on an open budget, and the
// setup card (docs/plans/first-budget-walkthrough.md, "Tests › Desktop").
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { produce } from "immer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("./bridge", async () => {
  const { templatesFixture } = await import("./budgetTemplates.fixture");
  return {
    api: {
      budgetTemplates: vi.fn(async () => templatesFixture()),
      setupBudget: vi.fn(),
      setupPreview: vi.fn(),
      createBudget: vi.fn(),
      autoAssignPreview: vi.fn(),
      action: vi.fn(),
      listBudgets: vi.fn(async () => []),
    },
  };
});
vi.mock("./cloud", () => ({ cloud: { open: vi.fn(async () => {}) } }));
import { api } from "./bridge";
import { templatesFixture } from "./budgetTemplates.fixture";
import { SetupCard, SetupWalkthrough, type SetupSession } from "./SetupWalkthrough";
import { FirstRunWelcome } from "./Welcome";
import { newDraft, readDraft, readMark, writeDraft, writeMark, type SetupAnswers, type SetupDraft, type SetupPreview } from "./setup";
import { ToastProvider } from "./ui";
import type { Budget, Summary } from "./types";

const MONTH = "2026-11";
const USD = { currency: "USD", numberFormat: { decimal: ".", group: ",", symbol: "before" }, dateFormat: "mdy" } as const;

/** What the engine's setup::build makes of the answers, near enough for the screens (setup.rs has the real one and its tests). */
function build(answers: SetupAnswers): Budget {
  const target = (key: string) => {
    const found = answers.amounts?.find((a) => a.category === key);
    return found ? (found.period === "year" ? Math.ceil(found.amount / 12) : found.amount) : 0;
  };
  const categories: Budget["categories"] = [];
  templatesFixture()
    .find((x) => x.id === answers.template)
    ?.groups.forEach((group, g) =>
      group.categories.forEach((name, c) => {
        const key = `${g}.${c}`;
        if (!answers.leftOut?.includes(key)) categories.push({ id: `c${key}`, name, group: group.name, target: target(key) });
      }),
    );
  answers.added?.forEach((name, n) => {
    if (name.trim()) categories.push({ id: `c+${n}`, name, group: "Other", target: target(`+${n}`) });
  });
  const accounts: Budget["accounts"] = answers.accounts.map((a, i) => ({
    id: `a${i}`,
    name: a.name,
    kind: a.type === "creditCard" ? "credit" : "cash",
    accountType: a.type,
    openingBalance: a.type === "creditCard" ? -a.balance : a.balance,
    archived: false,
  }));
  for (const a of accounts) if (a.kind === "credit") categories.push({ id: `pay-${a.id}`, name: a.name, group: "Credit Card Payments", target: 0, paymentAccountId: a.id });
  return {
    schemaVersion: 2,
    name: answers.name,
    currency: answers.currency,
    numberFormat: answers.numberFormat,
    dateFormat: answers.dateFormat,
    accounts,
    categories,
    transactions: [],
    assignments: [],
    ...(answers.expectedIncome ? { expectedIncome: answers.expectedIncome } : {}),
  };
}
const assigned = (b: Budget) => b.assignments.reduce((sum, a) => sum + a.amount, 0);
const cash = (b: Budget) => b.accounts.filter((a) => a.kind === "cash").reduce((sum, a) => sum + a.openingBalance, 0);
/** Underfunded, plainly: each target and each card's debt in order, until the money runs out. */
function underfunded(b: Budget, limit = true): SetupPreview["changes"] {
  let left = limit ? cash(b) - assigned(b) : Infinity;
  return b.categories.flatMap((c) => {
    const before = b.assignments.find((a) => a.categoryId === c.id)?.amount ?? 0;
    const wants = c.paymentAccountId ? -(b.accounts.find((a) => a.id === c.paymentAccountId)?.openingBalance ?? 0) : c.target;
    const give = Math.max(0, Math.min(wants - before, left));
    left -= give;
    return give ? [{ id: c.id, before, after: before + give }] : [];
  });
}
const summaryOf = (b: Budget) =>
  ({ readyToAssign: cash(b) - assigned(b), accounts: b.accounts.map((a) => ({ id: a.id, balance: a.openingBalance, cleared: 0, uncleared: 0 })), categories: [] }) as unknown as Summary;

/** The library and the open budget, as App keeps them around the walkthrough. */
const world = { saved: new Map<string, Budget>(), open: null as Budget | null, undoSteps: 0, closed: 0 };

function Harness({ start, budget: initial = null }: { start: SetupSession; budget?: Budget | null }) {
  const [session, setSession] = useState<SetupSession | null>(start);
  const [budget, setBudget] = useState<Budget | null>(initial);
  world.open = budget;
  const change = (next: Budget) => {
    world.open = next;
    world.undoSteps++;
    setBudget(next);
  };
  if (!session) return <p>Walkthrough closed</p>;
  return (
    <ToastProvider>
      <SetupWalkthrough
        session={session}
        onSession={setSession}
        budget={budget}
        budgetId={budget ? "made" : ""}
        summary={budget ? summaryOf(budget) : null}
        month={MONTH}
        edit={(update) => change(produce(world.open!, update))}
        transform={async (work) => change(await work(world.open!))}
        openBudget={async (id) => {
          world.open = world.saved.get(id)!;
          setBudget(world.open);
        }}
        close={() => {
          world.closed++;
          setSession(null);
        }}
      />
    </ToastProvider>
  );
}

beforeEach(() => {
  localStorage.clear();
  Object.assign(world, { saved: new Map(), open: null, undoSteps: 0, closed: 0 });
  vi.mocked(api.setupBudget).mockImplementation(async (answers) => build(answers));
  vi.mocked(api.setupPreview).mockImplementation(async (b) => ({
    readyToAssign: cash(b),
    needed: underfunded(b, false).reduce((sum, c) => sum + c.after - c.before, 0),
    changes: underfunded(b),
    wanted: underfunded(b, false),
  }));
  vi.mocked(api.createBudget).mockImplementation(async (b) => {
    world.saved.set("made", b);
    return "made";
  });
  vi.mocked(api.autoAssignPreview).mockImplementation(async (b, _month, _ids, limit) => ({ underfunded: underfunded(b, limit) }));
  vi.mocked(api.action).mockImplementation(async (b, action) => {
    const set = (budget: Budget, id: string, amount: number) => ({
      ...budget,
      assignments: [...budget.assignments.filter((a) => a.categoryId !== id), { month: MONTH, categoryId: id, amount }],
    });
    if (action.type === "autoAssign") return underfunded(b).reduce((next, c) => set(next, c.id, c.after), b);
    if (action.type === "assignAmount") return set(b, action.categoryId as string, action.amount as number);
    throw new Error(`unexpected action ${String(action.type)}`);
  });
});
afterEach(cleanup);

const type = (label: string | RegExp, value: string, within_: ReturnType<typeof within> | typeof screen = screen) =>
  fireEvent.change(within_.getByLabelText(label), { target: { value } });
const press = (name: string | RegExp) => fireEvent.click(screen.getByRole("button", { name }));
const heading = (name: string) => screen.findByRole("heading", { level: 1, name });
/** A draft with the first three steps answered, left on `step`. */
const answered = (step: SetupDraft["step"]): SetupDraft => ({
  ...newDraft({ name: "My budget", format: { ...USD } }),
  step,
  accounts: [
    { name: "Checking", type: "checking", balance: "3,700" },
    { name: "Visa", type: "creditCard", balance: "420" },
    { name: "", type: "savings", balance: "" },
  ],
  leftOut: ["0.2"],
  added: ["Pets"],
  amounts: { "0.0": { amount: "1500" }, "1.3": { amount: "900", yearly: true }, "+0": { amount: "40" } },
  income: "5000",
});

it("makes nothing until Assign it, then funds the budget in one step and asks how it'll be kept up", async () => {
  function App() {
    const [started, setStarted] = useState(false);
    return started ? <Harness start={{ mode: "draft" }} /> : <ToastProvider><FirstRunWelcome openBudget={vi.fn()} onSetup={() => setStarted(true)} /></ToastProvider>;
  }
  render(<App />);
  press("Create my budget");
  await heading("Where is your money?");
  expect(screen.getByText("Step 1 of 5")).toBeTruthy();
  expect(document.querySelectorAll(".setup-progress > span")).toHaveLength(5);
  expect(document.querySelectorAll(".setup-progress > .done")).toHaveLength(1);
  // The step opens with the caret in its first field, and Skip setup beside its one filled button.
  expect(document.activeElement).toBe(screen.getByLabelText("Name"));
  expect(document.querySelectorAll("button.primary")).toHaveLength(1);
  // Next needs one account with a name.
  press("Next");
  expect(screen.getByRole("alert").textContent).toBe("Add one account with a name to go on.");
  type("Name", "Checking");
  type("Today's balance", "3700");
  // Left, the amount reads as the budget writes amounts.
  fireEvent.blur(screen.getByLabelText("Today's balance"));
  expect((screen.getByLabelText("Today's balance") as HTMLInputElement).value).toBe("3,700.00");
  // The columns are named once, above the rows.
  expect(document.querySelectorAll(".setup-row.head")).toHaveLength(1);
  press(/Add another/);
  const card = within(screen.getByRole("group", { name: "Account 2" }));
  expect(document.activeElement).toBe(card.getByLabelText("Name"));
  expect(document.querySelectorAll(".setup-row.head")).toHaveLength(1);
  type("Name", "Visa", card);
  fireEvent.change(card.getByLabelText("Type"), { target: { value: "creditCard" } });
  type("What you owe", "420", card);
  // A card is owed, not had, and it's said from the start, over the buttons as step 3's total is.
  expect(document.querySelector(".setup-actions .setup-note")!.textContent).toBe("You have $3,700.00 to plan with, and owe $420.00 on cards.");
  press("Next");

  await heading("What do you spend on?");
  const water = await screen.findByRole("button", { name: "Water" });
  expect(document.querySelectorAll(".setup-progress > .done")).toHaveLength(2);
  // Only the set is chosen here; its chips say what it holds, each with a check while it's in.
  expect(screen.queryByText("Bills, needs, wants and savings. A good fit for most people.")).toBeNull();
  expect(water.getAttribute("aria-pressed")).toBe("true");
  expect(water.querySelector("svg")).not.toBeNull();
  fireEvent.click(water);
  expect(water.getAttribute("aria-pressed")).toBe("false");
  expect(water.querySelector("svg")).toBeNull();
  press(/Add your own/);
  type("Category name", "Pets");
  fireEvent.keyDown(screen.getByLabelText("Category name"), { key: "Enter" });
  expect(screen.getByRole("button", { name: "Remove Pets" })).toBeTruthy();
  press("Next");

  await heading("What does a month cost?");
  expect(screen.queryByLabelText("Water")).toBeNull();
  type("Rent or mortgage", "1500");
  type("Insurance", "900");
  // A month or a year is quiet text after the amount: a switch, not a select.
  expect(screen.queryAllByRole("combobox")).toHaveLength(0);
  const yearly = screen.getByRole("switch", { name: "A year's amount, for Insurance" });
  expect([yearly.textContent, yearly.getAttribute("aria-checked")]).toEqual(["a month", "false"]);
  fireEvent.click(yearly);
  expect([yearly.textContent, yearly.getAttribute("aria-checked")]).toEqual(["a year", "true"]);
  expect(screen.getByText("$75.00 a month")).toBeTruthy();
  type("Pets", "40");
  // The total is beside the buttons, card and all, and follows the typing.
  const total = document.querySelector(".setup-actions .setup-note")!;
  expect(total.textContent).toBe("Your month costs $1,615.00, plus $420.00 owed on Visa. You have $3,700.00.");
  expect(total.className).toBe("setup-note");
  type("Groceries", "2500");
  expect(total.textContent).toBe("Your month costs $4,115.00, plus $420.00 owed on Visa. You have $3,700.00.");
  expect(total.className).toBe("setup-note warning");
  type("Groceries", "");
  // Enter goes down the amounts; from the last one it presses Next.
  const rent = screen.getByLabelText("Rent or mortgage");
  rent.focus();
  fireEvent.keyDown(rent, { key: "Enter" });
  expect(document.activeElement).toBe(screen.getByLabelText("Electricity"));
  // Tab goes down the amounts too, then down the switches.
  fireEvent.keyDown(screen.getByLabelText("Electricity"), { key: "Tab" });
  expect(document.activeElement).toBe(screen.getByLabelText("Internet"));
  fireEvent.keyDown(screen.getByLabelText("Pets"), { key: "Tab" });
  expect(document.activeElement).toBe(screen.getByRole("switch", { name: "A year's amount, for Rent or mortgage" }));
  type("Rent or mortgage", "15oo");
  press("Next");
  expect(screen.getByRole("alert").textContent).toMatch(/Enter an amount/);
  type("Rent or mortgage", "1500");
  press("Next");

  await heading("Give your money its jobs");
  // The budget is built to be shown, and still isn't in the library.
  await screen.findByText("Bills");
  expect(api.setupBudget).toHaveBeenCalledOnce();
  expect(api.createBudget).not.toHaveBeenCalled();
  expect(vi.mocked(api.setupBudget).mock.calls[0][0]).toMatchObject({
    name: "My budget",
    accounts: [
      { name: "Checking", type: "checking", balance: 370000 },
      { name: "Visa", type: "creditCard", balance: 42000 },
    ],
    template: "everyday",
    leftOut: ["0.2"],
    added: ["Pets"],
    amounts: [
      { category: "0.0", amount: 150000, period: "month" },
      { category: "1.3", amount: 90000, period: "year" },
      { category: "+0", amount: 4000, period: "month" },
    ],
  });
  // 3,700 less 1,500, 75, 40 and the card's 420.
  // What it comes to is read first, above the table, in the success tone, with the leftover's choice in the same box.
  const outcome = screen.getByRole("status");
  expect(outcome.textContent).toBe("$1,665.00 is left over. Put it in Emergency fund, or keep it for next month. $420.00 is set aside to pay your card.");
  expect(outcome.closest(".setup-result")!.className).toBe("setup-result good");
  expect(outcome.compareDocumentPosition(document.querySelector(".setup-plan")!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  press("Put it in Emergency fund");
  expect(outcome.textContent).toBe("All of it has a job. $420.00 is set aside to pay your card.");

  press("Assign it");
  await heading("How will you keep it up to date?");
  expect(api.createBudget).toHaveBeenCalledOnce();
  // Only the budget is passed: it joins account sync like any other (createBudget's own default).
  expect(vi.mocked(api.createBudget).mock.calls[0]).toHaveLength(1);
  // The budget was saved with nothing assigned; the money went in as one change Undo takes back.
  expect(world.saved.get("made")!.assignments).toEqual([]);
  expect(world.undoSteps).toBe(1);
  expect(vi.mocked(api.action).mock.calls[0][1]).toMatchObject({ type: "autoAssign", mode: "underfunded", month: MONTH, limitToAvailable: true });
  expect(assigned(world.open!)).toBe(370000);
  expect(world.open!.assignments.find((a) => a.categoryId === "c3.0")?.amount).toBe(166500);
  expect(readDraft()).toBeNull();
  expect(readMark("made")).toEqual({ card: true, assigned: true });

  // The budget is made: the last step has Finish and nothing else to press.
  expect(screen.queryByRole("button", { name: "Skip setup" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
  expect(document.querySelectorAll(".setup-progress > .done")).toHaveLength(5);
  expect(screen.getByText(/Start with tomorrow's transactions\./)).toBeTruthy();
  // It only records the choice, for this budget on this device, and that Budget has yet to say it's ready.
  fireEvent.click(screen.getByRole("radio", { name: /Import my bank's file/ }));
  press("Finish");
  expect(screen.getByText("Walkthrough closed")).toBeTruthy();
  expect(readMark("made")).toEqual({ card: true, assigned: true, keep: "file", ready: true });
  expect(world.undoSteps).toBe(1);
});

it("tells a month that costs more than there is, and one that fits exactly", async () => {
  writeDraft({ ...answered(3), accounts: [{ name: "Checking", type: "checking", balance: "1000" }], added: [], amounts: { "0.0": { amount: "1500" } } });
  const { unmount } = render(<Harness start={{ mode: "draft" }} />);
  press("Next");
  await screen.findByText("Left after this");
  expect(screen.getByRole("status").textContent).toBe(
    "Your month costs $500.00 more than you have right now. The rest waits for payday; assign it when it arrives.",
  );
  expect(screen.queryByRole("button", { name: /Put it in/ })).toBeNull();
  unmount();

  writeDraft({ ...answered(3), accounts: [{ name: "Checking", type: "checking", balance: "1500" }], added: [], amounts: { "0.0": { amount: "1500" } } });
  render(<Harness start={{ mode: "draft" }} />);
  press("Next");
  await screen.findByText("Left after this");
  expect(screen.getByRole("status").textContent).toBe("All of it has a job.");
});

it.each([
  [1, { template: undefined, amounts: undefined, accounts: 2 }],
  [2, { template: undefined, amounts: undefined, accounts: 2 }],
  [3, { template: "everyday", amounts: 3, accounts: 2 }],
  [4, { template: "everyday", amounts: 3, accounts: 2 }],
] as const)("Skip setup on step %i makes the budget that step implies", async (step, implied) => {
  writeDraft(answered(step));
  render(<Harness start={{ mode: "draft" }} />);
  await heading(["Where is your money?", "What do you spend on?", "What does a month cost?", "Give your money its jobs"][step - 1]);
  if (step === 4) await screen.findByText("Bills");
  expect(api.createBudget).not.toHaveBeenCalled();
  press("Skip setup");
  await screen.findByText("Walkthrough closed");
  const answers = vi.mocked(api.setupBudget).mock.calls.at(-1)![0];
  // The typed accounts always count (the row without a name isn't one); categories and what
  // they cost only once the second step has been answered.
  expect(answers.accounts.map((a) => a.name)).toEqual(["Checking", "Visa"]);
  expect(answers.template).toBe(implied.template);
  expect(answers.amounts?.length).toBe(implied.amounts);
  expect(answers.expectedIncome ?? undefined).toBe(step >= 3 ? 500000 : undefined);
  const made = world.saved.get("made")!;
  expect(api.createBudget).toHaveBeenCalledOnce();
  expect(made.accounts).toHaveLength(2);
  expect(made.categories.filter((c) => !c.paymentAccountId)).toHaveLength(step >= 3 ? 16 : 0);
  expect(made.categories.filter((c) => c.target > 0)).toHaveLength(step >= 3 ? 3 : 0);
  // Skipping assigns nothing, opens the budget, and leaves the way back on Budget.
  expect(made.assignments).toEqual([]);
  expect(api.action).not.toHaveBeenCalled();
  expect(world.open).toBe(made);
  expect(readMark("made")).toEqual({ card: true });
  expect(readDraft()).toBeNull();
});

it("keeps the answers as a draft and offers them again after a restart", async () => {
  const first = render(<ToastProvider><FirstRunWelcome openBudget={vi.fn()} onSetup={vi.fn()} /></ToastProvider>);
  expect(screen.queryByRole("button", { name: "Continue setup" })).toBeNull();
  press("Create my budget");
  first.unmount();
  const walk = render(<Harness start={{ mode: "draft" }} />);
  await heading("Where is your money?");
  type("Name", "Joint account");
  type("Today's balance", "1250.50");
  press("Next");
  await heading("What do you spend on?");
  fireEvent.click(await screen.findByRole("button", { name: "Phone" }));
  // The app closes here.
  walk.unmount();
  expect(api.createBudget).not.toHaveBeenCalled();

  render(<ToastProvider><FirstRunWelcome openBudget={vi.fn()} onSetup={vi.fn()} /></ToastProvider>);
  expect(screen.queryByRole("button", { name: "Create my budget" })).toBeNull();
  press("Continue setup");
  cleanup();
  render(<Harness start={{ mode: "draft" }} />);
  await heading("What do you spend on?");
  expect((await screen.findByRole("button", { name: "Phone" })).getAttribute("aria-pressed")).toBe("false");
  press("Back");
  await heading("Where is your money?");
  expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("Joint account");
  expect((screen.getByLabelText("Today's balance") as HTMLInputElement).value).toBe("1250.50");
  // Back from the first step leaves the walkthrough with the answers kept and nothing made.
  press("Back");
  expect(screen.getByText("Walkthrough closed")).toBeTruthy();
  expect(readDraft()?.accounts[0].name).toBe("Joint account");
  expect(api.createBudget).not.toHaveBeenCalled();
});

/** A budget made by skipping after the first step: an account, and nothing else. */
const skipped = (): Budget => ({
  schemaVersion: 2,
  name: "Home",
  ...USD,
  accounts: [{ id: "a0", name: "Checking", kind: "cash", accountType: "checking", openingBalance: 200000, archived: false }],
  categories: [],
  transactions: [],
  assignments: [],
});

it("runs the same steps on a budget that exists, as edits to it", async () => {
  writeMark("made", { card: true });
  render(<Harness start={{ mode: "budget", step: 2 }} budget={skipped()} />);
  await heading("What do you spend on?");
  fireEvent.click(await screen.findByRole("button", { name: "Internet" }));
  press("Next");
  await heading("What does a month cost?");
  expect(world.open!.categories).toHaveLength(15);
  expect(world.open!.groupOrder).toEqual(["Bills", "Needs", "Wants", "Savings"]);
  expect(api.setupBudget).not.toHaveBeenCalled();
  type("Rent or mortgage", "1200");
  type("Vacation", "1200");
  fireEvent.click(screen.getByRole("switch", { name: "A year's amount, for Vacation" }));
  press("Next");
  await heading("Give your money its jobs");
  const targets = Object.fromEntries(world.open!.categories.filter((c) => c.target).map((c) => [c.name, c.target]));
  expect(targets).toEqual({ "Rent or mortgage": 120000, Vacation: 10000 });
  await screen.findByText("Left after this");
  expect(screen.getByRole("status").textContent).toBe("$700.00 is left over. Put it in Emergency fund, or keep it for next month.");
  // Underfunded is previewed on the open budget, as Quick assign does it.
  expect(api.autoAssignPreview).toHaveBeenCalled();
  expect(api.setupPreview).not.toHaveBeenCalled();
  const before = world.undoSteps;
  press("Assign it");
  await heading("How will you keep it up to date?");
  expect(world.undoSteps).toBe(before + 1);
  expect(assigned(world.open!)).toBe(130000);
  expect(readMark("made")).toEqual({ card: true, assigned: true });
  expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
  cleanup();

  // Opened again at the step before: it shows what's there now, and assigns nothing twice.
  render(<Harness start={{ mode: "budget", step: 4 }} budget={world.open} />);
  await heading("Give your money its jobs");
  await screen.findByText("Left after this");
  expect(screen.getByRole("status").textContent).toBe("$700.00 is left over. Put it in Emergency fund, or keep it for next month.");
  expect(document.querySelectorAll(".setup-plan details")).toHaveLength(0);
  const steps = world.undoSteps;
  press("Assign it");
  await heading("How will you keep it up to date?");
  expect(world.undoSteps).toBe(steps);
  expect(assigned(world.open!)).toBe(130000);
  cleanup();
  render(<Harness start={{ mode: "budget", step: 4 }} budget={world.open} />);
  await heading("Give your money its jobs");
  press("Back");
  await heading("What does a month cost?");
  expect((screen.getByLabelText("Rent or mortgage") as HTMLInputElement).value).toBe("1,200.00");
  press("Back");
  await heading("What do you spend on?");
  expect(screen.queryByRole("combobox", { name: "Categories" })).toBeNull();
  expect(screen.getByText("These are your categories. Click one to leave it out, and add any that are missing.")).toBeTruthy();
  // The ones given money in step 4 stay; the rest can still be left out.
  expect(screen.queryByRole("button", { name: "Rent or mortgage" })).toBeNull();
  expect(screen.getByRole("button", { name: "Water" })).toBeTruthy();
  press("Back");
  await heading("Where is your money?");
  expect(screen.getByText("Checking", { selector: "strong" })).toBeTruthy();
  // Skip setup on a budget that exists only leaves.
  press("Skip setup");
  expect(screen.getByText("Walkthrough closed")).toBeTruthy();
  expect(api.createBudget).not.toHaveBeenCalled();
});

it("leaves out a budget's categories that nothing uses yet, and keeps the ones in use", async () => {
  // New budget made it from a template; an account was added, and Groceries was given money by hand.
  const rent = { id: "c-rent", name: "Rent", group: "Bills", target: 0 };
  const water = { id: "c-water", name: "Water", group: "Bills", target: 0 };
  const groceries = { id: "c-food", name: "Groceries", group: "Needs", target: 0 };
  const gifts = { id: "c-gifts", name: "Gifts", group: "Savings", target: 0 };
  const template: Budget = {
    ...skipped(),
    categories: [rent, water, groceries, gifts],
    groupOrder: ["Bills", "Needs", "Savings"],
    assignments: [{ month: MONTH, categoryId: "c-food", amount: 5000 }],
  };
  render(<Harness start={{ mode: "budget", step: 2 }} budget={template} />);
  await heading("What do you spend on?");
  expect(screen.getByText("These are your categories. Click one to leave it out, and add any that are missing.")).toBeTruthy();
  // One with money in it isn't a button: it stays.
  expect(screen.queryByRole("button", { name: "Groceries" })).toBeNull();
  expect(screen.getByText("Groceries").className).toBe("setup-chip fixed");
  press("Water");
  expect(screen.getByRole("button", { name: "Water" }).getAttribute("aria-pressed")).toBe("false");
  press("Gifts");
  // Put back before going on: it stays.
  press("Water");
  press("Water");
  const before = world.undoSteps;
  press("Next");
  await heading("What does a month cost?");
  expect(world.open!.categories.map((c) => c.name)).toEqual(["Rent", "Groceries"]);
  // Savings held only Gifts: the group goes with it. It's one change, which Undo takes back.
  expect(world.open!.groupOrder).toEqual(["Bills", "Needs"]);
  expect(world.undoSteps).toBe(before + 1);
  expect(world.open!.assignments).toEqual(template.assignments);
  cleanup();

  // Leaving them all out, with nothing added, can't go on.
  render(<Harness start={{ mode: "budget", step: 2 }} budget={{ ...template, categories: [rent], assignments: [] }} />);
  await heading("What do you spend on?");
  press("Rent");
  press("Next");
  expect(screen.getByRole("alert").textContent).toBe("Keep or add at least one category to go on.");
});

it("adds a card to an open budget with its payment category, owing what was typed", async () => {
  render(<Harness start={{ mode: "budget", step: 1 }} budget={skipped()} />);
  await heading("Where is your money?");
  press(/Add another/);
  const row = within(screen.getByRole("group", { name: "Account 2" }));
  type("Name", "Visa", row);
  fireEvent.change(row.getByLabelText("Type"), { target: { value: "creditCard" } });
  type("What you owe", "99.50", row);
  press("Next");
  await heading("What do you spend on?");
  const visa = world.open!.accounts[1];
  expect(visa).toMatchObject({ name: "Visa", kind: "credit", accountType: "creditCard", openingBalance: -9950 });
  expect(world.open!.categories).toEqual([expect.objectContaining({ name: "Visa", group: "Credit Card Payments", paymentAccountId: visa.id })]);
});

it("takes in a budget that isn't in use yet, whatever made it, and leaves one that is", async () => {
  const card = (b: Budget, unassigned: number, id: string) => <SetupCard budget={b} budgetId={id} unassigned={unassigned} onContinue={vi.fn()} />;
  const region = () => screen.queryByRole("region", { name: "Finish setting up" });
  // Made from a template in New budget, or on another device: no mark here, an account, nothing spent.
  const young = { ...skipped(), categories: [{ id: "c", name: "Rent", group: "Bills", target: 0 }] };
  const view = render(card(young, 200000, "template"));
  await waitFor(() => expect(region()?.textContent).toContain("3 steps left"));
  expect(readMark("template")).toEqual({ card: true });
  // Started before the walkthrough and left, its checklist still showing.
  localStorage.setItem("lb:setup:left", "on");
  view.rerender(card(young, 200000, "left"));
  await waitFor(() => expect(readMark("left")).toEqual({ card: true }));
  expect(region()).toBeTruthy();
  // Its checklist hidden: it stays hidden.
  localStorage.setItem("lb:setup:hidden", "off");
  view.rerender(card(young, 200000, "hidden"));
  expect(region()).toBeNull();
  expect(localStorage.getItem("lb:setup:hidden")).toBe("off");
  // In use: something was spent in it. It's never offered setup, and gets no mark.
  const purchase = { id: "t", date: "2026-11-02", accountId: "a0", payee: "Shop", memo: "", amount: -500, status: "cleared", splits: [], transferAccountId: null, transferStatus: null };
  view.rerender(card({ ...young, transactions: [purchase] } as Budget, 200000, "used"));
  expect(region()).toBeNull();
  expect(readMark("used")).toBeNull();
  // Everything done by hand: how it's kept up to date isn't asked on its own.
  view.rerender(card({ ...young, categories: [{ ...young.categories[0], target: 100000 }] }, 0, "by-hand"));
  expect(region()).toBeNull();
  expect(readMark("by-hand")).toBeNull();
  // A fresh start is marked as set up when New budget makes it.
  writeMark("fresh", { card: false });
  view.rerender(card(young, 200000, "fresh"));
  expect(region()).toBeNull();
  // A sample or an import was set up somewhere else.
  view.rerender(card({ ...young, sample: {} }, 200000, "sample"));
  expect(region()).toBeNull();
  expect(readMark("sample")).toBeNull();
});

it("keeps the way back in when the walkthrough is left partway on a budget that exists", async () => {
  // New budget made it empty; the walkthrough was opened on it from its page.
  render(<Harness start={{ mode: "budget", step: 1 }} budget={{ ...skipped(), accounts: [] }} />);
  await heading("Where is your money?");
  expect(readMark("made")).toEqual({ card: true });
  type("Name", "Checking");
  type("Today's balance", "2000");
  press("Next");
  await heading("What do you spend on?");
  press("Skip setup");
  expect(screen.getByText("Walkthrough closed")).toBeTruthy();
  const onContinue = vi.fn();
  render(<SetupCard budget={world.open!} budgetId="made" unassigned={200000} onContinue={onContinue} />);
  expect(screen.getByRole("region", { name: "Finish setting up" }).textContent).toContain("4 steps left");
  press("Continue");
  expect(onContinue).toHaveBeenLastCalledWith(2);
  cleanup();
  // A card hidden before comes back when the walkthrough is opened again.
  writeMark("made", { card: false });
  render(<Harness start={{ mode: "budget", step: 2 }} budget={world.open} />);
  await heading("What do you spend on?");
  expect(readMark("made")).toEqual({ card: true });
});

it("shows the setup card on a budget left unfinished, and opens the first step it doesn't answer", async () => {
  const onContinue = vi.fn();
  const budget = skipped();
  const card = (b: Budget, unassigned: number, id = "made") => <SetupCard budget={b} budgetId={id} unassigned={unassigned} onContinue={onContinue} />;
  const view = render(card(budget, 200000));
  act(() => writeMark("made", { card: true }));
  view.rerender(card(budget, 200000));
  expect(screen.getByRole("region", { name: "Finish setting up" }).textContent).toContain("4 steps left");
  press("Continue");
  expect(onContinue).toHaveBeenLastCalledWith(2);

  const withCategories = { ...budget, categories: [{ id: "c", name: "Rent", group: "Bills", target: 0 }] };
  view.rerender(card(withCategories, 200000));
  expect(screen.getByRole("region").textContent).toContain("3 steps left");
  press("Continue");
  expect(onContinue).toHaveBeenLastCalledWith(3);

  const withTargets = { ...withCategories, categories: [{ ...withCategories.categories[0], target: 120000 }] };
  view.rerender(card(withTargets, 200000));
  press("Continue");
  expect(onContinue).toHaveBeenLastCalledWith(4);
  // With no money waiting for a job, only the last step is left.
  view.rerender(card(withTargets, 0));
  expect(screen.getByRole("region").textContent).toContain("1 step left");
  press("Continue");
  expect(onContinue).toHaveBeenLastCalledWith(5);
  // Left-over money the walkthrough already dealt with doesn't bring step 4 back.
  act(() => writeMark("made", { card: true, assigned: true }));
  view.rerender(card(withTargets, 70000));
  press("Continue");
  expect(onContinue).toHaveBeenLastCalledWith(5);

  view.rerender(card({ ...withTargets, accounts: [] }, 0));
  press("Continue");
  expect(onContinue).toHaveBeenLastCalledWith(1);

  // Finished: no card. A sample or an imported budget never has one.
  act(() => writeMark("made", { card: true, assigned: true, keep: "type" }));
  view.rerender(card(withTargets, 0));
  expect(screen.queryByRole("region")).toBeNull();
  act(() => writeMark("made", { card: true }));
  view.rerender(card({ ...withTargets, sample: {} }, 200000));
  expect(screen.queryByRole("region")).toBeNull();

  // Hiding it is one click, and it stays hidden.
  view.rerender(card(withTargets, 200000));
  press("Hide setup");
  expect(screen.queryByRole("region")).toBeNull();
  expect(readMark("made")).toEqual({ card: false });
  view.unmount();
  render(card(withTargets, 200000));
  expect(screen.queryByRole("region")).toBeNull();
});

it("opens a made budget again rather than making a second when opening it failed", async () => {
  writeDraft(answered(2));
  let fail = true;
  function Flaky() {
    const [session, setSession] = useState<SetupSession | null>({ mode: "draft" });
    return session ? (
      <ToastProvider>
        <SetupWalkthrough
          session={session}
          onSession={setSession}
          budget={null}
          budgetId=""
          summary={null}
          month={MONTH}
          edit={vi.fn()}
          transform={vi.fn()}
          openBudget={async () => {
            if (fail) throw new Error("Save the current budget first");
          }}
          close={() => setSession(null)}
        />
      </ToastProvider>
    ) : (
      <p>Walkthrough closed</p>
    );
  }
  render(<Flaky />);
  await heading("What do you spend on?");
  press("Skip setup");
  expect((await screen.findByRole("alert")).textContent).toBe("Save the current budget first");
  fail = false;
  press("Skip setup");
  await waitFor(() => expect(screen.getByText("Walkthrough closed")).toBeTruthy());
  expect(api.createBudget).toHaveBeenCalledOnce();
});

/** The reviewer's run: 3,500 in checking, 420 owed on a card, and a month that costs 3,375. */
const tight = (rent: string): SetupDraft => ({
  ...answered(3),
  accounts: [
    { name: "Checking", type: "checking", balance: "3500" },
    { name: "Visa", type: "creditCard", balance: "420" },
  ],
  leftOut: [],
  added: [],
  amounts: { "0.0": { amount: rent } },
  income: "",
});

it("says the card's debt from the start, so the steps agree when only the card goes short", async () => {
  writeDraft(tight("3375"));
  render(<Harness start={{ mode: "draft" }} />);
  await heading("What does a month cost?");
  const total = document.querySelector(".setup-actions .setup-note")!;
  // The month fits; the card is owed on top of it, and that isn't the month costing too much.
  expect(total.textContent).toBe("Your month costs $3,375.00, plus $420.00 owed on Visa. You have $3,500.00.");
  expect(total.className).toBe("setup-note");
  press("Next");
  await screen.findByText("Left after this");
  const outcome = screen.getByRole("status");
  expect(outcome.textContent).toBe("Everything this month is covered. $125.00 of the $420.00 you owe on Visa is set aside; the other $295.00 waits for payday.");
  expect(outcome.closest(".setup-result")!.className).toBe("setup-result good");
  expect(screen.queryByText(/more than you have right now/)).toBeNull();
  expect(screen.queryByRole("button", { name: /Put it in/ })).toBeNull();
});

it("says two things when the month and the card both go short, and names no card when there are several", async () => {
  writeDraft(tight("3600"));
  const first = render(<Harness start={{ mode: "draft" }} />);
  await heading("What does a month cost?");
  expect(document.querySelector(".setup-actions .setup-note")!.className).toBe("setup-note warning");
  press("Next");
  await screen.findByText("Left after this");
  expect(screen.getByRole("status").textContent).toBe(
    "Your month costs $100.00 more than you have right now. The rest waits for payday; assign it when it arrives. The $420.00 you owe on Visa waits for payday.",
  );
  expect(screen.getByRole("status").closest(".setup-result")!.className).toBe("setup-result warning");
  first.unmount();

  writeDraft({ ...tight("3375"), accounts: [...tight("3375").accounts, { name: "Amex", type: "creditCard", balance: "80" }] });
  render(<Harness start={{ mode: "draft" }} />);
  await heading("What does a month cost?");
  expect(document.querySelector(".setup-actions .setup-note")!.textContent).toBe("Your month costs $3,375.00, plus $500.00 owed on your cards. You have $3,500.00.");
  press("Next");
  await screen.findByText("Left after this");
  expect(screen.getByRole("status").textContent).toBe(
    "Everything this month is covered. $125.00 of the $500.00 you owe on your cards is set aside; the other $375.00 waits for payday.",
  );
});

it("presses the filled button on Enter from a step with no field to type in", async () => {
  writeDraft(answered(2));
  render(<Harness start={{ mode: "draft" }} />);
  await heading("What do you spend on?");
  await screen.findByRole("button", { name: "Phone" });
  // The set's select is the step's field; Enter there goes on, as it does from a text field.
  const set = screen.getByRole("combobox", { name: "Categories" });
  fireEvent.keyDown(set, { key: "Enter" });
  await heading("What does a month cost?");
  // From the last amount Enter isn't taken for moving down: the form's own submit presses Next.
  const last = screen.getByLabelText("Pets");
  expect(fireEvent.keyDown(last, { key: "Enter" })).toBe(true);
  fireEvent.submit(last.closest("form")!);
  const title = await heading("Give your money its jobs");
  await screen.findByText("Left after this");
  // No field here: the caret is on the question, and Enter presses Assign it.
  expect(document.activeElement).toBe(title);
  fireEvent.keyDown(title, { key: "Enter" });
  await heading("How will you keep it up to date?");
  expect(api.createBudget).toHaveBeenCalledOnce();
  expect(document.activeElement).toBe(screen.getByRole("radio", { name: /Type it in as I spend/ }));
});

it("tells a finished budget it's ready once, with the first thing to do for the way chosen", async () => {
  const onKeepUp = vi.fn();
  const budget: Budget = { ...skipped(), categories: [{ id: "c", name: "Rent", group: "Bills", target: 120000 }] };
  const card = <SetupCard budget={budget} budgetId="made" unassigned={0} onContinue={vi.fn()} onKeepUp={onKeepUp} />;
  act(() => writeMark("made", { card: true, assigned: true, keep: "file", ready: true }));
  const view = render(card);
  const line = screen.getByRole("region", { name: "Your budget is ready." });
  expect(line.textContent).toContain("Your budget is ready. Import your bank's file about once a week.");
  expect(line.querySelectorAll("button.primary")).toHaveLength(1);
  press("Import a bank file");
  expect(onKeepUp).toHaveBeenCalledExactlyOnceWith("file");
  // Used, it's gone for good.
  expect(screen.queryByRole("region")).toBeNull();
  expect(readMark("made")).toEqual({ card: true, assigned: true, keep: "file", ready: false });
  view.unmount();
  render(card);
  expect(screen.queryByRole("region")).toBeNull();
  cleanup();

  // Each way has its own sentence and button; closing it is enough.
  for (const [keep, text, button] of [
    ["type", "Add each purchase as you spend.", "Add a transaction"],
    ["bank", "Connect your bank when you're ready.", "Connect a bank"],
  ] as const) {
    act(() => writeMark("made", { card: true, assigned: true, keep, ready: true }));
    const shown = render(card);
    expect(screen.getByRole("region").textContent).toContain(text);
    expect(screen.getByRole("button", { name: button })).toBeTruthy();
    press("Close");
    expect(screen.queryByRole("region")).toBeNull();
    expect(readMark("made")?.ready).toBe(false);
    shown.unmount();
  }
  expect(onKeepUp).toHaveBeenCalledOnce();
});
