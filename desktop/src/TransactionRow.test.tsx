import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { TransactionEditor } from "./TransactionRow";
import { demoBudget } from "./demo";
import type { Transaction } from "./types";
import { setPref } from "./prefs";
import { editableAmount } from "./format";

afterEach(cleanup);

function editCategorizedTransaction() {
  const budget = demoBudget();
  const initial = {
    ...budget.transactions[0], accountId: "checking", amount: -2500,
    splits: [{ categoryId: "rent", amount: -2500 }],
  };
  const onSave = vi.fn(async (_transaction: Transaction) => {});
  const onCancel = vi.fn();
  render(<TransactionEditor budget={budget} accountId="checking" initial={initial}
    onSave={onSave} onCancel={onCancel} onMore={() => {}} />);
  const category = screen.getByRole("combobox", { name: "Category" });
  expect(category).toHaveProperty("value", "Rent");
  fireEvent.focus(category);
  fireEvent.change(category, { target: { value: "" } });
  return { category, onSave, onCancel };
}

it("commits a cleared category with Enter before the next Enter saves the transaction", async () => {
  const { category, onSave } = editCategorizedTransaction();
  fireEvent.keyDown(category, { key: "Enter" });
  expect(onSave).not.toHaveBeenCalled();
  expect(category).toHaveProperty("value", "");
  expect(screen.queryByRole("listbox", { name: "Category" })).toBeNull();
  fireEvent.keyDown(category, { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  expect(onSave.mock.calls[0][0].splits).toEqual([expect.objectContaining({ categoryId: null, amount: -2500 })]);
});

it("saves a cleared category when clicking Save closes the dropdown before blur", async () => {
  const { category, onSave } = editCategorizedTransaction();
  const save = screen.getByRole("button", { name: "Save" });
  fireEvent.mouseDown(save);
  fireEvent.blur(category);
  fireEvent.mouseUp(save);
  fireEvent.click(save);
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  expect(onSave.mock.calls[0][0].splits).toEqual([expect.objectContaining({ categoryId: null, amount: -2500 })]);
});

it("keeps a category cleared after tabbing to another field", async () => {
  const { category, onSave } = editCategorizedTransaction();
  fireEvent.keyDown(category, { key: "Tab" });
  fireEvent.blur(category);
  expect(category).toHaveProperty("value", "");
  fireEvent.keyDown(screen.getByLabelText("Memo"), { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  expect(onSave.mock.calls[0][0].splits).toEqual([expect.objectContaining({ categoryId: null, amount: -2500 })]);
});

it("Escape cancels a pending category clearing without closing the transaction editor", async () => {
  const { category, onSave, onCancel } = editCategorizedTransaction();
  fireEvent.keyDown(category, { key: "Escape" });
  expect(category).toHaveProperty("value", "Rent");
  expect(onCancel).not.toHaveBeenCalled();
  fireEvent.blur(category);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  expect(onSave.mock.calls[0][0].splits).toEqual([{ categoryId: "rent", amount: -2500 }]);
});

it("chooses another category after emptying the field without clearing the new choice", async () => {
  const { category, onSave } = editCategorizedTransaction();
  const groceries = screen.getByRole("option", { name: "Groceries" });
  fireEvent.mouseDown(groceries);
  fireEvent.mouseUp(groceries);
  fireEvent.click(groceries);
  expect(category).toHaveProperty("value", "Groceries");
  fireEvent.blur(category);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  expect(onSave.mock.calls[0][0].splits).toEqual([{ categoryId: "groceries", amount: -2500 }]);
});

it.each(["Enter", "Tab"])("selects a category with arrow keys and %s after emptying the field", async (key) => {
  const { category, onSave } = editCategorizedTransaction();
  fireEvent.keyDown(category, { key: "ArrowDown" });
  fireEvent.keyDown(category, { key: "ArrowDown" });
  expect(screen.getByRole("option", { name: "Groceries" }).className).toContain("active");
  fireEvent.keyDown(category, { key });
  if (key === "Tab") fireEvent.blur(category);
  expect(category).toHaveProperty("value", "Groceries");
  expect(onSave).not.toHaveBeenCalled();
  fireEvent.keyDown(screen.getByLabelText("Memo"), { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  expect(onSave.mock.calls[0][0].splits[0].categoryId).toBe("groceries");
});

it("keeps the next future-dated row uncleared after Schedule & add another", async () => {
  setPref("newCleared", true);
  try {
    const onSave = vi.fn(async (_transaction: Transaction) => {});
    render(<TransactionEditor budget={demoBudget()} accountId="checking"
      onSave={onSave} onCancel={() => {}} onMore={() => {}} />);
    const date = screen.getByLabelText("Date");
    fireEvent.focus(date);
    fireEvent.change(date, { target: { value: "2999-01-15" } });
    fireEvent.blur(date);
    fireEvent.change(screen.getByLabelText("Inflow"), { target: { value: "25" } });
    fireEvent.keyDown(screen.getByLabelText("Inflow"), { key: "Enter", shiftKey: true });
    await waitFor(() => expect(screen.getByLabelText("Inflow")).toHaveProperty("value", ""));
    fireEvent.change(screen.getByLabelText("Inflow"), { target: { value: "30" } });
    fireEvent.keyDown(screen.getByLabelText("Inflow"), { key: "Enter" });
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
    expect(onSave.mock.calls[1][0]).toMatchObject({ date: "2999-01-15", status: "uncleared" });
  } finally {
    setPref("newCleared", false);
  }
});

it("blocks an invalid date in the inline editor for both Save and Enter", async () => {
  const budget = demoBudget();
  const transaction = budget.transactions[0];
  const onSave = vi.fn(async () => {});
  render(<TransactionEditor budget={budget} accountId={transaction.accountId} initial={transaction}
    onSave={onSave} onCancel={() => {}} onMore={() => {}} />);
  const date = screen.getByLabelText("Date");
  fireEvent.focus(date);
  fireEvent.change(date, { target: { value: "2026-02-31" } });
  fireEvent.blur(date);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  fireEvent.keyDown(date, { key: "Enter" });
  expect(onSave).not.toHaveBeenCalled();
  expect(date).toHaveProperty("value", "2026-02-31");
  fireEvent.focus(date);
  fireEvent.change(date, { target: { value: "2026-02-28" } });
  fireEvent.keyDown(date, { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ date: "2026-02-28" })));
});

it("keeps the new row's ID on retry after a failed save", async () => {
  const onSave = vi.fn().mockRejectedValueOnce(new Error("Disk unavailable")).mockResolvedValue(undefined);
  render(<TransactionEditor budget={demoBudget()} accountId="checking"
    onSave={onSave} onCancel={() => {}} onMore={() => {}} />);
  fireEvent.change(screen.getByLabelText("Inflow"), { target: { value: "25" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Disk unavailable");
  const firstId = onSave.mock.calls[0][0].id;
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
  expect(onSave.mock.calls[1][0].id).toBe(firstId);
});

it("starts a distinct transaction after a successful Save & add another", async () => {
  const onSave = vi.fn(async (_transaction: Transaction) => {});
  const onCancel = vi.fn();
  render(<TransactionEditor budget={demoBudget()} accountId="checking"
    onSave={onSave} onCancel={onCancel} onMore={() => {}} />);
  fireEvent.change(screen.getByLabelText("Inflow"), { target: { value: "25" } });
  fireEvent.click(screen.getByRole("button", { name: "Save & add another" }));
  await waitFor(() => expect(screen.getByLabelText("Inflow")).toHaveProperty("value", ""));
  expect(onCancel).not.toHaveBeenCalled();
  const firstId = onSave.mock.calls[0][0].id;
  fireEvent.change(screen.getByLabelText("Inflow"), { target: { value: "30" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
  expect(onSave.mock.calls[1][0].id).not.toBe(firstId);
});

/** A budget whose payees have habits: Netflix a bill, Employer pay coming in, the hardware store
 * spending with a memo each time. */
function habitBudget() {
  const budget = demoBudget();
  const t = (n: number, payee: string, amount: number, categoryId: string | null, memo = "", extra = {}): Transaction => ({
    id: `h${n}`, date: `2026-09-${String(n).padStart(2, "0")}`, accountId: "checking", payee, memo, amount,
    status: "cleared", splits: [{ categoryId, amount }], transferAccountId: null, transferStatus: null, ...extra,
  });
  budget.transactions = [
    t(1, "Netflix", -1599, "bills"),
    t(2, "Netflix", -1799, "bills"),
    t(3, "Employer", 250000, null),
    t(4, "Hardware store", -4500, "gifts", "Paint"),
    t(5, "Hardware store", -1200, "gifts", "Nails"),
    t(6, "Netflix", -1899, "bills", "NETFLIX.COM 866-579", { hasImportSource: true }),
    t(7, "Lidl", -2340, "groceries"),
  ];
  return budget;
}
function newRow(budget = habitBudget()) {
  const onSave = vi.fn(async (_transaction: Transaction) => {});
  const onCancel = vi.fn();
  render(<TransactionEditor budget={budget} accountId="checking" onSave={onSave} onCancel={onCancel} onMore={() => {}} />);
  return { onSave, onCancel, payee: screen.getByRole("combobox", { name: "Payee" }) };
}
const focused = (el: Element) => waitFor(() => expect(document.activeElement).toBe(el));
const tab = (el: Element) => fireEvent.keyDown(el, { key: "Tab" });

it("tabs past the category the payee filled in and the memo, to its suggested amount and on", async () => {
  const { payee, onSave } = newRow();
  fireEvent.change(payee, { target: { value: "Netf" } });
  tab(payee);
  const outflow = screen.getByLabelText("Outflow");
  await focused(outflow);
  expect(screen.getByRole("combobox", { name: "Category" })).toHaveProperty("value", "Bills & utilities");
  // The latest of its own, typed or imported.
  expect(outflow).toHaveProperty("placeholder", editableAmount(1899));
  tab(outflow);
  await focused(screen.getByLabelText("Cleared"));
  expect(outflow).toHaveProperty("value", editableAmount(1899));
  tab(screen.getByLabelText("Cleared"));
  await focused(screen.getByRole("button", { name: "Save & add another" }));
  fireEvent.keyDown(screen.getByLabelText("Cleared"), { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  expect(onSave.mock.calls[0][0]).toMatchObject({ payee: "Netflix", amount: -1899, splits: [{ categoryId: "bills", amount: -1899 }] });
});

it("tabs to Inflow for a payee that pays in, and to Memo for one that usually has one", async () => {
  const { payee } = newRow();
  fireEvent.change(payee, { target: { value: "Employer" } });
  tab(payee);
  const inflow = screen.getByLabelText("Inflow");
  await focused(inflow);
  expect(inflow).toHaveProperty("placeholder", editableAmount(250000));
  cleanup();
  const row = newRow();
  fireEvent.change(row.payee, { target: { value: "Hardware" } });
  tab(row.payee);
  await focused(screen.getByLabelText("Memo"));
  // Its memos stop Tab only while the memo is empty.
  fireEvent.change(screen.getByLabelText("Memo"), { target: { value: "Paint" } });
  tab(row.payee);
  await focused(screen.getByLabelText("Outflow"));
});

it("fills in a payee's latest category but stops there when it has had others lately", async () => {
  const budget = habitBudget();
  const shop = (n: number, categoryId: string | null, extra = {}) =>
    ({ ...budget.transactions[6], id: `a${n}`, date: `2026-10-${String(n).padStart(2, "0")}`, payee: "Amazon",
      splits: [{ categoryId, amount: -2340 }], ...extra }) as Transaction;
  // Groceries most of the time, gifts once lately; one imported and not yet categorized doesn't count.
  budget.transactions.push(shop(1, "groceries"), shop(2, "groceries"), shop(3, "gifts"), shop(4, "groceries"),
    shop(5, null, { hasImportSource: true }));
  const { payee } = newRow(budget);
  fireEvent.change(payee, { target: { value: "Amaz" } });
  tab(payee);
  const category = screen.getByRole("combobox", { name: "Category" });
  await focused(category);
  expect(category).toHaveProperty("value", "Groceries");
  tab(category);
  await focused(screen.getByLabelText("Outflow"));
  cleanup();
  // With the gifts more than 10 of its categorized transactions ago, it's one category again.
  for (let n = 6; n <= 15; n++) budget.transactions.push(shop(n, "groceries"));
  const row = newRow(budget);
  fireEvent.change(row.payee, { target: { value: "Amaz" } });
  tab(row.payee);
  await focused(screen.getByLabelText("Outflow"));
});

it("takes a payee's default category as read, whatever its transactions had", async () => {
  const budget = habitBudget();
  budget.transactions.push({ ...budget.transactions[0], id: "odd", date: "2026-09-30", splits: [{ categoryId: "gifts", amount: -1599 }] });
  budget.payees = [{ id: "p", name: "Netflix", categoryId: "bills" }];
  const { payee } = newRow(budget);
  fireEvent.change(payee, { target: { value: "Netf" } });
  tab(payee);
  await focused(screen.getByLabelText("Outflow"));
  expect(screen.getByRole("combobox", { name: "Category" })).toHaveProperty("value", "Bills & utilities");
});

it("stops at the category for a new payee, then goes past the memo", async () => {
  const { payee } = newRow();
  fireEvent.change(payee, { target: { value: "A new shop" } });
  tab(payee);
  const category = screen.getByRole("combobox", { name: "Category" });
  await focused(category);
  fireEvent.change(category, { target: { value: "Groc" } });
  tab(category);
  const outflow = screen.getByLabelText("Outflow");
  await focused(outflow);
  expect(outflow).toHaveProperty("placeholder", "Outflow");
  // Tab from an empty Outflow goes on to Inflow.
  tab(outflow);
  await focused(screen.getByLabelText("Inflow"));
});

it("fills in the suggested amount with Enter, and saves it with the next", async () => {
  const { payee, onSave } = newRow();
  fireEvent.change(payee, { target: { value: "Lidl" } });
  tab(payee);
  const outflow = screen.getByLabelText("Outflow");
  await focused(outflow);
  fireEvent.keyDown(outflow, { key: "Enter" });
  expect(onSave).not.toHaveBeenCalled();
  expect(outflow).toHaveProperty("value", editableAmount(2340));
  fireEvent.keyDown(outflow, { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ payee: "Lidl", amount: -2340 })));
});

it("drops the suggestion once an amount is typed, and → takes it", async () => {
  const { payee } = newRow();
  fireEvent.change(payee, { target: { value: "Lidl" } });
  tab(payee);
  const outflow = screen.getByLabelText("Outflow");
  await focused(outflow);
  fireEvent.keyDown(outflow, { key: "ArrowRight" });
  expect(outflow).toHaveProperty("value", editableAmount(2340));
  fireEvent.change(outflow, { target: { value: "" } });
  expect(outflow).toHaveProperty("placeholder", "Outflow");
});

it("leaves a longer expression with a sign where it adds up as typed", () => {
  const { onSave } = newRow();
  const outflow = screen.getByLabelText("Outflow");
  const inflow = screen.getByLabelText("Inflow");
  // -5+10 is 5 in: moved to Outflow as 5+10 it would be 15 out.
  fireEvent.change(inflow, { target: { value: "-5+10" } });
  expect(inflow).toHaveProperty("value", "-5+10");
  expect(outflow).toHaveProperty("value", "");
  fireEvent.keyDown(inflow, { key: "Enter" });
  return waitFor(() => expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ amount: 500 })));
});

it("moves money typed with + into Inflow, and with - into Outflow", async () => {
  newRow();
  const outflow = screen.getByLabelText("Outflow");
  const inflow = screen.getByLabelText("Inflow");
  fireEvent.change(outflow, { target: { value: "+25" } });
  expect(outflow).toHaveProperty("value", "");
  expect(inflow).toHaveProperty("value", "25");
  await focused(inflow);
  fireEvent.change(inflow, { target: { value: "-30" } });
  expect(inflow).toHaveProperty("value", "");
  expect(outflow).toHaveProperty("value", "30");
  await focused(outflow);
});

it("clears with Alt+C from any field, and with C on the mark", async () => {
  const { payee, onSave } = newRow();
  const mark = screen.getByLabelText("Cleared");
  fireEvent.keyDown(payee, { key: "c", code: "KeyC", altKey: true });
  expect(mark.getAttribute("aria-pressed")).toBe("true");
  fireEvent.keyDown(mark, { key: "c", code: "KeyC" });
  expect(mark.getAttribute("aria-pressed")).toBe("false");
  // A layout with no c on that key (Arabic writes ؤ) still has the key; one with c elsewhere (Dvorak) its c.
  fireEvent.keyDown(payee, { key: "ؤ", code: "KeyC", altKey: true });
  fireEvent.keyDown(payee, { key: "c", code: "KeyI", altKey: true });
  fireEvent.keyDown(payee, { key: "c", code: "KeyI", altKey: true });
  // Not with Ctrl too: AltGr, which is Ctrl+Alt, writes letters on some layouts.
  fireEvent.keyDown(payee, { key: "ć", code: "KeyC", altKey: true, ctrlKey: true });
  fireEvent.change(screen.getByLabelText("Outflow"), { target: { value: "5" } });
  fireEvent.keyDown(mark, { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ status: "cleared" })));
});

it("leaves Enter on the row's buttons to the button", () => {
  const { onSave } = newRow();
  fireEvent.change(screen.getByLabelText("Outflow"), { target: { value: "12" } });
  for (const name of ["Cancel", "Save & add another", "Split", "More options"])
    expect(fireEvent.keyDown(screen.getByRole("button", { name: new RegExp(name) }), { key: "Enter" })).toBe(true);
  expect(onSave).not.toHaveBeenCalled();
});

it("keeps Tab as it is when editing a transaction", () => {
  const budget = habitBudget();
  render(<TransactionEditor budget={budget} accountId="checking" initial={budget.transactions[0]}
    onSave={async () => {}} onCancel={() => {}} onMore={() => {}} />);
  expect(tab(screen.getByRole("combobox", { name: "Payee" }))).toBe(true);
});


it("moves between fields with ← and → once the caret is at the end of the text", async () => {
  const { payee } = newRow();
  const memo = screen.getByLabelText<HTMLInputElement>("Memo");
  fireEvent.change(memo, { target: { value: "Paint" } });
  memo.focus();
  memo.setSelectionRange(2, 2);
  // Inside the text, the caret moves.
  expect(fireEvent.keyDown(memo, { key: "ArrowRight" })).toBe(true);
  expect(fireEvent.keyDown(memo, { key: "ArrowLeft" })).toBe(true);
  memo.setSelectionRange(5, 5);
  fireEvent.keyDown(memo, { key: "ArrowRight" });
  const outflow = screen.getByLabelText("Outflow");
  expect(document.activeElement).toBe(outflow);
  fireEvent.keyDown(outflow, { key: "ArrowRight" });
  fireEvent.keyDown(screen.getByLabelText("Inflow"), { key: "ArrowRight" });
  expect(document.activeElement).toBe(screen.getByLabelText("Cleared"));
  fireEvent.keyDown(screen.getByLabelText("Cleared"), { key: "ArrowLeft" });
  expect(document.activeElement).toBe(screen.getByLabelText("Inflow"));
  // Arriving selects the field, so the next arrow goes on: back to the memo, the category, the payee.
  fireEvent.keyDown(screen.getByLabelText("Inflow"), { key: "ArrowLeft" });
  fireEvent.keyDown(outflow, { key: "ArrowLeft" });
  expect(document.activeElement).toBe(memo);
  expect([memo.selectionStart, memo.selectionEnd]).toEqual([0, 5]);
  fireEvent.keyDown(memo, { key: "ArrowLeft" });
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Category" }), { key: "ArrowLeft" });
  expect(document.activeElement).toBe(payee);
});

it("leaves ← and → to an open list of payees", () => {
  const { payee } = newRow();
  fireEvent.change(payee, { target: { value: "Net" } });
  expect(payee.getAttribute("aria-expanded")).toBe("true");
  expect(fireEvent.keyDown(payee, { key: "ArrowRight" })).toBe(true);
});

it("fills in Unassigned for a payee whose pay is left to it, and goes past it", async () => {
  const budget = habitBudget();
  budget.transactions[2] = { ...budget.transactions[2], splits: [{ categoryId: null, amount: 250000, unassigned: true }] };
  const { payee, onSave } = newRow(budget);
  fireEvent.change(payee, { target: { value: "Employer" } });
  tab(payee);
  const inflow = screen.getByLabelText("Inflow");
  await focused(inflow);
  // Its pay goes to Unassigned, before its amount is in too.
  expect(screen.getByRole("combobox", { name: "Category" })).toHaveProperty("value", "To Unassigned");
  tab(inflow);
  expect(screen.getByRole("combobox", { name: "Category" })).toHaveProperty("value", "To Unassigned");
  fireEvent.keyDown(inflow, { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalled());
  expect(onSave.mock.calls[0][0].splits).toEqual([expect.objectContaining({ categoryId: null, unassigned: true, amount: 250000 })]);
});

it("moves through Account with ← and → when entering in all accounts, without changing it", () => {
  render(<TransactionEditor budget={habitBudget()} accountId="all" onSave={async () => {}} onCancel={() => {}} onMore={() => {}} />);
  const payee = screen.getByRole("combobox", { name: "Payee" });
  const account = screen.getByLabelText<HTMLSelectElement>("Account");
  payee.focus();
  fireEvent.keyDown(payee, { key: "ArrowRight" });
  expect(document.activeElement).toBe(account);
  expect(fireEvent.keyDown(account, { key: "ArrowRight" })).toBe(false);
  expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "Category" }));
  fireEvent.keyDown(document.activeElement!, { key: "ArrowLeft" });
  expect(document.activeElement).toBe(account);
  // Kept from the select, so it doesn't change the account on its way.
  expect(fireEvent.keyDown(account, { key: "ArrowLeft" })).toBe(false);
  expect(document.activeElement).toBe(payee);
});

it("moves the other way with ← and → right to left", () => {
  render(
    <div dir="rtl">
      <TransactionEditor budget={habitBudget()} accountId="checking" onSave={async () => {}} onCancel={() => {}} onMore={() => {}} />
    </div>,
  );
  const payee = screen.getByRole("combobox", { name: "Payee" });
  payee.focus();
  fireEvent.keyDown(payee, { key: "ArrowLeft" });
  const category = screen.getByRole("combobox", { name: "Category" });
  expect(document.activeElement).toBe(category);
  fireEvent.keyDown(category, { key: "ArrowRight" });
  expect(document.activeElement).toBe(payee);
});

it("goes by the fields themselves, not their labels, which another language translates", async () => {
  const { payee } = newRow();
  // As the Arabic window labels them.
  for (const [label, arabic] of [["Payee", "المستفيد"], ["Category", "الفئة"], ["Memo", "ملاحظة"], ["Outflow", "مصروف"], ["Cleared", "مُسوّى"]])
    document.querySelector(`.reg-editor [aria-label="${label}"]`)!.setAttribute("aria-label", arabic);
  fireEvent.change(payee, { target: { value: "Netf" } });
  tab(payee);
  const outflow = document.querySelector<HTMLInputElement>('[data-field="Outflow"] input')!;
  await focused(outflow);
  tab(outflow);
  await focused(document.querySelector(".clear-button")!);
});

it("leaves keys to an input method while it's composing", () => {
  const { onSave } = newRow();
  const outflow = screen.getByLabelText("Outflow");
  fireEvent.change(outflow, { target: { value: "12" } });
  expect(fireEvent.keyDown(outflow, { key: "Enter", isComposing: true })).toBe(true);
  expect(fireEvent.keyDown(outflow, { key: "Process", keyCode: 229 })).toBe(true);
  expect(onSave).not.toHaveBeenCalled();
});

it("switches field as soon as a sign is typed first, key by key", () => {
  newRow();
  const outflow = screen.getByLabelText("Outflow");
  const inflow = screen.getByLabelText("Inflow");
  fireEvent.change(inflow, { target: { value: "-" } });
  expect(document.activeElement).toBe(outflow);
  fireEvent.change(outflow, { target: { value: "5+10" } });
  expect([outflow, inflow].map((el) => (el as HTMLInputElement).value)).toEqual(["5+10", ""]);
});

it("starts the next one at the payee with the same date after Save & add another", async () => {
  const { onSave } = newRow();
  const date = screen.getByLabelText<HTMLInputElement>("Date");
  fireEvent.focus(date);
  fireEvent.change(date, { target: { value: "2026-09-15" } });
  fireEvent.blur(date);
  const outflow = screen.getByLabelText("Outflow");
  fireEvent.change(outflow, { target: { value: "10" } });
  fireEvent.keyDown(outflow, { key: "Enter", shiftKey: true });
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  await focused(screen.getByRole("combobox", { name: "Payee" }));
  fireEvent.change(outflow, { target: { value: "20" } });
  fireEvent.keyDown(outflow, { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
  expect(onSave.mock.calls.map(([t]) => t.date)).toEqual(["2026-09-15", "2026-09-15"]);
});

it("saves the next pay to Unassigned, not an older expense's category (Codex review R4)", async () => {
  const budget = habitBudget();
  const pay = budget.transactions[2];
  budget.transactions.push({ ...pay, id: "gift", date: "2026-08-01", amount: -5000, splits: [{ categoryId: "gifts", amount: -5000 }] },
    { ...pay, id: "pay2", date: "2026-09-10" });
  const { payee, onSave } = newRow(budget);
  fireEvent.change(payee, { target: { value: "Employer" } });
  tab(payee);
  // Its pay, with a gift once among its categories: filled in, and checked.
  const category = screen.getByRole("combobox", { name: "Category" });
  await focused(category);
  tab(category);
  const inflow = screen.getByLabelText("Inflow");
  await focused(inflow);
  fireEvent.change(inflow, { target: { value: "2500" } });
  expect(category).toHaveProperty("value", "To Unassigned");
  fireEvent.keyDown(inflow, { key: "Enter" });
  await waitFor(() => expect(onSave).toHaveBeenCalled());
  expect(onSave.mock.calls[0][0].splits).toEqual([expect.objectContaining({ categoryId: null, amount: 250000 })]);
});

it("keeps adding with Enter once Save & add another has been used, until Escape", async () => {
  const { onSave, onCancel } = newRow();
  const outflow = screen.getByLabelText("Outflow");
  fireEvent.change(outflow, { target: { value: "10" } });
  fireEvent.keyDown(outflow, { key: "Enter", shiftKey: true });
  await waitFor(() => expect(screen.getByText("1 added")).toBeTruthy());
  expect(screen.getByRole("button", { name: "Save & add another" }).className).toContain("primary");
  expect(screen.getByRole("button", { name: "Save" }).className).toContain("secondary");
  fireEvent.change(outflow, { target: { value: "20" } });
  fireEvent.keyDown(outflow, { key: "Enter" });
  await waitFor(() => expect(screen.getByText("2 added")).toBeTruthy());
  expect(onSave).toHaveBeenCalledTimes(2);
  expect(onCancel).not.toHaveBeenCalled();
  fireEvent.keyDown(outflow, { key: "Escape" });
  expect(onCancel).toHaveBeenCalled();
});
