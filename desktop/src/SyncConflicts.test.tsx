import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { BudgetSync, SyncConflict } from "./cloud";
import { formatDate, money } from "./format";
import { SyncConflicts, aboutText, shownValue } from "./SyncConflicts";

afterEach(cleanup);

const conflicts: SyncConflict[] = [
  {
    key: "category/food/name",
    kind: "category",
    label: "Groceries",
    about: { group: "Living" },
    field: "name",
    local: "Groceries",
    remote: "Food & drink",
  },
  {
    key: "transaction/t1",
    kind: "transaction",
    label: "Shop",
    about: { date: "2026-10-15", account: "Card", amount: -2000 },
    field: null,
    local: null,
    remote: { changed: ["memo", "amount"] },
  },
  {
    key: "transaction/t2/amount",
    kind: "transaction",
    label: "Market",
    about: { date: "2026-10-16", account: "Cash", amount: -1200 },
    field: "amount",
    local: { amount: -1200, lines: [{ category: "Food", transfer: null, amount: -1200 }], transfer: null },
    remote: {
      amount: -1000,
      lines: [
        { category: "Food", transfer: null, amount: -400 },
        { category: null, transfer: null, amount: -600 },
      ],
      transfer: null,
    },
  },
];
const budget: BudgetSync = {
  id: "b",
  name: "Household",
  state: "conflict",
  device: "Pixel 8",
  updatedAt: null,
  message: null,
  conflicts,
  notes: [],
};

it("shows each record with both versions and sends the answers", async () => {
  const resolve = vi.fn(async () => {});
  render(<SyncConflicts budget={budget} close={() => {}} resolve={resolve} />);
  expect(screen.getByText("Category: Groceries")).toBeTruthy();
  expect(screen.getByText("Living")).toBeTruthy();
  expect(screen.getByText("Deleted on one, edited on the other")).toBeTruthy();
  expect(screen.getByText("Deleted")).toBeTruthy();
  expect(screen.getByText("Edited: Memo, Amount")).toBeTruthy();
  const apply = screen.getByRole("button", { name: /Keep these and sync/ }) as HTMLButtonElement;
  expect(apply.disabled).toBe(true);
  // Everything from the phone, then this computer's name for the category.
  fireEvent.click(screen.getByRole("button", { name: "Pixel 8's" }));
  fireEvent.click(screen.getAllByRole("radio")[0]);
  expect(apply.disabled).toBe(false);
  fireEvent.click(apply);
  await vi.waitFor(() => expect(resolve).toHaveBeenCalled());
  expect(resolve).toHaveBeenCalledWith({
    "category/food/name": "local",
    "transaction/t1": "cloud",
    "transaction/t2/amount": "cloud",
  });
});

it("reads amounts, splits and the record they belong to", () => {
  expect(shownValue(conflicts[2], conflicts[2].local)).toBe(`${money(-1200)} · Food`);
  expect(shownValue(conflicts[2], conflicts[2].remote)).toBe(
    `${money(-1000)} · Food ${money(-400)}, Ready to Assign ${money(-600)}`,
  );
  expect(aboutText(conflicts[1])).toBe(`${formatDate("2026-10-15")} · Card · ${money(-2000)}`);
  const assigned: SyncConflict = { ...conflicts[0], kind: "assignment", field: "amount", about: { month: "2026-10" }, local: 30000, remote: 25000 };
  expect(shownValue(assigned, assigned.local)).toBe(money(30000));
  expect(aboutText(assigned)).toBe("October 2026");
});


it("shows each side's monthly money impact and named category changes", () => {
  const plan = {type:"monthPlan",currency:"USD",month:"2026-10",baseTotal:10000,total:12500,readyToAssignChange:-2500,moveCount:2,categories:[{id:"food",name:"Food",base:10000,amount:12500,delta:2500}]};
  const text = shownValue({...conflicts[0],kind:"assignment",field:"monthPlan"},plan);
  expect(text).toContain("Month total: " + money(10000,{currency:"USD"}) + " → " + money(12500,{currency:"USD"}));
  expect(text).toContain("Ready to Assign change: " + money(-2500,{currency:"USD"}));
  expect(text).toContain("Moves: 2");
  expect(text).toContain("Food: ");
  expect(text).toContain("Change: " + money(2500,{currency:"USD"}));
});
it("warns that an invariant fallback chooses the whole budget", () => {
  const conflict = {...conflicts[0],kind:"budget" as const,field:"budgetVersion",about:{wholeBudget:true}};
  expect(aboutText(conflict)).toContain("replaces all budget changes");
  expect(shownValue(conflict,{name:"Mine",accounts:2,categories:3,transactions:4,reason:"Transfer mismatch"})).toContain("Transactions: 4");
});

it("shows the source account and both reconciliation states in grouped transaction choices", () => {
  const value = {amount:-1200,lines:[],account:"Checking",status:"reconciled",transfer:"Savings",transferStatus:"cleared"};
  const shown = shownValue(conflicts[2],value);
  expect(shown).toContain("Checking");
  expect(shown).toContain("Reconciled");
  expect(shown).toContain("Transfer: Savings");
  expect(shown).toContain("Transfer status: Cleared");
});
