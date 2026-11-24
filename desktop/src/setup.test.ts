// The walkthrough's answers and what the device keeps of them.
import { beforeEach, expect, it, vi } from "vitest";
import { produce } from "immer";
import { templatesFixture } from "./budgetTemplates.fixture";
import { ar } from "./i18n/ar";
import {
  addAccounts,
  addCategories,
  answersOf,
  clearDraft,
  draftCategories,
  leftoverCategory,
  monthly,
  needsSetup,
  newDraft,
  readDraft,
  readMark,
  stepsLeft,
  updateMark,
  writeDraft,
  writeMark,
  type SetupDraft,
} from "./setup";
import type { Budget } from "./types";

type Fs = { readFileSync(path: string, encoding: "utf8"): string };
const fs = await vi.importActual<Fs>("node:fs");

const comma = { currency: "EUR", numberFormat: { decimal: ",", group: ".", symbol: "after" }, dateFormat: "dmy" } as const;
const draft = (patch: Partial<SetupDraft> = {}): SetupDraft => ({ ...newDraft({ name: " Home ", format: { ...comma } }), ...patch });
const blank = (): Budget => ({ schemaVersion: 2, name: "Home", currency: "EUR", accounts: [], categories: [], transactions: [], assignments: [] });

beforeEach(() => localStorage.clear());

it("starts a draft from the device's formats unless the first screen changed them", () => {
  const guessed = newDraft({ name: "My budget" });
  expect(guessed.format.currency).toMatch(/^[A-Z]{3}$/);
  expect(guessed).toMatchObject({ step: 1, template: "everyday", accounts: [{ name: "", type: "checking", balance: "" }] });
  expect(newDraft({ name: "Trip", format: { ...comma } })).toMatchObject({ name: "Trip", format: comma });
});

it("reads the answers in the draft's own number format, and only as far as the step they were left on", () => {
  const typed = draft({
    accounts: [
      { name: " Giro ", type: "checking", balance: "1.234,50" },
      { name: "Karte", type: "creditCard", balance: "99,9" },
      { name: "", type: "cash", balance: "50" },
      { name: "Bar", type: "cash", balance: "not a number" },
    ],
    leftOut: ["0.2"],
    added: ["Hund", ""],
    amounts: { "0.0": { amount: "800" }, "0.1": { amount: "" }, "1.3": { amount: "1.200", yearly: true }, "0.3": { amount: "x" } },
    income: "2.500",
  });
  const early = answersOf(typed, 1);
  expect(early).toEqual({
    name: "Home",
    currency: "EUR",
    numberFormat: comma.numberFormat,
    dateFormat: "dmy",
    // The row without a name isn't an account; a balance that doesn't read counts as none.
    accounts: [
      { name: "Giro", type: "checking", balance: 123450 },
      { name: "Karte", type: "creditCard", balance: 9990 },
      { name: "Bar", type: "cash", balance: 0 },
    ],
  });
  expect(answersOf(typed, 2)).toEqual(early);
  expect(answersOf(typed, 3)).toEqual({
    ...early,
    template: "everyday",
    leftOut: ["0.2"],
    added: ["Hund", ""],
    amounts: [
      { category: "0.0", amount: 80000, period: "month" },
      { category: "1.3", amount: 120000, period: "year" },
    ],
    expectedIncome: 250000,
  });
  expect(answersOf({ ...typed, step: 4 })).toEqual(answersOf(typed, 3));
  expect(answersOf({ ...typed, income: "" }, 3).expectedIncome).toBeNull();
  // A year's amount as the engine saves it (setup.rs).
  expect([monthly(90000, true), monthly(100000, true), monthly(100000)]).toEqual([7500, 8334, 100000]);
});

it("lists a draft's categories by the place the engine knows them by", () => {
  const [everyday] = templatesFixture();
  const rows = draftCategories({ leftOut: ["0.0", "3.2"], added: ["Pets", " ", "Gym"] }, everyday, "Other");
  expect(rows).toHaveLength(16 - 2 + 2);
  expect(rows[0]).toEqual({ key: "0.1", name: "Electricity", group: "Bills" });
  expect(rows.slice(-2)).toEqual([
    { key: "+0", name: "Pets", group: "Other" },
    { key: "+2", name: "Gym", group: "Other" },
  ]);
  expect(draftCategories({ leftOut: [], added: [] }, undefined, "Other")).toEqual([]);
});

it("keeps the first screen's draft, and forgives what it can't read", () => {
  expect(readDraft()).toBeNull();
  writeDraft(draft({ step: 3, leftOut: ["1.1"], template: "family", income: "2.000" }));
  expect(readDraft()).toMatchObject({ step: 3, leftOut: ["1.1"], name: " Home ", template: "family", income: "2.000" });
  clearDraft();
  expect(readDraft()).toBeNull();
  localStorage.setItem("lb:setup-draft:welcome", "{not json");
  expect(readDraft()).toBeNull();
  // One kept by an earlier build, with fields the draft no longer has.
  localStorage.setItem("lb:setup-draft:welcome", JSON.stringify({ origin: "welcome", localOnly: false, name: "Old", format: { currency: "USD" }, step: 9, accounts: [] }));
  const old = readDraft();
  expect(old).toMatchObject({ step: 1, template: "everyday", income: "", accounts: [{ name: "" }], leftOut: [], amounts: {}, format: { currency: "USD", numberFormat: { decimal: "." } } });
  expect(old).not.toHaveProperty("origin");
});

