import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { produce } from "immer";
import { useState } from "react";
import type { Budget } from "./types";

vi.mock("./bridge", () => ({
  desktop: false,
  api: {
    action: vi.fn(async (b: Budget, a: { type: string; from: string; to: string; ruleIds?: string[] }) => {
      if (a.type === "reapplyPayeeRules") {
        // What workflows.rs does, by way of its mirror.
        const { reapplied } = await import("./bankNames");
        const only = a.ruleIds && new Set(a.ruleIds);
        return {
          ...b,
          transactions: b.transactions.map((t) => {
            const r = reapplied(b, t, only);
            return r ? { ...t, payee: r.payee, splits: [{ ...t.splits[0], categoryId: r.categoryId }] } : t;
          }),
        };
      }
      return a.type === "renamePayee"
        ? {
            ...b,
            transactions: b.transactions.map((t) => (t.payee === a.from ? { ...t, payee: a.to } : t)),
            payees: (b.payees ?? []).map((p) => (p.name === a.from ? { ...p, name: a.to } : p)),
          }
        : b;
    }),
    previewSchedules: vi.fn(async (b: Budget) => b),
  },
}));
import { api } from "./bridge";
import { demoBudget } from "./demo";
import { Payees, payeeUsage, tidyPayees } from "./PayeesPage";
import { payeeIndex } from "./payees";
import { ToastProvider } from "./ui";

afterEach(cleanup);
beforeEach(() => { vi.clearAllMocks(); });

function harness(initial: Budget, page: (props: {
  budget: Budget;
  edit: (fn: (b: Budget) => void) => void;
  transform: (fn: (b: Budget) => Promise<Budget>) => Promise<void>;
  undo: () => void;
}) => React.ReactNode) {
  let current = initial;
  const undo = vi.fn();
  function Harness() {
    const [budget, setBudget] = useState(initial);
    return page({
      budget,
      undo,
      edit: (fn) => {
        current = produce(current, fn);
        setBudget(current);
      },
      transform: async (fn) => {
        current = await fn(current);
        setBudget(current);
      },
    });
  }
  render(
    <ToastProvider>
      <Harness />
    </ToastProvider>,
  );
  return { budget: () => current, undo };
}

it("a payee's default category beats the last one used, unless that category is gone", () => {
  const b = demoBudget();
  b.payees = [
    { id: "p1", name: "Electricity", categoryId: "gifts" },
    { id: "p2", name: "Home", categoryId: "deleted-category" },
    { id: "p3", name: "Salary", categoryId: "__rta" },
  ];
  const index = new Map(payeeIndex(b).map((p) => [p.name, p]));
  expect(index.get("Electricity")).toMatchObject({ categoryId: "gifts", fixed: true, count: 1 });
  expect(index.get("Home")).toMatchObject({ categoryId: "rent" });
  expect(index.get("Home")!.fixed).toBeUndefined();
  expect(index.get("Salary")).toMatchObject({ categoryId: null, fixed: true, count: 0 });
});

it("virtualizes large payee lists while supporting keyboard navigation and search", async () => {
  const b = demoBudget();
  b.transactions = [];
  b.payees = Array.from({ length: 10000 }, (_, i) => ({ id: `p${i}`, name: `Payee ${String(i).padStart(5, "0")}` }));
  harness(b, p => <Payees {...p} />);
  const list = screen.getByRole("listbox", { name: "Payee list" });
  expect(list.tabIndex).toBe(0);
  expect(within(list).getAllByRole("option").length).toBeLessThan(50);
  fireEvent.keyDown(list, { key: "End" });
  fireEvent.keyDown(list, { key: "Enter" });
  expect(screen.getByRole("region", { name: "Payee Payee 09999" })).toBeTruthy();
  expect(document.getElementById(list.getAttribute("aria-activedescendant")!)).toHaveProperty("textContent", expect.stringContaining("Payee 09999"));
  fireEvent.change(screen.getByLabelText("Search payees"), { target: { value: "Payee 00002" } });
  expect(within(list).getAllByRole("option")).toHaveLength(1);
  fireEvent.keyDown(list, { key: " " });
  expect(screen.getByRole("region", { name: "Payee Payee 00002" })).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Search payees"), { target: { value: "" } });
  fireEvent.keyDown(list, { key: "Home" });
  fireEvent.keyDown(list, { key: "ArrowDown" });
  fireEvent.keyDown(list, { key: "Enter" });
  expect(screen.getByRole("region", { name: "Payee Payee 00001" })).toBeTruthy();
});

