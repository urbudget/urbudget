import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { produce } from "immer";
import { useState } from "react";
import type { Budget, Transaction } from "./types";

vi.mock("./bridge", () => ({
  api: {
    upsertTransaction: vi.fn(async (b: Budget, t: Transaction) => ({
      ...b,
      transactions: [...b.transactions.filter((x) => x.id !== t.id), t],
    })),
    action: vi.fn(async (b: Budget, a: { operation: string; ids: string[] }) =>
      a.operation === "delete" ? { ...b, transactions: b.transactions.filter((t) => !a.ids.includes(t.id)) } : b,
    ),
    previewSchedules: vi.fn(async (b: Budget) => b),
  },
}));
import { api } from "./bridge";
import { demoBudget } from "./demo";
import { money } from "./format";
import { Register, registerEntries, saveSchedule, saveTransaction, scheduledMessage } from "./Register";
import { ToastProvider } from "./ui";
import { today } from "./types";

afterEach(cleanup);
beforeEach(() => vi.clearAllMocks());

function setup(accountId = "all", initial: Budget = demoBudget(), props: Partial<Parameters<typeof Register>[0]> = {}) {
  const undo = vi.fn();
  const onEdit = vi.fn();
  let current = initial;
  function Harness() {
    const [budget, setBudget] = useState(initial);
    return (
      <Register
        budget={budget}
        accountId={accountId}
        edit={(fn) => {
          current = produce(current, fn);
          setBudget(current);
        }}
        transform={async (fn) => {
          current = await fn(current);
          setBudget(current);
        }}
        onEdit={onEdit}
        undo={undo}
        {...props}
      />
    );
  }
  render(
    <ToastProvider>
      <Harness />
    </ToastProvider>,
  );
  return { undo, onEdit, budget: () => current };
}
const dataRows = () => screen.getAllByRole("row").filter((r) => r.hasAttribute("aria-selected"));
const payees = () => dataRows().map((r) => r.querySelector("[data-field=Payee]")?.textContent);

it("running balances include hidden rows and split transfer incoming sides", () => {
  const b = demoBudget();
  const t = b.transactions[0];
  t.accountId = b.accounts[0].id;
  t.amount = -300;
  t.splits = [
    { categoryId: null, amount: -100 },
    {
      categoryId: null,
      amount: -200,
      transferAccountId: b.accounts[1].id,
      transferStatus: "cleared",
    },
  ];
  b.transactions = [t];
  const rows = registerEntries(b, b.accounts[1].id);
  expect(rows).toHaveLength(1);
  expect(rows[0].amount).toBe(200);
  expect(rows[0].balance).toBe(b.accounts[1].openingBalance + 200);
});

it("filtering and selecting targets only visible transactions", () => {
  const b = demoBudget();
  setup("all", b);
  fireEvent.change(screen.getByLabelText("Search transactions"), {
    target: { value: "NoSuchPayee" },
  });
  expect(screen.getByText("No transactions match these filters.")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Search transactions"), {
    target: { value: b.transactions[0].payee },
  });
  fireEvent.click(screen.getByLabelText("Select all filtered transactions"));
  expect(within(screen.getByRole("toolbar", { name: "Selected transactions" })).getByText("1 selected")).toBeTruthy();
});

it("keeps selection summaries correct across scrolling, approval changes and removed transactions", () => {
  const b = demoBudget();
  b.transactions = Array.from({ length: 1000 }, (_, i) => ({ ...b.transactions[0], id: `selected-${i}`, approved: true }));
  const page = (budget: Budget) => (
    <ToastProvider>
      <Register budget={budget} accountId="all" edit={() => {}} transform={async () => {}} onEdit={() => {}} />
    </ToastProvider>
  );
  const view = render(page(b));
  fireEvent.click(screen.getByLabelText("Select all filtered transactions"));
  fireEvent.scroll(document.querySelector(".register-scroll")!, { target: { scrollTop: 10000 } });
  expect((screen.getByLabelText("Select all filtered transactions") as HTMLInputElement).checked).toBe(true);
  expect(screen.getByText("1,000 selected")).toBeTruthy();
  expect(within(screen.getByRole("toolbar", { name: "Selected transactions" })).queryByRole("button", { name: "Approve" })).toBeNull();
  view.rerender(page({ ...b, transactions: b.transactions.slice(1).map((t, i) => i === 0 ? { ...t, approved: false } : t) }));
  const bar = screen.getByRole("toolbar", { name: "Selected transactions" });
  expect(within(bar).getByText("999 selected")).toBeTruthy();
  expect(within(bar).getByRole("button", { name: "Approve" })).toBeTruthy();
  expect((screen.getByLabelText("Select all filtered transactions") as HTMLInputElement).checked).toBe(true);
});

it("adds a transaction inline, filling in the payee's last category", async () => {
  setup("checking");
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  fireEvent.change(within(row).getByRole("combobox", { name: "Payee" }), { target: { value: "Electr" } });
  fireEvent.click(screen.getByRole("option", { name: "Electricity" }));
  expect((within(row).getByRole("combobox", { name: "Category" }) as HTMLInputElement).value).toBe("Bills & utilities");
  const outflow = within(row).getByLabelText("Outflow");
  fireEvent.change(outflow, { target: { value: "12.50+3" } });
  fireEvent.keyDown(outflow, { key: "Enter" });
  await waitFor(() => expect(screen.queryByRole("row", { name: "New transaction" })).toBeNull());
  expect(api.upsertTransaction).toHaveBeenCalledWith(
    expect.anything(),
    expect.objectContaining({
      accountId: "checking",
      payee: "Electricity",
      amount: -1550,
      splits: [expect.objectContaining({ categoryId: "bills", amount: -1550 })],
    }),
  );
});

it("saves an outflow with no category, as one that needs a category", async () => {
  setup("checking");
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  fireEvent.change(within(row).getByLabelText("Outflow"), { target: { value: "5" } });
  fireEvent.click(within(row).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(api.upsertTransaction).toHaveBeenCalled());
  const saved = vi.mocked(api.upsertTransaction).mock.calls[0][1] as Transaction;
  expect(saved.splits).toEqual([expect.objectContaining({ categoryId: null, amount: -500 })]);
  expect(saved.splits[0].unassigned).toBe(false);
});

