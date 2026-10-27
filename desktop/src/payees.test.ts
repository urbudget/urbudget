import { expect, it } from "vitest";
import { payeeHabits, payeeIndex } from "./payees";
import { demoBudget } from "./demo";
import type { Transaction } from "./types";

const at = (n: number, payee: string, amount: number, categoryId: string | null, memo = "", extra = {}): Transaction => ({
  id: `${payee}-${n}`, date: `2026-09-${String(n).padStart(2, "0")}`, accountId: "checking", payee, memo, amount,
  status: "cleared", splits: [{ categoryId, amount }], transferAccountId: null, transferStatus: null, ...extra,
});

it("learns which way a payee's money goes and its latest amount that way", () => {
  const budget = demoBudget();
  budget.transactions = [at(1, "Employer", 250000, null), at(10, "Employer", 260000, null), at(20, "Employer", -5000, "gifts")];
  // A payment back out, even the latest, doesn't make the employer a payee you pay.
  expect(payeeHabits(budget).get("Employer")).toMatchObject({ income: true, amount: 260000 });
});

it("takes a payee's category as read only when its latest 10 with one all had it", () => {
  const budget = demoBudget();
  budget.transactions = [
    at(1, "Amazon", -100, "gifts"),
    ...Array.from({ length: 10 }, (_, i) => at(i + 2, "Amazon", -100, "groceries")),
    // Not yet categorized, a split, YNAB's Uncategorized: none of them count.
    at(20, "Amazon", -100, null, "", { hasImportSource: true }),
    { ...at(21, "Amazon", -100, "dining"), splits: [{ categoryId: "dining", amount: -60 }, { categoryId: "gifts", amount: -40 }] },
    at(22, "Amazon", -100, "local-uncategorized"),
    at(1, "Corner shop", -100, "groceries"),
    at(2, "Corner shop", -100, "dining"),
  ];
  const habits = payeeHabits(budget);
  expect(habits.get("Amazon")).toMatchObject({ category: "groceries", oneCategory: true });
  expect(habits.get("Corner shop")).toMatchObject({ category: "dining", oneCategory: false });
  budget.transactions.push(at(12, "Amazon", -100, "gifts"));
  expect(payeeHabits(budget).get("Amazon")).toMatchObject({ category: "gifts", oneCategory: false });
});

it("takes a payee's default category as read, and keeps one for a payee with no transactions", () => {
  const budget = demoBudget();
  budget.transactions = [at(1, "Corner shop", -100, "groceries"), at(2, "Corner shop", -100, "dining")];
  budget.payees = [
    { id: "a", name: "Corner shop", categoryId: "groceries" },
    { id: "b", name: "Landlord", categoryId: "rent" },
    { id: "c", name: "Gone", categoryId: "deleted-category" },
  ];
  const habits = payeeHabits(budget);
  expect(habits.get("Corner shop")).toMatchObject({ category: "groceries", oneCategory: true });
  expect(habits.get("Landlord")).toEqual({ income: false, category: "rent", oneCategory: true, memo: false });
  expect(habits.get("Gone")).toBeUndefined();
});

it("stops at the memo for a payee whose latest 10 typed in mostly had one, not the bank's", () => {
  const budget = demoBudget();
  const imported = { hasImportSource: true };
  budget.transactions = [
    // Older memos fall out of the latest 10: 7 of its 13 typed in have one, but 4 of the latest 10.
    ...Array.from({ length: 3 }, (_, i) => at(i + 1, "Pharmacy", -100, "bills", "Prescription")),
    ...Array.from({ length: 10 }, (_, i) => at(i + 10, "Pharmacy", -100, "bills", i < 6 ? "" : "Vitamins")),
    ...Array.from({ length: 4 }, (_, i) => at(i + 20, "Pharmacy", -100, "bills", "", { memo: "CARD 1234 PHARMACY", ...imported })),
    at(1, "Bookshop", -100, "gifts", "For Sam"),
    at(2, "Bookshop", -100, "gifts", "For Lee"),
    at(3, "Bookshop", -100, "gifts"),
    at(1, "Bank only", -100, "bills", "SEPA 123", imported),
  ];
  const habits = payeeHabits(budget);
  expect(habits.get("Pharmacy")?.memo).toBe(false);
  expect(habits.get("Bookshop")?.memo).toBe(true);
  expect(habits.get("Bank only")?.memo).toBe(false);
});

it("lists a payee's latest single-category transaction's category, whatever order they come in", () => {
  const budget = demoBudget();
  const split = { ...at(20, "Amazon", -100, "dining"), splits: [{ categoryId: "dining", amount: -60 }, { categoryId: "gifts", amount: -40 }] };
  budget.transactions = [split, at(10, "Amazon", -100, "groceries"), at(1, "Amazon", -100, "gifts")];
  expect(payeeIndex(budget).find((p) => p.name === "Amazon")).toMatchObject({ categoryId: "groceries", last: "2026-09-20" });
});

it("counts income left to Unassigned on purpose as the payee's category", () => {
  const budget = demoBudget();
  const pay = (n: number) => ({ ...at(n, "Employer", 250000, null), splits: [{ categoryId: null, amount: 250000, unassigned: true }] });
  budget.transactions = [pay(1), at(5, "Employer", 4000, "gifts"), pay(10)];
  // Not the one categorized refund in between, as if the pay had no category.
  expect(payeeHabits(budget).get("Employer")).toMatchObject({ income: true, category: "__rta", oneCategory: false });
  budget.transactions = [pay(1), pay(10)];
  expect(payeeHabits(budget).get("Employer")).toMatchObject({ category: "__rta", oneCategory: true });
});

it("counts income with no category as Unassigned, marked or not, but not spending waiting for one", () => {
  const budget = demoBudget();
  // An older expense doesn't stand for the pay that came since (Codex review R4).
  budget.transactions = [at(1, "Employer", -5000, "gifts"), at(5, "Employer", 250000, null), at(10, "Employer", 250000, null)];
  expect(payeeHabits(budget).get("Employer")).toMatchObject({ income: true, category: "__rta", oneCategory: false });
  budget.transactions = [at(1, "Shop", -100, "groceries"), at(5, "Shop", -100, null)];
  expect(payeeHabits(budget).get("Shop")).toMatchObject({ category: "groceries", oneCategory: true });
});

it("leaves accounts off the budget out of a payee's categories", () => {
  const budget = demoBudget();
  budget.accounts = [...budget.accounts, { ...budget.accounts[0], id: "brokerage", kind: "tracking" }];
  budget.transactions = [at(1, "Fund", -100, "bills"), at(5, "Fund", 900, null, "", { accountId: "brokerage" })];
  expect(payeeHabits(budget).get("Fund")).toMatchObject({ category: "bills", oneCategory: true });
});

it("ignores a deleted payee's default category", () => {
  const budget = demoBudget();
  budget.transactions = [at(1, "Netflix", -100, "bills")];
  budget.payees = [{ id: "a", name: "Netflix", categoryId: "gifts", deleted: true } as NonNullable<typeof budget.payees>[number]];
  expect(payeeHabits(budget).get("Netflix")).toMatchObject({ category: "bills" });
  expect(payeeIndex(budget).find((p) => p.name === "Netflix")).toMatchObject({ categoryId: "bills" });
  expect(payeeIndex(budget).find((p) => p.name === "Netflix")?.fixed).toBeUndefined();
});
