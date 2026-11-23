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
  ...newDraft("welcome", { name: "My budget", format: { ...USD } }),
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
    return started ? <Harness start={{ mode: "draft", origin: "welcome" }} /> : <ToastProvider><FirstRunWelcome openBudget={vi.fn()} onSetup={() => setStarted(true)} /></ToastProvider>;
  }
  render(<App />);
  press("Create my budget");
  await heading("Where is your money?");
  expect(screen.getByText("Step 1 of 5")).toBeTruthy();
  // Next needs one account with a name.
  press("Next");
  expect(screen.getByRole("alert").textContent).toBe("Add one account with a name to go on.");
  type("Name", "Checking");
  type("Today's balance", "3,700");
  press(/Add another/);
  const card = within(screen.getByRole("group", { name: "Account 2" }));
  type("Name", "Visa", card);
  fireEvent.change(card.getByLabelText("Type"), { target: { value: "creditCard" } });
  type("What you owe", "420", card);
  // A card is owed, not had.
  expect(screen.getByText(/to plan with/).textContent).toBe("You have $3,700.00 to plan with.");
  press("Next");

  await heading("What do you spend on?");
  const water = await screen.findByRole("button", { name: "Water" });
  expect(water.getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(water);
  expect(water.getAttribute("aria-pressed")).toBe("false");
  press(/Add your own/);
  type("Category name", "Pets");
  fireEvent.keyDown(screen.getByLabelText("Category name"), { key: "Enter" });
  expect(screen.getByRole("button", { name: "Remove Pets" })).toBeTruthy();
  press("Next");

  await heading("What does a month cost?");
  expect(screen.queryByLabelText("Water")).toBeNull();
  type("Rent or mortgage", "1500");
  type("Insurance", "900");
  fireEvent.change(screen.getByLabelText("How often, for Insurance"), { target: { value: "year" } });
  expect(screen.getByText("$75.00 a month")).toBeTruthy();
  type("Pets", "40");
  expect(screen.getByText(/Your month costs/).textContent).toBe("Your month costs $1,615.00. You have $3,700.00.");
  type("Rent or mortgage", "15oo");
  press("Next");
  expect(screen.getByRole("alert").textContent).toMatch(/Enter an amount/);
  type("Rent or mortgage", "1500");
  press("Next");

  await heading("Give your money its jobs.");
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
  expect(screen.getByText("$420.00 is set aside to pay your card.")).toBeTruthy();
  // 3,700 less 1,500, 75, 40 and the card's 420.
  expect(screen.getByRole("status").textContent).toBe("$1,665.00 is left over. Put it in Emergency fund, or keep it for next month.");
  press("Put it in Emergency fund");
  expect(screen.getByRole("status").textContent).toBe("All of it has a job.");

  press("Assign it");
  await heading("How will you keep it up to date?");
  expect(api.createBudget).toHaveBeenCalledOnce();
  expect(vi.mocked(api.createBudget).mock.calls[0][2]).toBe(false);
  // The budget was saved with nothing assigned; the money went in as one change Undo takes back.
  expect(world.saved.get("made")!.assignments).toEqual([]);
  expect(world.undoSteps).toBe(1);
  expect(vi.mocked(api.action).mock.calls[0][1]).toMatchObject({ type: "autoAssign", mode: "underfunded", month: MONTH, limitToAvailable: true });
  expect(assigned(world.open!)).toBe(370000);
  expect(world.open!.assignments.find((a) => a.categoryId === "c3.0")?.amount).toBe(166500);
  expect(readDraft("welcome")).toBeNull();
  expect(readMark("made")).toEqual({ card: true, assigned: true });

  // The last step only records the choice, for this budget on this device.
  fireEvent.click(screen.getByRole("radio", { name: /Import my bank's file/ }));
  press("Finish");
  expect(screen.getByText("Walkthrough closed")).toBeTruthy();
  expect(readMark("made")).toEqual({ card: true, assigned: true, keep: "file" });
  expect(world.undoSteps).toBe(1);
});

it("tells a month that costs more than there is, and one that fits exactly", async () => {
  writeDraft({ ...answered(3), accounts: [{ name: "Checking", type: "checking", balance: "1000" }], added: [], amounts: { "0.0": { amount: "1500" } } });
  const { unmount } = render(<Harness start={{ mode: "draft", origin: "welcome" }} />);
  press("Next");
  await screen.findByText("Left after this");
  expect(screen.getByRole("status").textContent).toBe(
    "Your month costs $500.00 more than you have right now. The rest waits for payday; assign it when it arrives.",
  );
  expect(screen.queryByRole("button", { name: /Put it in/ })).toBeNull();
  unmount();

  writeDraft({ ...answered(3), accounts: [{ name: "Checking", type: "checking", balance: "1500" }], added: [], amounts: { "0.0": { amount: "1500" } } });
  render(<Harness start={{ mode: "draft", origin: "welcome" }} />);
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
  render(<Harness start={{ mode: "draft", origin: "welcome" }} />);
  await heading(["Where is your money?", "What do you spend on?", "What does a month cost?", "Give your money its jobs."][step - 1]);
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
  expect(readDraft("welcome")).toBeNull();
});

it("keeps the answers as a draft and offers them again after a restart", async () => {
  const first = render(<ToastProvider><FirstRunWelcome openBudget={vi.fn()} onSetup={vi.fn()} /></ToastProvider>);
  expect(screen.queryByRole("button", { name: "Continue setup" })).toBeNull();
  press("Create my budget");
  first.unmount();
  const walk = render(<Harness start={{ mode: "draft", origin: "welcome" }} />);
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
  render(<Harness start={{ mode: "draft", origin: "welcome" }} />);
  await heading("What do you spend on?");
  expect((await screen.findByRole("button", { name: "Phone" })).getAttribute("aria-pressed")).toBe("false");
  press("Back");
  await heading("Where is your money?");
  expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("Joint account");
  expect((screen.getByLabelText("Today's balance") as HTMLInputElement).value).toBe("1250.50");
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
  fireEvent.change(screen.getByLabelText("How often, for Vacation"), { target: { value: "year" } });
  press("Next");
  await heading("Give your money its jobs.");
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
  // Back shows what's there now; the accounts step lists the one the budget has.
  press("Back");
  await heading("Give your money its jobs.");
  press("Back");
  await heading("What does a month cost?");
  expect((screen.getByLabelText("Rent or mortgage") as HTMLInputElement).value).toBe("1200.00");
  press("Back");
  await heading("What do you spend on?");
  expect(screen.queryByRole("combobox", { name: "Categories" })).toBeNull();
  expect(screen.getByText("These are your categories. Add any that are missing.")).toBeTruthy();
  press("Back");
  await heading("Where is your money?");
  expect(screen.getByText("Checking", { selector: "strong" })).toBeTruthy();
  // Skip setup on a budget that exists only leaves.
  press("Skip setup");
  expect(screen.getByText("Walkthrough closed")).toBeTruthy();
  expect(api.createBudget).not.toHaveBeenCalled();
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

it("shows the setup card on a budget the walkthrough left unfinished, and opens the first step it doesn't answer", async () => {
  const onContinue = vi.fn();
  const budget = skipped();
  const card = (b: Budget, unassigned: number, id = "made") => <SetupCard budget={b} budgetId={id} unassigned={unassigned} onContinue={onContinue} />;
  // A budget the walkthrough didn't make has no card; nor one from before it, with the old checklist's mark.
  const view = render(card(budget, 200000));
  expect(screen.queryByRole("region", { name: "Finish setting up" })).toBeNull();
  localStorage.setItem("lb:setup:old", "on");
  view.rerender(card(budget, 200000, "old"));
  expect(screen.queryByRole("region", { name: "Finish setting up" })).toBeNull();

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
    const [session, setSession] = useState<SetupSession | null>({ mode: "draft", origin: "welcome" });
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