it("clears a category emptied by hand instead of putting it back", async () => {
  setup("checking");
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  fireEvent.change(within(row).getByRole("combobox", { name: "Payee" }), { target: { value: "Electr" } });
  fireEvent.click(screen.getByRole("option", { name: "Electricity" }));
  const category = within(row).getByRole("combobox", { name: "Category" }) as HTMLInputElement;
  expect(category.value).toBe("Bills & utilities");
  // Empty it and move on to the amount: it stays empty.
  category.focus();
  fireEvent.change(category, { target: { value: "" } });
  fireEvent.blur(category);
  expect(category.value).toBe("");
  const outflow = within(row).getByLabelText("Outflow");
  fireEvent.change(outflow, { target: { value: "8" } });
  fireEvent.keyDown(outflow, { key: "Enter" });
  await waitFor(() => expect(api.upsertTransaction).toHaveBeenCalled());
  const saved = vi.mocked(api.upsertTransaction).mock.calls[0][1] as Transaction;
  expect(saved.splits).toEqual([expect.objectContaining({ categoryId: null, amount: -800 })]);
});

it("edits a row in place and keeps its id", async () => {
  setup("checking");
  const home = dataRows().find((r) => r.textContent?.includes("Home"))!;
  fireEvent.doubleClick(home.querySelector("[data-field=Memo]")!);
  const row = screen.getByRole("row", { name: "Edit transaction" });
  const memo = within(row).getByLabelText("Memo") as HTMLInputElement;
  expect(memo.value).toBe("Monthly rent");
  fireEvent.change(memo, { target: { value: "October rent" } });
  fireEvent.keyDown(memo, { key: "Enter" });
  await waitFor(() => expect(screen.queryByRole("row", { name: "Edit transaction" })).toBeNull());
  expect(api.upsertTransaction).toHaveBeenCalledWith(
    expect.anything(),
    expect.objectContaining({ id: "t1", memo: "October rent", amount: -500000 }),
  );
});

it("expands split lines, edits splits in place, and sends split transfers to the dialog", () => {
  const b = demoBudget();
  b.transactions[0].splits = [
    { categoryId: "rent", amount: -400000, memo: "Rent part" },
    { categoryId: "bills", amount: -100000, memo: "Service charge" },
  ];
  b.transactions[3].splits = [
    { categoryId: "bills", amount: -21840 },
    { categoryId: null, amount: -10000, transferAccountId: "savings", transferStatus: "uncleared" },
  ];
  const { onEdit } = setup("checking", b);
  const home = dataRows().find((r) => r.textContent?.includes("Home"))!;
  fireEvent.click(within(home).getByRole("button", { name: /Split \(2\)/ }));
  expect(screen.getByText("Service charge")).toBeTruthy();
  fireEvent.doubleClick(home.querySelector("[data-field=Payee]")!);
  const editor = screen.getByRole("row", { name: "Edit transaction" });
  expect((within(editor).getByLabelText("Memo 2") as HTMLInputElement).value).toBe("Service charge");
  expect((within(editor).getByLabelText("Line 1 Outflow") as HTMLInputElement).value).toBe("4000.00");
  expect(onEdit).not.toHaveBeenCalled();
  fireEvent.keyDown(editor, { key: "Escape" });
  const electricity = dataRows().find((r) => r.textContent?.includes("Electricity"))!;
  fireEvent.doubleClick(electricity.querySelector("[data-field=Payee]")!);
  expect(onEdit).toHaveBeenCalledWith(expect.objectContaining({ id: "t4" }));
});

it("chooses a transfer from the payee field", async () => {
  setup("checking");
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  fireEvent.change(within(row).getByRole("combobox", { name: "Payee" }), { target: { value: "Sav" } });
  fireEvent.click(screen.getByRole("option", { name: "Transfer: Savings" }));
  // Between two budget accounts a transfer has no category.
  expect(within(row).queryByRole("combobox", { name: "Category" })).toBeNull();
  fireEvent.change(within(row).getByLabelText("Outflow"), { target: { value: "100" } });
  fireEvent.keyDown(within(row).getByLabelText("Outflow"), { key: "Enter" });
  await waitFor(() => expect(api.upsertTransaction).toHaveBeenCalled());
  expect(api.upsertTransaction).toHaveBeenCalledWith(
    expect.anything(),
    expect.objectContaining({
      accountId: "checking",
      transferAccountId: "savings",
      transferStatus: "uncleared",
      payee: "",
      amount: -10000,
      splits: [],
    }),
  );
});

it("shows a transfer from each side, linked to the other, with no category needed", () => {
  const withTransfer = produce(demoBudget(), (d) => {
    d.transactions.push({
      id: "move",
      date: today(),
      accountId: "checking",
      payee: "",
      memo: "",
      amount: -5000,
      status: "uncleared",
      splits: [],
      transferAccountId: "savings",
      transferStatus: "uncleared",
    });
  });
  const onOpenTransfer = vi.fn();
  setup("checking", withTransfer, { onOpenTransfer });
  const out = dataRows().find((r) => r.textContent?.includes("To: Savings"))!;
  expect(out.querySelector("[data-field=Payee]")?.textContent).toBe("To: Savings");
  expect(out.querySelector("[data-field=Category]")?.textContent).toBe("No Category (Transfer)");
  fireEvent.click(within(out).getByRole("button", { name: "Show in Savings" }));
  expect(onOpenTransfer).toHaveBeenCalledWith("savings", "move");
  // The button opens the other side without selecting its own row; there, the transaction is selected.
  expect(out.getAttribute("aria-selected")).toBe("false");
  cleanup();
  setup("savings", withTransfer, { initialFilter: { focus: "move" } });
  const other = dataRows().find((r) => r.textContent?.includes("From: Everyday account"))!;
  expect(other.getAttribute("aria-selected")).toBe("true");
});

it("opens on the transactions an assistant's card names, all of them selected", () => {
  setup();
  // Two that the register shows, by the ids its rows go by.
  const ids = dataRows()
    .slice(1, 3)
    .map((r) => r.id.replace(/^reg-/, ""));
  expect(ids).toHaveLength(2);
  cleanup();
  // One that's gone since the card was made is passed over.
  setup("all", demoBudget(), { initialFilter: { select: [ids[1], "gone", ids[0]] } });
  const selected = dataRows().filter((r) => r.getAttribute("aria-selected") === "true");
  expect(selected.map((r) => r.id).sort()).toEqual(ids.map((id) => `reg-${id}`).sort());
  // With them selected, what can be done to a selection is offered.
  expect(screen.getByRole("toolbar", { name: "Selected transactions" })).toBeTruthy();
});