it("marks a budget's setup on this device, and keeps the old checklist hidden where it was", () => {
  expect(readMark("b")).toBeNull();
  // The old checklist showing is no mark: the budget is looked at afresh. Hidden, it stays hidden.
  localStorage.setItem("lb:setup:b", "on");
  expect(readMark("b")).toBeNull();
  localStorage.setItem("lb:setup:b", "off");
  expect(readMark("b")).toEqual({ card: false });
  writeMark("b", { card: true });
  updateMark("b", { keep: "bank" });
  expect(readMark("b")).toEqual({ card: true, keep: "bank" });
  // A budget made another way gets a mark when a step is done on it.
  updateMark("other", { assigned: true });
  expect(readMark("other")).toEqual({ card: true, assigned: true });
});

it("counts the steps a budget doesn't answer yet", () => {
  const empty = blank();
  expect(stepsLeft(empty, 0, { card: true })).toEqual([1, 2, 3, 5]);
  const withAccount = produce(empty, (d) => addAccounts(d, [{ name: "Visa", type: "creditCard", balance: "10" }], "."));
  // A card's payment category isn't one of the person's own.
  expect(stepsLeft(withAccount, 5000, { card: true })).toEqual([2, 3, 4, 5]);
  const withTarget = { ...withAccount, categories: [...withAccount.categories, { id: "c", name: "Rent", group: "Bills", target: 100 }] };
  expect(stepsLeft(withTarget, 5000, { card: true })).toEqual([4, 5]);
  expect(stepsLeft(withTarget, 5000, { card: true, assigned: true, keep: "type" })).toEqual([]);
  expect(stepsLeft({ ...withTarget, categories: [{ id: "c", name: "Car", group: "Bills", target: 0, goal: { goal_type: "NEED" } }] }, 0, null)).toEqual([5]);
});

it("offers setup to a budget that isn't in use yet, whatever made it", () => {
  const rent = { id: "c", name: "Rent", group: "Bills", target: 0 };
  const account = { id: "a", name: "Checking", kind: "cash" as const, openingBalance: 350000, archived: false };
  const purchase = { id: "t", date: "2026-11-02", accountId: "a", payee: "Shop", memo: "", amount: -500, status: "cleared" as const, splits: [], transferAccountId: null, transferStatus: null };
  // Empty, or a template's categories and nothing else: New budget's two ways to start.
  expect(needsSetup(blank(), 0)).toBe(true);
  expect(needsSetup({ ...blank(), categories: [rent] }, 0)).toBe(true);
  // Accounts and categories typed by hand, with no amounts or money still to assign.
  expect(needsSetup({ ...blank(), accounts: [account], categories: [rent] }, 350000)).toBe(true);
  expect(needsSetup({ ...blank(), accounts: [account], categories: [{ ...rent, target: 100000 }] }, 250000)).toBe(true);
  // All of it done by hand: only how it's kept up to date is unsaid, and that isn't asked on its own.
  expect(needsSetup({ ...blank(), accounts: [account], categories: [{ ...rent, target: 100000 }] }, 0)).toBe(false);
  // In use: something has been spent or received in it.
  expect(needsSetup({ ...blank(), accounts: [account], categories: [rent], transactions: [purchase] } as Budget, 350000)).toBe(false);
  // Set up somewhere else.
  expect(needsSetup({ ...blank(), sample: {} }, 0)).toBe(false);
  expect(needsSetup({ ...blank(), ynabPlanId: "plan" } as Budget, 0)).toBe(false);
  expect(needsSetup({ ...blank(), importedFrom: "YNAB" } as Budget, 0)).toBe(false);
});

it("adds accounts and categories to a budget as the engine would have made them", () => {
  const budget = produce(blank(), (d) => {
    addAccounts(d, [{ name: " Giro ", type: "savings", balance: "1.000,00" }, { name: "", type: "cash", balance: "" }, { name: "Karte", type: "creditCard", balance: "250" }], ",");
    addCategories(d, [
      { key: "0.0", name: "Miete", group: "Fixkosten" },
      { key: "+0", name: "Hund", group: "Other" },
    ]);
  });
  expect(budget.accounts.map((a) => [a.name, a.kind, a.accountType, a.openingBalance])).toEqual([
    ["Giro", "cash", "savings", 100000],
    ["Karte", "credit", "creditCard", -25000],
  ]);
  // The person's own categories come before the card's payment category, and only their groups are ordered.
  expect(budget.categories.map((c) => [c.name, c.group])).toEqual([
    ["Miete", "Fixkosten"],
    ["Hund", "Other"],
    ["Karte", "Credit Card Payments"],
  ]);
  expect(budget.categories[2].paymentAccountId).toBe(budget.accounts[1].id);
  expect(budget.groupOrder).toEqual(["Fixkosten", "Other"]);
  const more = produce(budget, (d) => addCategories(d, [{ key: "+0", name: "Sport", group: "Other" }]));
  expect(more.categories.map((c) => c.name)).toEqual(["Miete", "Hund", "Sport", "Karte"]);
  expect(more.groupOrder).toEqual(["Fixkosten", "Other"]);
});

it("finds Emergency fund by the name the engine gives it in either language", () => {
  const arabic = ar["Emergency fund"] as string;
  expect(leftoverCategory([{ name: "Rent" }, { name: "Emergency fund" }])?.name).toBe("Emergency fund");
  expect(leftoverCategory([{ name: arabic }])?.name).toBe(arabic);
  expect(leftoverCategory([{ name: "Vacation" }])).toBeUndefined();
  // The engine's own Arabic for it and for the added categories' group (arabic.rs), which these must match.
  const engine = fs.readFileSync("../crates/urbudget-core/src/i18n/arabic.rs", "utf8");
  expect(engine).toContain(`("Emergency fund", "${arabic}")`);
  expect(engine).toContain(`("Other", "${ar["Other"] as string}")`);
});
