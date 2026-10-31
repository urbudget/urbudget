import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CategoryRow, type CategorySummary } from "./CategoryRow";
import { BudgetInspector } from "./BudgetInspector";
import { TargetEditor } from "./TargetEditor";
import { CategoryForm } from "./forms";
import { demoBudget } from "./demo";
import type { Budget, Category, Summary } from "./types";

vi.mock("./QuickAssign", () => ({ AUTO_ASSIGN: [], QuickAssign: () => null }));
vi.mock("./LoanHeader", () => ({ CategoryLoans: () => null }));
afterEach(cleanup);

const month = "2026-10";
const noop = () => {};
const category: Category = { id: "goal", name: "Savings", group: "Goals", target: 10000, goal: { goal_type: "TB" } };
const row: CategorySummary = { id: "goal", assigned: 1000, activity: 0, available: 2500, carried: 1500, needed: 0, targetFundingRequired: 10000, targetFundingFunded: 2500 };

function renderRow(c: Category, s: CategorySummary) {
  return render(
    <CategoryRow category={c} summary={s} month={month} selected={false} renaming={false} drop={null} dragging={false} dnd={null}
      onSelect={noop} onAssign={noop} onRename={noop} onStartRename={noop} onMenu={noop} onMoveMoney={noop} onActivity={noop}
      moveCount={0} movedMoney={false} onMoves={noop} onStep={noop} />,
  );
}

describe("target status", () => {
  it.each([["Cash Reserve", 2_000_000], ["Budget Margin", 200_000]] as const)("shows no funding warning for %s after moving its balance out", (name, amount) => {
    const c = { ...category, name, target: 0, goal: null };
    const s = { ...row, assigned: -amount, carried: amount, available: 0, needed: 0, targetFundingRequired: 0, targetFundingFunded: 0 };
    const view = renderRow(c, s);
    expect(view.container.querySelector(".row-status")?.textContent).toBe("");
    expect(screen.getByRole("button", { name: new RegExp(`available in ${name}`) }).classList.contains("zero")).toBe(true);
    view.unmount();
    const details = inspector([c], [s], [c]);
    expect(screen.queryByText("Still needed")).toBeNull();
    expect(details.container.querySelector(".target-status.short")).toBeNull();
  });

  it("keeps an undated balance goal green while showing its progress and what's needed eventually", () => {
    const { container } = renderRow(category, row);
    expect(screen.getByRole("button", { name: /available in Savings/ }).classList.contains("positive")).toBe(true);
    expect(container.querySelector(".target-bar i")).toHaveProperty("style.width", "25%");
    expect(container.textContent).toContain("75.00 SAR more needed eventually");
    expect(container.textContent).not.toMatch(/needed (this month|by)/);
  });

  it("shows scheduled shortfall even when the target is snoozed", () => {
    const { container } = renderRow({ ...category, goal: { goal_type: "NEED", goal_snoozed_at: `${month}-01` } }, { ...row, needed: 3500, upcoming: 6000 });
    const available = screen.getByRole("button", { name: /available in Savings/ });
    expect(available.classList.contains("underfunded")).toBe(true);
    expect(container.textContent).toMatch(/35\.00 SAR (more )?needed/);
    expect(screen.queryByText("Snoozed")).toBeNull();
  });

  it.each([false, true])("shows overspending before snooze (credit: %s)", (credit) => {
    const { container } = renderRow({ ...category, goal: { goal_type: "NEED", goal_snoozed_at: `${month}-01` } },
      { ...row, available: -500, needed: 500, creditOverspending: credit ? 500 : 0 });
    expect(screen.getByRole("button", { name: /available in Savings/ }).classList.contains(credit ? "credit" : "negative")).toBe(true);
    expect(container.textContent).toMatch(/[Oo]verspent/);
    expect(screen.queryByText("Snoozed")).toBeNull();
  });
});

function inspector(categories: Category[], rows: CategorySummary[], selected: Category[] = [], targetOpen = false, selectedMonth = month) {
  const budget = { ...demoBudget(), categories };
  const summary: Summary = { assigned: 0, available: 0, activity: 0, income: 0, netWorth: 0, readyToAssign: 0, accounts: [], categories: rows };
  return render(<BudgetInspector budget={budget} summary={summary} summaries={new Map(rows.map((s) => [s.id, s]))}
    month={selectedMonth} currentMonth={month} selected={selected} visibleIds={categories.map((c) => c.id)}
    onAutoAssign={noop} onEditCategory={noop} onClear={noop} edit={noop} targetOpen={targetOpen} onTargetOpen={noop} onSaved={noop} />);
}

it("global funding omits an undated balance goal but retains schedules and snoozed overspending", () => {
  const snoozed = { ...category, id: "snoozed", name: "Snoozed goal", goal: { goal_type: "NEED", goal_snoozed_at: `${month}-01` } };
  const { container } = inspector([category, snoozed], [{ ...row, upcoming: 6000 }, { ...row, id: snoozed.id, available: -500, needed: 500 }]);
  expect(container.querySelector(".target-status")?.textContent).toBe("40.00 SAR still needed this month");
});

it("a balance goal still to save leaves the month's funding complete, also when it's selected", () => {
  const global = inspector([category], [row]);
  expect(screen.getByText("This month's funding is complete")).toBeTruthy();
  global.unmount();
  const { container } = inspector([category], [row], [category]);
  expect(screen.queryByText("Still needed")).toBeNull();
  expect(container.querySelector(".target-status.later")?.textContent).toBe("75.00 SAR more needed eventually");
});

it("the inspector passes the viewed month into its target editor", () => {
  inspector([category], [row], [category], true, "2026-11");
  expect(screen.queryByLabelText(/Snooze target/)).toBeNull();
});

describe.each(["inspector", "dialog"] as const)("%s target editing", (editor) => {
  function renderEditor(c: Category, selectedMonth: string, save: (c: Category) => void) {
    return editor === "inspector" ? render(
      <TargetEditor category={c} month={selectedMonth} currentMonth={month} open onOpenChange={noop} onSaved={noop}
        edit={(change) => { const b: Budget = { ...demoBudget(), categories: [{ ...c }] }; change(b); save(b.categories[0]); }} />,
    ) : render(<CategoryForm initial={c} groups={[c.group]} month={selectedMonth} currentMonth={month} close={noop} save={save} />);
  }

  it("preserves historical snooze metadata while saving a changed amount", () => {
    const save = vi.fn();
    const c = { ...category, goal: { goal_type: "NEED", goal_snoozed_at: "2026-09-01", imported: "retained" } };
    renderEditor(c, "2026-11", save);
    expect(screen.queryByLabelText(/Snooze target/)).toBeNull();
    const amount = screen.getByLabelText(editor === "inspector" ? "Amount" : "Target amount");
    fireEvent.change(amount, { target: { value: "125" } });
    fireEvent.submit(amount.closest("form")!);
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ target: 12500, goal: expect.objectContaining(c.goal) }));
  });

  it("never offers snooze for a card payment target", () => {
    renderEditor({ ...category, paymentAccountId: "card" }, month, noop);
    expect(screen.queryByLabelText(/Snooze target/)).toBeNull();
  });
});