it("distinguishes spending awaiting a category from explicit Unassigned money and filters it", () => {
  const b = produce(demoBudget(), (d) => {
    const base = d.transactions.find((t) => t.accountId === "checking")!;
    d.transactions.push(
      { ...base, id: "gift", payee: "Grandma", amount: 10000, splits: [{ categoryId: null, amount: 10000 }] },
      { ...base, id: "fix", payee: "Correction", amount: -2500, splits: [{ categoryId: null, amount: -2500 }] },
      { ...base, id: "explicit", payee: "Explicit", amount: -2500, splits: [{ categoryId: null, amount: -2500, unassigned: true }] },
    );
  });
  setup("checking", b);
  const category = (payee: string) =>
    dataRows().find((r) => r.textContent?.includes(payee))?.querySelector("[data-field=Category]")?.textContent;
  expect(category("Grandma")).toBe("To Unassigned");
  expect(category("Correction")).toBe("Needs a category");
  expect(category("Explicit")).toBe("From Unassigned");
  expect(screen.getAllByRole("row").at(-1)!.textContent).toContain("To Unassigned");
  fireEvent.click(screen.getByRole("button", { name: /^Filters/ }));
  fireEvent.change(screen.getByLabelText("Flag & review"), { target: { value: "categorization" } });
  fireEvent.keyDown(document, { key: "Escape" });
  expect(payees()).toEqual(["Correction"]);
});

it("creates a category from the category field and uses it", async () => {
  const { budget } = setup("checking");
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  fireEvent.change(within(row).getByRole("combobox", { name: "Payee" }), { target: { value: "Petco" } });
  const category = within(row).getByRole("combobox", { name: "Category" });
  fireEvent.change(category, { target: { value: "Pet food" } });
  fireEvent.keyDown(category, { key: "Enter" });
  const dialog = await screen.findByRole("dialog", { name: "New category" });
  expect(within(dialog).getByLabelText("Name")).toHaveProperty("value", "Pet food");
  fireEvent.click(within(dialog).getByRole("button", { name: "Create category" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "New category" })).toBeNull());
  const created = budget().categories.find((c) => c.name === "Pet food");
  expect(created).toBeTruthy();
  expect(category).toHaveProperty("value", "Pet food");
  fireEvent.change(within(row).getByLabelText("Outflow"), { target: { value: "38.20" } });
  fireEvent.keyDown(within(row).getByLabelText("Outflow"), { key: "Enter" });
  await waitFor(() =>
    expect(api.upsertTransaction).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ payee: "Petco", splits: [expect.objectContaining({ categoryId: created!.id, amount: -3820 })] }),
    ),
  );
});

it("asks for a category on a transfer that leaves the budget", async () => {
  const b = demoBudget();
  b.accounts.push({ id: "asset", name: "Investment", kind: "tracking", onBudget: false, openingBalance: 0, archived: false });
  setup("checking", b);
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  fireEvent.change(within(row).getByRole("combobox", { name: "Payee" }), { target: { value: "Inv" } });
  fireEvent.click(screen.getByRole("option", { name: "Transfer: Investment" }));
  fireEvent.change(within(row).getByRole("combobox", { name: "Category" }), { target: { value: "Gifts" } });
  fireEvent.click(screen.getByRole("option", { name: /^Gifts/ }));
  fireEvent.change(within(row).getByLabelText("Outflow"), { target: { value: "20" } });
  fireEvent.click(within(row).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(api.upsertTransaction).toHaveBeenCalled());
  expect(api.upsertTransaction).toHaveBeenCalledWith(
    expect.anything(),
    expect.objectContaining({
      transferAccountId: "asset",
      splits: [expect.objectContaining({ categoryId: "gifts", amount: -2000 })],
    }),
  );
});

it("edits a transfer from the receiving account without flipping it", async () => {
  const b = demoBudget();
  b.transactions = [
    {
      id: "move",
      date: b.transactions[0].date,
      accountId: "checking",
      payee: "",
      memo: "Sent",
      amount: -10000,
      status: "cleared",
      splits: [],
      transferAccountId: "savings",
      transferStatus: "uncleared",
      transferSource: { memo: "Received" },
    },
  ];
  setup("savings", b);
  const row = dataRows()[0];
  expect(row.querySelector("[data-field=Inflow]")!.textContent).toBe("100.00 SAR");
  fireEvent.doubleClick(row.querySelector("[data-field=Memo]")!);
  const editor = screen.getByRole("row", { name: "Edit transaction" });
  expect((within(editor).getByRole("combobox", { name: "Payee" }) as HTMLInputElement).value).toBe(
    "Transfer: Everyday account",
  );
  expect((within(editor).getByLabelText("Memo") as HTMLInputElement).value).toBe("Received");
  fireEvent.change(within(editor).getByLabelText("Memo"), { target: { value: "Moved" } });
  fireEvent.change(within(editor).getByLabelText("Inflow"), { target: { value: "150" } });
  fireEvent.click(within(editor).getByRole("button", { name: "Cleared" }));
  fireEvent.keyDown(within(editor).getByLabelText("Memo"), { key: "Enter" });
  await waitFor(() => expect(api.upsertTransaction).toHaveBeenCalled());
  expect(api.upsertTransaction).toHaveBeenCalledWith(
    expect.anything(),
    expect.objectContaining({
      id: "move",
      accountId: "checking",
      transferAccountId: "savings",
      amount: -15000,
      memo: "Sent",
      status: "cleared",
      transferStatus: "cleared",
      transferSource: { memo: "Moved" },
    }),
  );
});

it("splits a new transaction into lines that must add up", async () => {
  setup("checking");
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  fireEvent.change(within(row).getByRole("combobox", { name: "Payee" }), { target: { value: "Market" } });
  fireEvent.change(within(row).getByLabelText("Outflow"), { target: { value: "100" } });
  fireEvent.click(within(row).getByRole("button", { name: /Split/ }));
  const line = (n: number) => within(row).getByRole("row", { name: `Split line ${n}` });
  expect((within(line(1)).getByLabelText("Line 1 Outflow") as HTMLInputElement).value).toBe("100.00");
  fireEvent.change(within(line(1)).getByRole("combobox", { name: "Category 1" }), { target: { value: "Groc" } });
  fireEvent.click(screen.getByRole("option", { name: /^Groceries/ }));
  fireEvent.change(within(line(1)).getByLabelText("Line 1 Outflow"), { target: { value: "60" } });
  expect(within(row).getByText("40.00 SAR left to split")).toBeTruthy();
  fireEvent.click(within(row).getByRole("button", { name: "Save" }));
  expect(within(row).getByRole("alert").textContent).toContain("must add up");
  fireEvent.change(within(line(2)).getByRole("combobox", { name: "Category 2" }), { target: { value: "Gift" } });
  fireEvent.click(screen.getByRole("option", { name: /^Gifts/ }));
  fireEvent.change(within(line(2)).getByLabelText("Line 2 Outflow"), { target: { value: "40" } });
  fireEvent.click(within(row).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(api.upsertTransaction).toHaveBeenCalled());
  expect(api.upsertTransaction).toHaveBeenCalledWith(
    expect.anything(),
    expect.objectContaining({
      amount: -10000,
      splits: [
        expect.objectContaining({ categoryId: "groceries", amount: -6000 }),
        expect.objectContaining({ categoryId: "gifts", amount: -4000 }),
      ],
    }),
  );
});