it("sets a payee's default category", async () => {
  const { budget } = harness(demoBudget(), (p) => <Payees {...p} />);
  fireEvent.click(screen.getByRole("option", { name: /^Electricity/ }));
  const detail = screen.getByRole("region", { name: "Payee Electricity" });
  fireEvent.change(within(detail).getByRole("combobox", { name: "Default category" }), { target: { value: "Gift" } });
  fireEvent.click(screen.getByRole("option", { name: /^Gifts/ }));
  expect(budget().payees).toEqual([expect.objectContaining({ name: "Electricity", categoryId: "gifts" })]);
  expect(await screen.findByText("Electricity now defaults to Gifts.")).toBeTruthy();
});

it("merges one payee into another, saying what moves", async () => {
  const b = demoBudget();
  b.payeeRules = [{ id: "r", match: "cafe", payee: "Little café", categoryId: null }];
  expect(payeeUsage(b, "Little café")).toEqual({ transactions: 1, schedules: 0, rules: 1 });
  const { budget } = harness(b, (p) => <Payees {...p} />);
  fireEvent.click(screen.getByRole("option", { name: /^Little café/ }));
  const detail = screen.getByRole("region", { name: "Payee Little café" });
  fireEvent.change(within(detail).getByRole("combobox", { name: "Merge into" }), { target: { value: "Dinner" } });
  fireEvent.click(screen.getByRole("option", { name: "Dinner with friends" }));
  expect(within(detail).getByText("Moves 1 transaction, 1 rule from Little café to Dinner with friends.")).toBeTruthy();
  fireEvent.click(within(detail).getByRole("button", { name: "Merge" }));
  await waitFor(() => expect(api.action).toHaveBeenCalled());
  expect(api.action).toHaveBeenCalledWith(expect.anything(), { type: "renamePayee", from: "Little café", to: "Dinner with friends" });
  await waitFor(() => expect(budget().transactions.filter((t) => t.payee === "Dinner with friends")).toHaveLength(2));
});

it("keeps one entry per payee after a merge, preferring one with a default", () => {
  const b = demoBudget();
  b.payees = [
    { id: "a", name: "Shop" },
    { id: "b", name: "Shop", categoryId: "groceries" },
    { id: "c", name: "Other" },
  ];
  expect(tidyPayees(b, "Shop").payees!.map((p) => p.id)).toEqual(["b", "c"]);
});

it("adds and deletes import rules, and applies them to transactions already there", async () => {
  const b = demoBudget();
  b.payeeRules = [
    { id: "a", match: "market", payee: "The neighborhood market", categoryId: "groceries" },
    { id: "b", match: "cafe", payee: "Little café", categoryId: null },
  ];
  const base = b.transactions.find((t) => t.payee === "Little café")!;
  b.transactions.push({
    ...base,
    id: "bill",
    payee: "DEWA 0042",
    importedPayee: "DEWA 0042",
    splits: [{ categoryId: null, amount: base.amount }],
  });
  const { budget, undo } = harness(b, (p) => <Payees {...p} />);
  fireEvent.click(screen.getByRole("tab", { name: /Import rules/ }));
  // Order no longer matters: the longest match wins.
  expect(screen.queryByRole("button", { name: /Move rule/ })).toBeNull();
  expect(screen.queryByRole("button", { name: /^Apply to/ })).toBeNull();

  const form = screen.getByRole("form", { name: "New rule" });
  fireEvent.click(within(form).getByRole("button", { name: /Add rule/ }));
  expect(within(form).getByRole("alert").textContent).toContain("Enter the text");
  fireEvent.change(within(form).getByLabelText("When the bank payee contains"), { target: { value: "DEWA" } });
  fireEvent.click(within(form).getByRole("button", { name: /Add rule/ }));
  expect(within(form).getByRole("alert").textContent).toContain("new name, a category");
  fireEvent.change(within(form).getByRole("combobox", { name: "Rule category" }), { target: { value: "Bills" } });
  fireEvent.click(screen.getByRole("option", { name: /^Bills/ }));
  fireEvent.click(within(form).getByRole("button", { name: /Add rule/ }));
  expect(budget().payeeRules![2]).toMatchObject({ match: "DEWA", payee: "", categoryId: "bills" });

  expect(screen.getByText("The rules would rename or categorize 1 transaction you already have.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Apply to 1 transaction" }));
  await screen.findByText("Updated 1 transaction.");
  expect(api.action).toHaveBeenCalledWith(expect.anything(), { type: "reapplyPayeeRules" });
  expect(budget().transactions.find((t) => t.id === "bill")).toMatchObject({
    payee: "DEWA 0042",
    splits: [{ categoryId: "bills" }],
  });
  expect(screen.queryByRole("button", { name: /^Apply to/ })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Delete rule 1" }));
  expect(budget().payeeRules!.map((r) => r.match)).toEqual(["cafe", "DEWA"]);
  fireEvent.click(screen.getAllByRole("button", { name: "Undo" }).at(-1)!);
  expect(undo).toHaveBeenCalled();
});

