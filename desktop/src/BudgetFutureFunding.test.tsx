import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
vi.mock("./bridge", () => ({ api: { reports: vi.fn(async () => ({ ageOfMoney: null })), autoAssignPreview: vi.fn(async () => ({})) } }));
import { BudgetPage } from "./BudgetPage";
import { ToastProvider } from "./ui";
import { demoBudget } from "./demo";
import { configureFormat } from "./format";
import type { Summary } from "./types";

afterEach(cleanup);
it("shows an intermediate future shortage even when later returns cancel the aggregate", () => {
  const budget = demoBudget();
  configureFormat(budget);
  const summary: Summary = {
    readyToAssign: 100000, futureAssigned: 0, futureReadyToAssign: 100000,
    futureShortfalls: [{ month: "2026-11", amount: 50000 }],
    assigned: 0, activity: 0, available: 0, income: 0, netWorth: 100000,
    accounts: [], categories: [],
  };
  render(<ToastProvider><BudgetPage budget={budget} summary={summary} month="2026-10"
    edit={vi.fn()} transform={vi.fn()} onEditorChange={vi.fn()} onEditCategory={vi.fn()} /></ToastProvider>);
  const warning = screen.getByText(/November 2026 is short by 500.00/);
  expect(warning.closest(".notice")?.classList.contains("error")).toBe(true);
  expect(warning.textContent).toContain("Move funds back from a future month");
});
it.each([
  [150000, true],
  [-150000, false],
])("explains a past month's Ready to Assign without a shortage, assigned later %s", (futureAssigned, heldBack) => {
  const budget = demoBudget();
  configureFormat(budget);
  // Past months list no shortfalls (later income funds later months), even when more is assigned
  // later than the month held.
  const summary: Summary = {
    readyToAssign: heldBack ? 0 : 100000, futureAssigned, futureReadyToAssign: 100000 - futureAssigned,
    futureShortfalls: [],
    assigned: 0, activity: 0, available: 0, income: 0, netWorth: 100000,
    accounts: [], categories: [],
  };
  render(<ToastProvider><BudgetPage budget={budget} summary={summary} month="2024-10"
    edit={vi.fn()} transform={vi.fn()} onEditorChange={vi.fn()} onEditCategory={vi.fn()} /></ToastProvider>);
  const notice = screen.getByText(/Assigned in future months/);
  expect(notice.classList.contains("error")).toBe(false);
  expect(notice.textContent).not.toContain("short");
  expect(notice.textContent?.includes("held back")).toBe(heldBack);
});