it("starts an account's running balance from its opening balance, listed last", () => {
  setup("checking");
  const start = screen.getAllByRole("row").at(-1)!;
  expect(start.textContent).toContain("Starting balance");
  const headers = screen.getAllByRole("columnheader");
  const cells = within(start).getAllByRole("cell");
  expect(cells).toHaveLength(headers.length);
  expect(cells[headers.indexOf(screen.getByRole("columnheader", { name: "Payee" }))].textContent).toBe("Starting balance");
  // 18,500.00 in, and the balance it leaves.
  expect(within(start).getAllByText("18,500.00 SAR")).toHaveLength(2);
  fireEvent.change(screen.getByLabelText("Search transactions"), { target: { value: "Home" } });
  expect(screen.queryByText("Starting balance")).toBeNull();
  cleanup();
  setup("all");
  expect(screen.queryByText("Starting balance")).toBeNull();
});

it("sorts by column and shows balances only in date order", () => {
  setup("checking");
  expect(screen.getByRole("columnheader", { name: "Balance" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Payee" }));
  expect(payees()).toEqual(["A thoughtful something", "Electricity", "Home"]);
  expect(screen.queryByRole("columnheader", { name: "Balance" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Payee" }));
  expect(payees()).toEqual(["Home", "Electricity", "A thoughtful something"]);
  fireEvent.click(screen.getByRole("button", { name: "Outflow" }));
  expect(payees()).toEqual(["Home", "A thoughtful something", "Electricity"]);
});

it("selects a range with Shift and deletes it with an Undo", async () => {
  const { undo } = setup("all");
  const rows = dataRows();
  fireEvent.click(rows[0]);
  fireEvent.click(rows[3], { shiftKey: true });
  expect(dataRows().filter((r) => r.getAttribute("aria-selected") === "true")).toHaveLength(4);
  fireEvent.keyDown(rows[0].closest(".register-scroll")!, { key: "Delete" });
  expect(await screen.findByText("4 transactions deleted.")).toBeTruthy();
  expect(api.action).toHaveBeenCalledWith(
    expect.anything(),
    expect.objectContaining({ operation: "delete", ids: ["t7", "t6", "t5", "t4"] }),
  );
  expect(dataRows()).toHaveLength(3);
  fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  expect(undo).toHaveBeenCalled();
});

it("checks rows from anywhere in the checkbox cell, checks ranges with Shift, and totals the selection", () => {
  const b = demoBudget();
  const base = b.transactions.find((t) => !t.transferAccountId && t.splits.length === 1)!;
  const entry = (id: string, date: string, amount: number): Transaction =>
    ({ ...base, id, payee: id, date, amount, flag: null, approved: true, splits: [{ ...base.splits[0], amount }] });
  b.transactions = [entry("Lunch", "2026-01-04", -1000), entry("Fuel", "2026-01-03", -2550), entry("Salary", "2026-01-02", 5000), entry("Coffee", "2026-01-01", -300)];
  setup("all", b);
  const picked = () => payees().filter((_, i) => dataRows()[i].getAttribute("aria-selected") === "true");
  const checkbox = (payee: string) => screen.getByLabelText(new RegExp(`^Select ${payee} `));
  const bar = () => screen.getByRole("toolbar", { name: "Selected transactions" });
  // The net, then each side (its tooltip), formatted as the columns format amounts.
  const totals = () => {
    const total = bar().querySelector(".bulk-total")!;
    return [total.querySelector("strong")?.textContent, total.getAttribute("title")];
  };

  fireEvent.click(checkbox("Lunch"));
  // Just beside the checkbox adds the row instead of replacing the selection with it.
  fireEvent.click(checkbox("Salary").closest(".select-cell")!);
  expect(picked()).toEqual(["Lunch", "Salary"]);
  expect(totals()).toEqual([money(4000), `${money(1000)} out · ${money(5000)} in`]);

  // Shift checks everything from the last row checked; on a checked row, it unchecks them.
  fireEvent.click(checkbox("Coffee"), { shiftKey: true });
  expect(picked()).toEqual(["Lunch", "Salary", "Coffee"]);
  fireEvent.click(checkbox("Lunch").closest(".select-cell")!, { shiftKey: true });
  expect(picked()).toEqual([]);
  fireEvent.click(checkbox("Fuel"));
  fireEvent.click(checkbox("Coffee"), { shiftKey: true });
  expect(picked()).toEqual(["Fuel", "Salary", "Coffee"]);
  expect(totals()).toEqual([money(2150), `${money(2850)} out · ${money(5000)} in`]);

  // Beside the select-all checkbox works it too.
  fireEvent.click(screen.getByLabelText("Select all filtered transactions").closest(".select-cell")!);
  expect(picked()).toHaveLength(4);
  fireEvent.click(screen.getByLabelText("Select all filtered transactions"));
  fireEvent.click(checkbox("Fuel"));
  expect(totals()).toEqual([money(-2550), `${money(2550)} out · ${money(0)} in`]);
  // The count's pill ends the selection.
  fireEvent.click(within(bar()).getByRole("button", { name: "Clear selection" }));
  expect(picked()).toEqual([]);
});

it("moves and extends the selection with the arrow keys", () => {
  setup("all");
  const rows = dataRows();
  const grid = rows[0].closest(".register-scroll")!;
  fireEvent.click(rows[1]);
  fireEvent.keyDown(grid, { key: "ArrowDown" });
  expect(dataRows()[2].getAttribute("aria-selected")).toBe("true");
  expect(dataRows()[1].getAttribute("aria-selected")).toBe("false");
  fireEvent.keyDown(grid, { key: "ArrowDown", shiftKey: true });
  fireEvent.keyDown(grid, { key: "ArrowDown", shiftKey: true });
  expect(dataRows().filter((r) => r.getAttribute("aria-selected") === "true")).toHaveLength(3);
});

it("lists upcoming scheduled transactions and enters one now", async () => {
  const b = demoBudget();
  b.schedules = [
    {
      id: "gym",
      nextDate: "2999-01-01",
      anchorDate: "2999-01-01",
      frequency: "monthly",
      paused: false,
      transaction: { ...b.transactions[0], id: "template", payee: "Gym" },
    },
  ];
  setup("checking", b);
  const upcoming = screen.getByText("Gym").closest("[role=row]") as HTMLElement;
  // It's marked as scheduled and says how often it repeats.
  expect(within(upcoming).getByRole("button", { name: "Schedule options: Gym" })).toBeTruthy();
  expect(within(upcoming).getByText("Monthly")).toBeTruthy();
  fireEvent.click(within(upcoming).getByRole("button", { name: "Enter" }));
  await screen.findByText("Entered Gym.");
  expect(api.previewSchedules).toHaveBeenCalledWith(expect.anything(), today(), "gym", false);
});

it("saving with Repeat enters this occurrence and schedules the next", async () => {
  const b = demoBudget();
  const t = { ...b.transactions[0], id: "new", date: "2020-01-15" };
  await saveTransaction(b, t, "monthly");
  const [scheduled] = vi.mocked(api.upsertTransaction).mock.calls[0];
  expect(scheduled.schedules).toEqual([
    expect.objectContaining({ frequency: "monthly", nextDate: "2020-01-15", anchorDate: "2020-01-15" }),
  ]);
  const id = scheduled.schedules![0].id;
  expect(api.previewSchedules).toHaveBeenCalledWith(expect.anything(), "2020-01-15", id, true);

  vi.clearAllMocks();
  const later = await saveTransaction(b, { ...t, status: "uncleared", date: "2999-01-01" }, "yearly");
  expect(api.upsertTransaction).not.toHaveBeenCalled();
  expect(later.transactions).toHaveLength(b.transactions.length);
  expect(later.schedules?.[0].frequency).toBe("yearly");
});

it("retrying a recurring transaction reuses its schedule without skipping another occurrence", async () => {
  const b = demoBudget();
  const t = { ...b.transactions[0], id: "stable-draft", date: "2020-01-15" };
  vi.mocked(api.previewSchedules).mockImplementationOnce(async (budget) => ({
    ...budget,
    schedules: budget.schedules!.map((s) => ({ ...s, nextDate: "2020-02-15" })),
  }));
  const first = await saveTransaction(b, t, "monthly");
  const retry = await saveTransaction(first, t, "monthly");
  expect(retry.schedules).toHaveLength(1);
  expect(retry.schedules![0]).toMatchObject({ id: "stable-draft", nextDate: "2020-02-15", transaction: { id: "stable-draft" } });
  expect(retry.transactions.filter((entry) => entry.id === t.id)).toHaveLength(1);
  expect(api.previewSchedules).toHaveBeenCalledTimes(1);

  const future = { ...t, id: "future-draft", date: "2999-01-01" };
  const scheduled = await saveTransaction(b, future, "yearly");
  const scheduledAgain = await saveTransaction(scheduled, future, "yearly");
  expect(scheduledAgain.schedules).toHaveLength(1);
  expect(scheduledAgain.schedules![0].id).toBe(scheduled.schedules![0].id);
});

it.each([false, true])("C clears the receiving side of a transfer (split: %s)", async (split) => {
  const b = demoBudget();
  b.transactions = [{
    ...b.transactions[0], id: "incoming", accountId: "checking", amount: -10000, status: "cleared",
    transferAccountId: split ? null : "savings", transferStatus: "uncleared",
    splits: split ? [{ categoryId: null, amount: -10000, transferAccountId: "savings", transferStatus: "uncleared" }] : [],
  }];
  setup("savings", b);
  fireEvent.click(screen.getByLabelText("Select all filtered transactions"));
  fireEvent.keyDown(screen.getByRole("grid"), { key: "c" });
  await waitFor(() => expect(api.action).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ operation: "status", value: "cleared", accountId: "savings", ids: ["incoming"] })));
});

it("clears the selection with Esc, from a row's checkbox or anywhere on the page", () => {
  setup("all");
  const box = within(dataRows()[0]).getByRole("checkbox");
  fireEvent.click(box);
  expect(dataRows()[0].getAttribute("aria-selected")).toBe("true");
  fireEvent.keyDown(box, { key: "Escape" });
  expect(dataRows()[0].getAttribute("aria-selected")).toBe("false");
  fireEvent.click(within(dataRows()[1]).getByRole("checkbox"));
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(dataRows().some((r) => r.getAttribute("aria-selected") === "true")).toBe(false);
});

it("clears, approves and adds with single keys, with no delete button on each row", async () => {
  const b = demoBudget();
  b.transactions.forEach((t) => (t.approved = false));
  setup("all", b);
  expect(screen.queryAllByRole("button", { name: /^Delete / })).toHaveLength(0);
  const grid = dataRows()[0].closest(".register-scroll")!;
  fireEvent.click(dataRows()[0]);
  fireEvent.keyDown(grid, { key: "c" });
  await waitFor(() =>
    expect(api.action).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ operation: "status" })),
  );
  fireEvent.keyDown(grid, { key: "a" });
  await waitFor(() =>
    expect(api.action).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ operation: "approve" })),
  );
  fireEvent.keyDown(grid, { key: "n" });
  expect(screen.getByRole("row", { name: "New transaction" })).toBeTruthy();
});