it("keeps a payee's bank names, learns the ones seen in bank files and applies them", async () => {
  const b = demoBudget();
  const base = b.transactions.find((t) => t.payee === "Little café")!;
  b.transactions.push(
    { ...base, id: "i1", importedPayee: "SQ *LITTLE CAFE 0042" },
    { ...base, id: "i2", payee: "SQ *LITTLE CAFE 0077", importedPayee: "SQ *LITTLE CAFE 0077" },
  );
  b.payeeRules = [{ id: "r", match: "corner", payee: "Little café", categoryId: "groceries" }];
  const { budget } = harness(b, (p) => <Payees {...p} />);
  fireEvent.click(screen.getByRole("option", { name: /^Little café/ }));
  const detail = screen.getByRole("region", { name: "Payee Little café" });
  const names = within(detail).getByRole("list", { name: "Bank names for Little café" });
  expect(names.textContent).toContain("“corner”");
  expect(names.textContent).toContain("Groceries");

  // Bank text on its transactions that no rule names Little café yet.
  const seen = within(detail).getByRole("list", { name: "Seen in bank files" });
  expect(seen.textContent).toContain("SQ *LITTLE CAFE 0042");
  fireEvent.click(within(seen).getByRole("button", { name: "Add “little cafe”" }));
  expect(budget().payeeRules!.at(-1)).toMatchObject({ match: "little cafe", payee: "Little café", categoryId: null });
  expect(within(detail).queryByRole("list", { name: "Seen in bank files" })).toBeNull();

  // The other transaction from that place still has the bank's name; apply the rule to it.
  fireEvent.click(within(detail).getByRole("button", { name: "Apply to 1 earlier transaction" }));
  await screen.findByText("Updated 1 transaction.");
  expect(api.action).toHaveBeenCalledWith(expect.anything(), {
    type: "reapplyPayeeRules",
    ruleIds: ["r", budget().payeeRules![1].id],
  });
  expect(budget().transactions.find((t) => t.id === "i2")!.payee).toBe("Little café");

  const add = within(detail).getByRole("form", { name: "Add a bank name" });
  fireEvent.change(within(add).getByLabelText("Bank name"), { target: { value: "Little-Cafe" } });
  fireEvent.click(within(add).getByRole("button", { name: /Add/ }));
  expect(within(detail).getByRole("alert").textContent).toBe("“Little-Cafe” is already one of Little café's bank names.");
  fireEvent.change(within(add).getByLabelText("Bank name"), { target: { value: "LTL CAFE" } });
  fireEvent.click(within(add).getByRole("button", { name: /Add/ }));
  expect(budget().payeeRules!.map((r) => r.match)).toEqual(["corner", "little cafe", "LTL CAFE"]);

  fireEvent.click(within(detail).getByRole("button", { name: "Remove bank name corner" }));
  expect(budget().payeeRules!.map((r) => r.match)).toEqual(["little cafe", "LTL CAFE"]);
});
