import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { produce } from "immer";
import { afterEach, expect, it, vi } from "vitest";
import { Sidebar, type Page } from "./Sidebar";
import { newBudget } from "./newBudget";
import { templatesFixture } from "./budgetTemplates.fixture";
import { defaultNumberFormat } from "./format";
import { demoBudget } from "./demo";
import type { Budget, Summary } from "./types";

afterEach(cleanup);

const layout = {
  width: 236,
  expandedWidth: 236,
  collapsed: false,
  setWidth: vi.fn(),
  toggle: vi.fn(),
};
function renderSidebar(budget: Budget) {
  let current = budget;
  const edit = vi.fn((update: (d: Budget) => void) => {
    current = produce(current, update);
  });
  render(
    <Sidebar
      budget={budget}
      budgetId="test"
      balances={new Map()}
      page={"budget" as Page}
      accountId="all"
      layout={layout}
      edit={edit}
      onNavigate={vi.fn()}
      onAccount={vi.fn()}
      onAddAccount={vi.fn()}
      onEditAccount={vi.fn()}
      onReconcile={vi.fn()}
      onSwitchBudget={vi.fn()}
      onNewBudget={vi.fn()}
      onManageBudgets={vi.fn()}
    />,
  );
  return { edit, result: () => current };
}

it("moves an account down within its section from the account menu", () => {
  const b = demoBudget();
  const [first, second] = b.accounts.filter((a) => a.kind !== "tracking" && !a.archived);
  const { result } = renderSidebar(b);
  fireEvent.click(screen.getByRole("button", { name: `Actions for ${first.name}` }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Move down" }));
  const order = result().accounts.map((a) => a.id);
  expect(order.indexOf(first.id)).toBe(order.indexOf(second.id) + 1);
});

it("closes an account from the menu and offers undo", () => {
  const b = demoBudget();
  const account = b.accounts[0];
  const { result } = renderSidebar(b);
  fireEvent.contextMenu(screen.getByRole("button", { name: new RegExp(`^${account.name}`) }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Close account" }));
  expect(result().accounts[0].archived).toBe(true);
});

it("marks a bank-synced account after its name, keeping the balance in line", () => {
  const b = produce(demoBudget(), (d) => {
    d.accounts[0].simplefin = { id: "A", name: "Everyday Checking", org: "First Bank", from: "2026-09-01", lastSync: "2026-10-18" };
  });
  const [linked, plain] = b.accounts;
  const onAccount = vi.fn();
  const onSyncDetails = vi.fn();
  render(
    <Sidebar
      budget={b}
      budgetId="test"
      balances={new Map()}
      page={"budget" as Page}
      accountId="all"
      layout={layout}
      edit={vi.fn()}
      onNavigate={vi.fn()}
      onAccount={onAccount}
      onAddAccount={vi.fn()}
      onEditAccount={vi.fn()}
      onReconcile={vi.fn()}
      onSync={vi.fn()}
      onSyncDetails={onSyncDetails}
      onSwitchBudget={vi.fn()}
      onNewBudget={vi.fn()}
      onManageBudgets={vi.fn()}
    />,
  );
  const row = screen.getByRole("button", { name: new RegExp(`^${linked.name}`) });
  // The mark sits with the name, so the balance is still the row's last item, as on other rows.
  const mark = row.querySelector(".account-name .sync-mark")!;
  expect(mark.getAttribute("title")).toContain("Syncs with Everyday Checking at First Bank");
  expect(row.lastElementChild?.className).toContain("num");
  expect(screen.getByRole("button", { name: new RegExp(`^${plain.name}`) }).querySelector(".sync-mark")).toBeNull();
  // Only one control per row besides the "…" menu.
  expect(row.parentElement!.querySelectorAll("button")).toHaveLength(2);
  fireEvent.click(mark);
  expect(onSyncDetails).toHaveBeenCalledWith(expect.objectContaining({ id: linked.id }));
  expect(onAccount).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: `Actions for ${linked.name}` }));
  expect(screen.getByRole("menuitem", { name: "Sync with bank" })).toBeTruthy();
  expect(screen.getByRole("menuitem", { name: "Bank sync details…" })).toBeTruthy();
});

it("new budgets start empty, from a template, or by starting over", () => {
  const options = { name: " Travel ", currency: "EUR", numberFormat: defaultNumberFormat, dateFormat: "dmy" as const };
  const blank = newBudget({ ...options, start: "blank" });
  expect(blank).toMatchObject({ name: "Travel", currency: "EUR", dateFormat: "dmy", accounts: [], categories: [] });
  const [everyday, student, empty] = templatesFixture();
  const starter = newBudget({ ...options, start: "template", template: everyday });
  expect(starter.groupOrder).toEqual(["Bills", "Needs", "Wants", "Savings"]);
  expect(starter.categories).toHaveLength(16);
  expect(newBudget({ ...options, start: "template", template: student }).groupOrder).toEqual(["School", "Living", "Fun", "Savings"]);
  expect(newBudget({ ...options, start: "template", template: empty })).toMatchObject({ categories: [] });

  const old = demoBudget();
  old.accounts[0].source = { id: "ynab" };
  old.transactions = [{ id: "settled", accountId: old.accounts[0].id, date: "2026-01-01", amount: 500,
    payee: "Opening income", memo: "", status: "cleared", transferAccountId: null, transferStatus: null,
    splits: [{ categoryId: null, amount: 500 }] }];
  const summary = {
    accounts: old.accounts.map((a, i) => ({ id: a.id, balance: 1000 * (i + 1), cleared: 0, uncleared: 0 })),
  } as unknown as Summary;
  const copy = newBudget({ ...options, currency: old.currency, start: "copy" }, { budget: old, summary }, "2026-10-19");
  expect(copy.transactions).toEqual([]);
  expect(copy.assignments).toEqual([]);
  expect(copy.accounts.map((a) => a.openingBalance)).toEqual(old.accounts.map((a, i) => a.openingBalance + (i === 0 ? 500 : 0)));
  expect(copy.accounts[0].source).toBeUndefined();
  expect(copy.categories.map((c) => c.id)).toEqual(old.categories.map((c) => c.id));
});