it("shows flags by the budget's names in the Flag column and sets one from its menu", async () => {
  const b = demoBudget();
  b.flagNames = { red: "Reimburse" };
  b.transactions = b.transactions.map((t, i) => ({ ...t, flag: i === 0 ? "red" : null }));
  const { budget } = setup("all", b);
  expect(screen.getByRole("button", { name: "Reimburse flag: change" })).toBeTruthy();
  const unflagged = dataRows().find((r) => !r.querySelector(".flag-pick:not(.empty)"))!;
  fireEvent.click(within(unflagged).getByRole("button", { name: /^Flag / }));
  // The menu lists every flag by its name; colors without one keep their color.
  expect(screen.getByRole("menuitem", { name: /Reimburse/ })).toBeTruthy();
  fireEvent.click(screen.getByRole("menuitem", { name: /Orange/ }));
  expect(budget().transactions.filter((t) => t.flag === "orange")).toHaveLength(1);
  expect(budget().transactions.filter((t) => t.flag === "red")).toHaveLength(1);
});

it("names the flags from the Flag menu, and a blank name goes back to the color", async () => {
  const b = demoBudget();
  b.flagNames = { blue: "Tax" };
  const { budget } = setup("all", b);
  fireEvent.click(within(dataRows()[0]).getByRole("button", { name: /^Flag / }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Name the flags…" }));
  const dialog = screen.getByRole("dialog", { name: "Flag names" });
  fireEvent.change(within(dialog).getByLabelText("Red flag's name"), { target: { value: " Reimburse " } });
  fireEvent.change(within(dialog).getByLabelText("Blue flag's name"), { target: { value: "" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Save names" }));
  expect(budget().flagNames).toEqual({ red: "Reimburse" });
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("sets a flag in the row editor and leaves unflagged transactions without one", async () => {
  const b = demoBudget();
  b.flagNames = { green: "Shared" };
  b.transactions = b.transactions.map((t) => ({ ...t, flag: undefined }));
  setup("checking", b);
  const row = dataRows().find((r) => r.querySelector("[data-field=Memo]"))!;
  fireEvent.doubleClick(row.querySelector("[data-field=Memo]")!);
  const editor = screen.getByRole("row", { name: "Edit transaction" });
  const select = within(editor).getByLabelText("Flag") as HTMLSelectElement;
  expect([...select.options].map((o) => o.textContent)).toContain("Shared");
  fireEvent.change(select, { target: { value: "green" } });
  fireEvent.keyDown(within(editor).getByLabelText("Memo"), { key: "Enter" });
  await waitFor(() => expect(api.upsertTransaction).toHaveBeenCalled());
  expect(vi.mocked(api.upsertTransaction).mock.calls[0][1]).toMatchObject({ flag: "green" });
});

it("leaves the Flag column out when it's turned off in Settings", () => {
  localStorage.setItem("lb:prefs", JSON.stringify({ flagColumn: false }));
  try {
    setup("all");
    expect(document.querySelector(".register.no-flag")).toBeTruthy();
  } finally {
    localStorage.removeItem("lb:prefs");
  }
  cleanup();
  setup("all");
  expect(document.querySelector(".register.no-flag")).toBeNull();
});

it("keeps icon columns narrow, fits dates and amounts, and shares the rest among text columns", () => {
  localStorage.setItem("lb:columns:register-v3", JSON.stringify({ payee: 260 }));
  try {
    const b = demoBudget();
    b.transactions[0].approved = false;
    setup("checking", b);
  } finally {
    localStorage.removeItem("lb:columns:register-v3");
  }
  const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
  expect(headers.slice(1, 4)).toEqual(["Flag", "Date", "Payee"]);
  const tracks = (document.querySelector(".register") as HTMLElement).style.getPropertyValue("--cols").split(/ (?![^(]*\))/);
  const [select, flag, date, payee, category, memo, outflow, inflow, , cleared] = tracks;
  expect([select, flag, cleared]).toEqual(["36px", "40px", "56px"]);
  expect(date).toMatch(/^\d+px$/);
  expect(outflow).toMatch(/^\d+px$/);
  expect(inflow).toBe(outflow);
  // Payee was dragged to a width; Category and Memo share what's left.
  expect(payee).toBe("minmax(120px, 260px)");
  expect(category).toMatch(/fr\)$/);
  expect(memo).toMatch(/fr\)$/);
  // Waiting for approval: a dot beside the row (in its first cell), not in the date's column.
  const waiting = screen.getByRole("button", { name: /^Approve / });
  expect(waiting.closest("[role=row]")!.querySelector("[role=cell]")!.contains(waiting)).toBe(true);
  expect(waiting.closest("[data-field=Date]")).toBeNull();
});

it("reads future-dated transactions quieter and says what Skip does", () => {
  const b = demoBudget();
  // The demo's rows are dated this month, so early in a month some are ahead of today.
  b.transactions = b.transactions.map((tx) => ({ ...tx, date: "2001-01-15" }));
  b.transactions[0] = { ...b.transactions[0], date: "2999-01-01", payee: "Later" };
  b.schedules = [
    {
      id: "gym",
      nextDate: "2999-02-01",
      anchorDate: "2999-02-01",
      frequency: "monthly",
      paused: false,
      transaction: { ...b.transactions[1], id: "template", payee: "Gym" },
    },
  ];
  setup("all", b);
  expect(screen.getByText("Later").closest("[role=row]")!.classList.contains("future")).toBe(true);
  expect(dataRows().filter((r) => r.classList.contains("future"))).toHaveLength(1);
  const upcoming = screen.getByText("Gym").closest("[role=row]") as HTMLElement;
  expect(within(upcoming).getByRole("button", { name: "Skip" }).getAttribute("title")).toMatch(/^Skip 2999-02-01/);
});

it("keeps schedules in the register: a heading folds them, and each has its menu", async () => {
  const b = demoBudget();
  b.schedules = [
    {
      id: "rent",
      nextDate: "2999-01-01",
      anchorDate: "2999-01-01",
      frequency: "monthly",
      paused: false,
      transaction: { ...b.transactions[0], id: "template", payee: "Landlord" },
    },
  ];
  const { budget, undo } = setup("checking", b);
  const heading = document.querySelector(".schedules-toggle") as HTMLElement;
  expect(heading.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(heading);
  expect(screen.queryByText("Landlord")).toBeNull();
  fireEvent.click(heading);
  const row = () => screen.getByText("Landlord").closest("[role=row]") as HTMLElement;
  const menu = () => fireEvent.click(within(row()).getByRole("button", { name: "Schedule options: Landlord" }));

  // Paused schedules stay listed, so they can be resumed.
  menu();
  fireEvent.click(screen.getByRole("menuitem", { name: "Pause" }));
  expect(budget().schedules![0].paused).toBe(true);
  expect(within(row()).getByText("Paused")).toBeTruthy();
  menu();
  fireEvent.click(screen.getByRole("menuitem", { name: "Resume" }));
  expect(budget().schedules![0].paused).toBe(false);

  menu();
  fireEvent.click(screen.getByRole("menuitem", { name: "Enter now" }));
  await waitFor(() => expect(api.previewSchedules).toHaveBeenCalledWith(expect.anything(), today(), "rent", false));

  // Edit opens the schedule's editor, from its menu or a double-click on the row.
  menu();
  fireEvent.click(screen.getByRole("menuitem", { name: "Edit schedule…" }));
  const editor = screen.getByRole("dialog", { name: "Scheduled transaction" });
  expect((within(editor).getByLabelText("Date") as HTMLInputElement).value).toBe("2999-01-01");
  fireEvent.click(within(editor).getByRole("button", { name: "Close dialog" }));
  fireEvent.doubleClick(within(row()).getAllByRole("cell")[3]);
  fireEvent.click(within(screen.getByRole("dialog", { name: "Scheduled transaction" })).getByRole("button", { name: "Close dialog" }));

  menu();
  fireEvent.click(screen.getByRole("menuitem", { name: "Delete schedule" }));
  expect(budget().schedules).toEqual([]);
  expect(document.querySelector(".schedules-toggle")).toBeNull();
  fireEvent.click(screen.getAllByRole("button", { name: "Undo" }).at(-1)!);
  expect(undo).toHaveBeenCalled();
});

it("schedules a repeating transaction from the row editor, entering nothing dated ahead", async () => {
  const { budget } = setup("checking");
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  const date = within(row).getByLabelText("Date");
  fireEvent.focus(date);
  fireEvent.change(date, { target: { value: "2999-01-15" } });
  fireEvent.change(within(row).getByRole("combobox", { name: "Payee" }), { target: { value: "Electr" } });
  fireEvent.click(screen.getByRole("option", { name: "Electricity" }));
  fireEvent.change(within(row).getByLabelText("Repeat"), { target: { value: "monthly" } });
  const outflow = within(row).getByLabelText("Outflow");
  fireEvent.change(outflow, { target: { value: "90" } });
  fireEvent.keyDown(outflow, { key: "Enter" });
  await screen.findByText("Scheduled monthly from 2999-01-15.");
  expect(api.upsertTransaction).not.toHaveBeenCalled();
  expect(budget().schedules).toEqual([
    expect.objectContaining({ frequency: "monthly", nextDate: "2999-01-15", anchorDate: "2999-01-15" }),
  ]);
  // It's listed with the other schedules, above the transactions.
  expect(screen.getByRole("button", { name: "Schedule options: Electricity" })).toBeTruthy();
});

it("saves a transaction dated ahead as a scheduled one, moving an existing one out of the register", async () => {
  const b = demoBudget();
  b.transactions[0].status = "uncleared";
  const t = b.transactions[0];
  // A new one dated ahead waits in Scheduled, once; nothing is entered.
  const fresh = await saveTransaction(b, { ...t, id: "new", date: "2999-01-01" });
  expect(api.upsertTransaction).not.toHaveBeenCalled();
  expect(fresh.transactions).toEqual(b.transactions);
  expect(fresh.schedules).toEqual([expect.objectContaining({ id: "new", frequency: "never", nextDate: "2999-01-01" })]);
  expect(scheduledMessage({ ...t, date: "2999-01-01" })).toBe("Scheduled for 2999-01-01. It's entered on that day.");
  // An existing one moved ahead leaves the register, and the repeating schedule it was entered
  // with (same ID) is left alone.
  const repeating = { id: t.id, nextDate: "2999-02-01", anchorDate: "2026-01-01", frequency: "monthly", paused: false, transaction: t };
  b.schedules = [repeating];
  const moved = await saveTransaction(b, { ...t, date: "2999-01-15" });
  expect(moved.transactions.some((x) => x.id === t.id)).toBe(false);
  expect(moved.schedules).toEqual([
    repeating,
    expect.objectContaining({ id: `${t.id}:ahead`, frequency: "never", nextDate: "2999-01-15" }),
  ]);
  // Today or earlier, it's simply saved; so is one the bank has cleared, whatever its date.
  await saveTransaction(b, { ...t, memo: "Changed" });
  expect(api.upsertTransaction).toHaveBeenCalledWith(b, expect.objectContaining({ memo: "Changed" }));
  expect(scheduledMessage(t)).toBe("");
  await saveTransaction(b, { ...t, status: "cleared", date: "2999-01-01" });
  expect(api.upsertTransaction).toHaveBeenLastCalledWith(b, expect.objectContaining({ status: "cleared", date: "2999-01-01" }));
});

it("enters a schedule saved with a date that's due, and keeps one dated ahead", async () => {
  const b = demoBudget();
  const s = { id: "s", nextDate: today(), anchorDate: today(), frequency: "never", paused: false, transaction: b.transactions[0] };
  await saveSchedule(b, s);
  expect(api.previewSchedules).toHaveBeenCalledWith(expect.objectContaining({ schedules: [s] }), today(), null, false);
  vi.mocked(api.previewSchedules).mockClear();
  const ahead = await saveSchedule(b, { ...s, nextDate: "2999-01-01" });
  expect(api.previewSchedules).not.toHaveBeenCalled();
  expect(ahead.schedules).toEqual([expect.objectContaining({ nextDate: "2999-01-01" })]);
});

it("retrying a moved transaction preserves its original repeating schedule", async () => {
  const b = demoBudget();
  const t = { ...b.transactions[0], status: "uncleared" as const };
  const repeating = { id: t.id, nextDate: "2999-02-01", anchorDate: "2026-01-01", frequency: "monthly", paused: false, transaction: t };
  b.schedules = [repeating];
  const moved = { ...t, date: "2999-01-15" };
  const first = await saveTransaction(b, moved);
  const retry = await saveTransaction(first, moved);
  expect(retry).toEqual(first);
  expect(retry.schedules![0]).toEqual(repeating);
});

it("editing a future import or generated entry keeps it in the ledger", async () => {
  const b = demoBudget();
  for (const details of [
    { hasImportSource: true },
    { id: "scheduled:rent:2999-01-01" },
    { id: "loan-interest:loan:2999-01" },
  ]) {
    const t = { ...b.transactions[0], date: "2999-01-01", status: "uncleared" as const, ...details };
    b.transactions = [t];
    const edited = { ...t, memo: "Updated note" };
    const saved = await saveTransaction(b, edited);
    expect(saved.transactions).toEqual([edited]);
    expect(saved.schedules ?? []).toHaveLength(0);
    expect(scheduledMessage(edited)).toBe("");
  }
});

it("schedules a transaction added with a later date, saying so before and after", async () => {
  const { budget } = setup("checking");
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  const date = within(row).getByLabelText("Date");
  fireEvent.focus(date);
  fireEvent.change(date, { target: { value: "2999-01-15" } });
  fireEvent.change(within(row).getByRole("combobox", { name: "Payee" }), { target: { value: "Electr" } });
  fireEvent.click(screen.getByRole("option", { name: "Electricity" }));
  fireEvent.change(within(row).getByLabelText("Outflow"), { target: { value: "90" } });
  expect(within(row).getByRole("option", { name: "Once" })).toBeTruthy();
  fireEvent.click(within(row).getByRole("button", { name: "Schedule" }));
  await screen.findByText("Scheduled for 2999-01-15. It's entered on that day.");
  expect(api.upsertTransaction).not.toHaveBeenCalled();
  expect(budget().schedules).toEqual([expect.objectContaining({ frequency: "never", nextDate: "2999-01-15" })]);
  const scheduled = screen.getByText("Electricity", { selector: "[role=row].upcoming *" }).closest("[role=row]") as HTMLElement;
  expect(within(scheduled).getByText("Once")).toBeTruthy();
});

it("lists every schedule from the Scheduled button, and adds and acts on them there", async () => {
  const b = demoBudget();
  b.schedules = [
    {
      id: "rent",
      nextDate: "2999-01-01",
      anchorDate: "2999-01-01",
      frequency: "monthly",
      paused: false,
      transaction: { ...b.transactions[0], id: "template", payee: "Landlord" },
    },
  ];
  const { budget } = setup("checking", b);
  fireEvent.click(document.querySelector(".schedules-button")!);
  const list = screen.getByRole("dialog", { name: "Scheduled · Everyday account" });
  expect(within(list).getByText("Landlord")).toBeTruthy();
  fireEvent.click(within(list).getByRole("button", { name: "Pause Landlord" }));
  expect(budget().schedules![0].paused).toBe(true);
  fireEvent.click(within(list).getByRole("button", { name: "Resume Landlord" }));
  expect(budget().schedules![0].paused).toBe(false);
  fireEvent.click(within(list).getByRole("button", { name: "Delete Landlord" }));
  expect(budget().schedules).toEqual([]);
  expect(within(list).getByText("Nothing scheduled")).toBeTruthy();

  // New: the editor opens over the list, in this account, and the new one is listed.
  fireEvent.click(within(list).getByRole("button", { name: "New scheduled transaction" }));
  const editor = screen.getByRole("dialog", { name: "New scheduled transaction" });
  fireEvent.click(within(editor).getByRole("button", { name: /Inflow/ }));
  fireEvent.change(within(editor).getByLabelText("Payee"), { target: { value: "Salary" } });
  fireEvent.change(within(editor).getByPlaceholderText("0.00"), { target: { value: "2500" } });
  fireEvent.click(within(editor).getByRole("button", { name: "Save schedule" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "New scheduled transaction" })).toBeNull());
  expect(budget().schedules).toEqual([
    expect.objectContaining({ frequency: "monthly", transaction: expect.objectContaining({ accountId: "checking", amount: 250000, payee: "Salary" }) }),
  ]);
  expect(within(screen.getByRole("dialog", { name: "Scheduled · Everyday account" })).getByText("Salary")).toBeTruthy();
  await screen.findByText(/^Scheduled monthly from /);
});

it("shows the cursor's bar only while the keyboard moves it", () => {
  setup("checking");
  const grid = dataRows()[0].closest(".register-scroll") as HTMLElement;
  fireEvent.pointerDown(grid);
  fireEvent.click(dataRows()[0]);
  expect(grid.classList.contains("keyed")).toBe(false);
  fireEvent.keyDown(grid, { key: "ArrowDown" });
  expect(grid.classList.contains("keyed")).toBe(true);
  expect(dataRows()[1].classList.contains("cursor")).toBe(true);
  // A click puts it away, selected or not.
  fireEvent.pointerDown(grid);
  expect(grid.classList.contains("keyed")).toBe(false);
});

it("says which spending needs a category, and lists just those", () => {
  const b = demoBudget();
  const t = b.transactions[0];
  b.transactions.push(
    { ...t, id: "mystery", payee: "Mystery shop", memo: "", date: today(), splits: [{ categoryId: null, amount: t.amount }] },
    { ...t, id: "chosen", payee: "From the pot", memo: "", date: today(), splits: [{ categoryId: null, amount: t.amount, unassigned: true }] },
  );
  setup("checking", b);
  const category = (payee: string) =>
    dataRows().find((r) => r.textContent?.includes(payee))!.querySelector("[data-field=Category]")!;
  expect(category("Mystery shop").textContent).toBe("Needs a category");
  expect(category("Mystery shop").className).toBe("needs-category");
  expect(category("From the pot").textContent).toBe("From Unassigned");
  cleanup();
  setup("checking", b, { initialFilter: { uncategorized: true } });
  expect(payees()).toEqual(["Mystery shop"]);
});

it("includes uncategorized spending posted after midnight without remounting the register", () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  try {
    vi.setSystemTime(new Date(2026, 9, 20, 23, 59));
    const b = demoBudget();
    b.transactions = [b.transactions[0]];
    const page = (budget: Budget) => <ToastProvider><Register budget={budget} accountId="checking"
      initialFilter={{ uncategorized: true }} edit={() => {}} transform={async () => {}} onEdit={() => {}} /></ToastProvider>;
    const view = render(page(b));
    expect(screen.getByText("No transactions match these filters.")).toBeTruthy();
    vi.setSystemTime(new Date(2026, 9, 21, 0, 1));
    const posted = { ...b.transactions[0], id: "posted", date: today(), payee: "Posted overnight", memo: "",
      amount: -5000, splits: [{ categoryId: null, amount: -5000 }] };
    // Posting replaces the transactions array, but accounts/categories retain their identity.
    view.rerender(page({ ...b, transactions: [...b.transactions, posted] }));
    expect(payees()).toEqual(["Posted overnight"]);
    expect(screen.getByText("Needs a category", { selector: ".needs-pill" })).toBeTruthy();
  } finally {
    vi.useRealTimers();
  }
});

it("doesn't offer Unassigned for spending, as YNAB steers it to a category", () => {
  setup("checking");
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  const category = within(row).getByRole("combobox", { name: "Category" });
  category.focus();
  fireEvent.change(category, { target: { value: "Unassigned" } });
  expect(screen.queryByRole("option", { name: /^(To|From|Take from) Unassigned/ })).toBeNull();
  // Income goes to it.
  fireEvent.change(within(row).getByLabelText("Inflow"), { target: { value: "100" } });
  category.focus();
  fireEvent.change(category, { target: { value: "Unassigned" } });
  expect(screen.getByRole("option", { name: /^To Unassigned/ })).toBeTruthy();
});

it("keeps spending already taken from Unassigned as that when edited", async () => {
  const b = demoBudget();
  b.transactions[0].splits = [{ categoryId: null, amount: b.transactions[0].amount, unassigned: true }];
  setup("checking", b);
  const home = dataRows().find((r) => r.textContent?.includes("Home"))!;
  fireEvent.doubleClick(home.querySelector("[data-field=Memo]")!);
  const row = screen.getByRole("row", { name: "Edit transaction" });
  expect((within(row).getByRole("combobox", { name: "Category" }) as HTMLInputElement).value).toBe("From Unassigned");
  const memo = within(row).getByLabelText("Memo");
  fireEvent.change(memo, { target: { value: "Paid back" } });
  fireEvent.keyDown(memo, { key: "Enter" });
  await waitFor(() => expect(screen.queryByRole("row", { name: "Edit transaction" })).toBeNull());
  const saved = vi.mocked(api.upsertTransaction).mock.calls.at(-1)![1] as Transaction;
  expect(saved.splits).toEqual([expect.objectContaining({ categoryId: null, unassigned: true })]);
});

it("edits spending that needs a category without making it choose one", async () => {
  const b = demoBudget();
  b.transactions[0].splits = [{ categoryId: null, amount: b.transactions[0].amount }];
  setup("checking", b);
  const home = dataRows().find((r) => r.textContent?.includes("Home"))!;
  fireEvent.doubleClick(home.querySelector("[data-field=Memo]")!);
  const row = screen.getByRole("row", { name: "Edit transaction" });
  expect((within(row).getByRole("combobox", { name: "Category" }) as HTMLInputElement).value).toBe("");
  const memo = within(row).getByLabelText("Memo");
  fireEvent.change(memo, { target: { value: "Still to sort out" } });
  fireEvent.keyDown(memo, { key: "Enter" });
  await waitFor(() => expect(screen.queryByRole("row", { name: "Edit transaction" })).toBeNull());
  const saved = vi.mocked(api.upsertTransaction).mock.calls.at(-1)![1] as Transaction;
  expect(saved.splits).toEqual([expect.objectContaining({ categoryId: null })]);
  expect(saved.splits[0].unassigned).toBe(false);
});
