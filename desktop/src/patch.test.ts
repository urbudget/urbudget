import { produce } from "immer";
import { describe, expect, it } from "vitest";
import cases from "../../shared/fixtures/patch-cases.json";
import { apply, diff, type Patch } from "./patch";
import type { Budget, Transaction } from "./types";

const tx = (id: string, amount = -100): Transaction => ({
  id,
  date: "2026-10-01",
  accountId: "cash",
  payee: "Shop",
  memo: "",
  amount,
  status: "uncleared",
  splits: [{ categoryId: "food", amount }],
  transferAccountId: null,
  transferStatus: null,
});
const base: Budget = {
  schemaVersion: 2,
  name: "Home",
  currency: "SAR",
  accounts: [{ id: "cash", name: "Cash", kind: "cash", openingBalance: 0, archived: false }],
  categories: [
    { id: "food", name: "Food", group: "Living", target: 0 },
    { id: "rent", name: "Rent", group: "Bills", target: 0 },
  ],
  transactions: Array.from({ length: 30 }, (_, i) => tx(`t${i}`)),
  assignments: [{ month: "2026-10", categoryId: "food", amount: 100 }],
  ynabMonths: [{ month: "2026-10-01", note: "" }],
  moneyMoves: [],
};
const roundtrip = (before: Budget, after: Budget) => {
  const patch = diff(before, after);
  const applied = apply(before, JSON.parse(JSON.stringify(patch ?? {})));
  expect(JSON.parse(JSON.stringify(applied))).toEqual(JSON.parse(JSON.stringify(after)));
  return patch;
};

describe("budget patches", () => {
  it("are empty for the same budget", () => {
    expect(diff(base, base)).toBeNull();
    expect(diff(base, produce(base, () => {}))).toBeNull();
  });
  it("carry only the records an edit touched", () => {
    const after = produce(base, (d) => {
      d.transactions[3].memo = "Edited";
      d.transactions = d.transactions.filter((t) => t.id !== "t9");
      d.transactions.push(tx("new"));
      d.name = "Renamed";
    });
    const patch = roundtrip(base, after)!;
    expect(patch.transactions!.upsert.map((t) => t.id)).toEqual(["t3", "new"]);
    expect(patch.transactions!.delete).toEqual(["t9"]);
    expect(patch.transactions!.order).toBeUndefined();
    expect(patch.accounts).toBeUndefined();
    expect(patch.name).toBe("Renamed");
  });
  it("send the full order when records move", () => {
    const after = produce(base, (d) => {
      d.categories.reverse();
    });
    expect(roundtrip(base, after)!.categories!.order).toEqual(["rent", "food"]);
  });
  it("round-trip assignments and extras", () => {
    const after = produce(base, (d) => {
      d.assignments[0].amount = 250;
      d.assignments.push({ month: "2026-11", categoryId: "rent", amount: 5 });
      d.ynabMonths![0].note = "Hi";
      delete d.moneyMoves;
    });
    const patch = roundtrip(base, after)!;
    expect(patch.assignments!.upsert).toHaveLength(2);
    expect(patch.extras!.delete).toEqual(["moneyMoves"]);
    const removed = produce(after, (d) => {
      d.assignments = d.assignments.filter((a) => a.month !== "2026-11");
    });
    expect(roundtrip(after, removed)!.assignments!.delete).toEqual([
      { month: "2026-11", categoryId: "rent" },
    ]);
  });
  it("reuse untouched records when applied", () => {
    const patch = diff(base, produce(base, (d) => void (d.transactions[0].memo = "x")))!;
    const applied = apply(base, patch);
    expect(applied.transactions[1]).toBe(base.transactions[1]);
    expect(applied.accounts).toBe(base.accounts);
  });
  it("match the cases every client shares", () => {
    const shared = cases as unknown as {
      name: string;
      before: Budget;
      patch: Patch;
      after: Budget;
      nativeOnly?: boolean;
    }[];
    for (const { name, before, patch, after, nativeOnly } of shared) {
      if (!nativeOnly) expect(apply(before, patch), name).toEqual(after);
    }
  });
});
