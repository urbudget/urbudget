import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Budget } from "../types";
import { configureFormat } from "../format";
const read = vi.hoisted(() => vi.fn());
vi.mock("../bridge", () => ({ read, api: {} }));
vi.mock("../charts/Charts", () => ({
  Legend: () => null, LinesChart: () => null, BarList: () => null, ColumnChart: () => null,
  DivergingList: () => null, Donut: () => null, HeatCalendar: () => null, Treemap: () => null,
}));
vi.mock("./Spending", () => ({
  ByCategory: ({ scope }: { scope: { show: (figure: { title: string; categoryId: string }) => void } }) =>
    <button onClick={() => scope.show({ title: "Food", categoryId: "food" })}>Inspect Food</button>,
  ByPayee: () => null, OverTime: () => null, SpendingCalendar: () => null,
}));
import { Reports } from "../Reports";
import { Register } from "../Register";
import { ToastProvider } from "../ui";
import { reportTransactionIds } from "./TransactionsDialog";
import { Forecast } from "./Forecast";

beforeEach(() => configureFormat({ currency: "USD" }));
afterEach(() => { cleanup(); localStorage.clear(); vi.clearAllMocks(); });
const budget = (): Budget => ({ schemaVersion: 2, name: "Test", currency: "USD", assignments: [],
  accounts: ["checking", "savings"].map(id => ({ id, name: id, kind: "cash", archived: false, openingBalance: 1000000 })),
  categories: ["food", "travel"].map(id => ({ id, name: id, group: "Living", target: 0 })),
  transactions: ["checking", "savings"].map((accountId, i) => ({ id: `spend${i}`, accountId, date: "2026-10-10",
    amount: -1000, payee: "Shop", memo: "", approved: true, status: "cleared", transferAccountId: null, transferStatus: null,
    splits: [{ categoryId: "food", amount: -1000 }] })),
});

it("preserves the report's account scope through dialog-to-register navigation and can clear it", async () => {
  const b = budget();
  read.mockResolvedValue({ months: [], categories: { food: 2000 }, incomeSources: {}, income: 0,
    spending: 2000, ageOfMoney: null, ageOfMoneyByMonth: {} });
  const inspect = vi.fn();
  const view = render(<ToastProvider><Reports budget={b} budgetId="test" month="2026-10" onInspect={inspect} /></ToastProvider>);
  await screen.findByRole("button", { name: "Inspect Food" });
  fireEvent.click(screen.getByRole("button", { name: /Accounts: All accounts/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: "savings" }));
  fireEvent.keyDown(document, { key: "Escape" });
  fireEvent.click(await screen.findByRole("button", { name: "Inspect Food" }));
  const dialog = screen.getByRole("dialog", { name: "Food" });
  expect(within(dialog).queryByText("savings")).toBeNull();
  fireEvent.click(within(dialog).getByRole("button", { name: "Open in register" }));
  expect(inspect.mock.calls[0][0].reportQuery.accountIds).toEqual(new Set(["checking"]));
  view.unmount();
  render(<ToastProvider><Register budget={b} accountId="all" initialFilter={inspect.mock.calls[0][0]}
    edit={vi.fn()} transform={vi.fn()} onEdit={vi.fn()} /></ToastProvider>);
  const rows = () => screen.getAllByRole("row").filter(row => row.hasAttribute("aria-selected"));
  expect(rows()).toHaveLength(1);
  expect(rows()[0].textContent).toContain("checking");
  fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
  await waitFor(() => expect(rows()).toHaveLength(2));
  expect(screen.queryByRole("button", { name: "Remove report scope" })).toBeNull();
});

it("keeps exact income/payee, multi-category and budget-side transfer semantics", () => {
  const b = budget();
  b.accounts.push({ id: "asset", name: "Asset", kind: "tracking", archived: false, openingBalance: 0 });
  b.transactions.push(
    { ...b.transactions[0], id: "income", payee: "Shop", amount: 1000, splits: [{ categoryId: null, amount: 1000 }] },
    { ...b.transactions[0], id: "travel", splits: [{ categoryId: "travel", amount: -1000 }] },
    { ...b.transactions[0], id: "reverse", accountId: "asset", transferAccountId: "checking", amount: 1000,
      splits: [{ categoryId: "travel", amount: 1000 }] },
  );
  const q = { from: "2026-10-01", to: "2026-10-31", accountIds: new Set(["checking"]) };
  expect(reportTransactionIds(b, { ...q, income: true, payee: "Shop" })).toEqual(new Set(["income"]));
  expect(reportTransactionIds(b, { ...q, categoryIds: new Set(["food", "travel"]) }))
    .toEqual(new Set(["spend0", "travel", "reverse"]));
  expect(reportTransactionIds(b, { ...q, categoryIds: new Set() })).toEqual(new Set());
});

it("requests the selected accounts' spending history again when forecast account selection changes", async () => {
  const b = budget();
  read.mockImplementation(async (_b, _command, args) => ({
    balances: { checking: 1000000, savings: 1000000 }, events: [],
    recentSpending: args.accountIds.includes("savings") ? 0 : 300000,
  }));
  const view = render(<Forecast budget={b} accounts={new Set(["checking"])} inspect={vi.fn()} />);
  await waitFor(() => expect(screen.getByLabelText("Everyday spending a month")).toHaveProperty("value", "$3,000.00"));
  const previousRead = read.mock.calls.at(-1)![3] as AbortSignal;
  view.rerender(<Forecast budget={b} accounts={new Set(["savings"])} inspect={vi.fn()} />);
  await waitFor(() => expect(screen.getByLabelText("Everyday spending a month")).toHaveProperty("value", "$0.00"));
  expect(read).toHaveBeenLastCalledWith(b, "forecast", expect.objectContaining({ accountIds: ["savings"] }), expect.any(AbortSignal));
  expect(previousRead.aborted).toBe(true);
  expect(screen.getByText("In 6 months").closest("div")!.textContent).toContain("$10,000.00");
});

it("hides old report figures and drilldowns when filters change, including after failure, and retries", async () => {
  const data = { months: [], categories: { food: 2000 }, incomeSources: {}, income: 0,
    spending: 2000, ageOfMoney: null, ageOfMoneyByMonth: {} };
  let reject!: (error: Error) => void;
  read.mockResolvedValueOnce(data).mockImplementationOnce(() => new Promise((_resolve, fail) => { reject = fail; }));
  render(<ToastProvider><Reports budget={budget()} budgetId="test" month="2026-10" onInspect={vi.fn()} /></ToastProvider>);
  fireEvent.click(await screen.findByRole("button", { name: "Inspect Food" }));
  expect(screen.getByRole("dialog", { name: "Food" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /Accounts: All accounts/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: "savings" }));
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("button", { name: "Inspect Food" })).toBeNull();
  expect(screen.queryByRole("dialog", { name: "Food" })).toBeNull();
  await act(async () => reject(new Error("Report unavailable")));
  expect(screen.getByRole("alert").textContent).toContain("Report unavailable");
  expect(screen.queryByRole("button", { name: "Inspect Food" })).toBeNull();
  read.mockResolvedValueOnce(data);
  fireEvent.click(screen.getByRole("button", { name: "Retry report" }));
  expect(screen.queryByRole("alert")).toBeNull();
  await screen.findByRole("button", { name: "Inspect Food" });
});

it("does not reuse a prior report while a changed budget snapshot is pending", async () => {
  const b = budget();
  read.mockResolvedValueOnce({ months: [], categories: {}, incomeSources: {}, income: 0,
    spending: 0, ageOfMoney: null, ageOfMoneyByMonth: {} }).mockImplementationOnce(() => new Promise(() => {}));
  const view = render(<Reports budget={b} budgetId="test" month="2026-10" onInspect={vi.fn()} />);
  await screen.findByRole("button", { name: "Inspect Food" });
  view.rerender(<Reports budget={{ ...b, name: "New snapshot" }} budgetId="test" month="2026-10" onInspect={vi.fn()} />);
  expect(screen.queryByRole("button", { name: "Inspect Food" })).toBeNull();
});

it("keeps forecast range controls available after failure and clears old errors while recalculating", async () => {
  const b = budget();
  const data = { balances: { checking: 1000000, savings: 1000000 }, events: [], recentSpending: 300000 };
  read.mockResolvedValueOnce(data).mockRejectedValueOnce(new Error("Forecast exceeds supported range"));
  const view = render(<Forecast budget={b} accounts={new Set(["checking"])} inspect={vi.fn()} />);
  await screen.findByLabelText("Everyday spending a month");
  view.rerender(<Forecast budget={b} accounts={new Set(["savings"])} inspect={vi.fn()} />);
  expect(screen.queryByLabelText("Everyday spending a month")).toBeNull();
  await screen.findByRole("button", { name: "Retry forecast" });
  read.mockImplementationOnce(() => new Promise(() => {}));
  fireEvent.click(screen.getByRole("button", { name: "Retry forecast" }));
  expect(screen.queryByRole("alert")).toBeNull();
  read.mockResolvedValueOnce(data);
  fireEvent.click(screen.getByRole("tab", { name: "3 months" }));
  await screen.findByLabelText("Everyday spending a month");
  expect(screen.queryByRole("alert")).toBeNull();
});

it("offers expense filtering for income vs expense and keeps the income visible", async () => {
  localStorage.setItem("lb:reports:view", "incomeExpense");
  const b = budget();
  read.mockImplementation(async (_budget, _command, { filter }) => ({
    months: [{ month: "2026-10", income: 100000, spending: 1000, netIncome: 99000, assets: 0, debt: 0,
      netWorth: 0, categories: { food: 1000 }, incomeSources: { Employer: 100000 }, accounts: {} }],
    income: 100000, spending: 1000, categories: { food: 1000 }, incomeSources: { Employer: 100000 },
    ageOfMoney: null, ageOfMoneyByMonth: {}, selected: filter.categoryIds,
  }));
  render(<ToastProvider><Reports budget={b} budgetId="test" month="2026-10" onInspect={vi.fn()} /></ToastProvider>);
  await screen.findByText("Employer");
  fireEvent.click(screen.getByRole("button", { name: /Categories: All categories/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: "travel" }));
  fireEvent.keyDown(document, { key: "Escape" });
  await waitFor(() => expect(read).toHaveBeenLastCalledWith(b, "calculate_reports", expect.objectContaining({
    filter: expect.objectContaining({ categoryIds: expect.arrayContaining(["food", "__uncategorized"]) }),
  }), expect.any(AbortSignal)));
  expect(screen.getByText("Employer")).toBeTruthy();
  expect(screen.queryByText(/this report needs every category/)).toBeNull();
});

it("keeps uncategorized report spending visible and opens exactly its transactions", async () => {
  const b = budget();
  b.transactions.push({ ...b.transactions[0], id: "missing", payee: "Mystery", splits: [{ categoryId: null, amount: -700 }] });
  read.mockResolvedValue({ months: [], income: 0, spending: 700, categories: { __uncategorized: 700 },
    incomeSources: {}, ageOfMoney: null, ageOfMoneyByMonth: {} });
  const inspect = vi.fn();
  render(<ToastProvider><Reports budget={b} budgetId="test" month="2026-10" onInspect={inspect} /></ToastProvider>);
  await screen.findByText(/of spending still needs a category/);
  fireEvent.click(screen.getByRole("button", { name: "Review transactions" }));
  const dialog = screen.getByRole("dialog", { name: "Needs a category" });
  expect(within(dialog).getByText("Mystery")).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Open in register" }));
  expect(reportTransactionIds(b, inspect.mock.calls[0][0].reportQuery)).toEqual(new Set(["missing"]));
});

it("remembers what each budget's reports leave out, shows it, and lets each item back in", async () => {
  const data = { months: [], categories: { food: 2000 }, incomeSources: {}, income: 0,
    spending: 2000, ageOfMoney: null, ageOfMoneyByMonth: {} };
  read.mockResolvedValue(data);
  const sent = () => read.mock.calls.at(-1)![2].filter;
  const b = budget();
  const first = render(<ToastProvider><Reports budget={b} budgetId="home" month="2026-10" onInspect={vi.fn()} /></ToastProvider>);
  await screen.findByRole("button", { name: "Inspect Food" });
  expect(screen.queryByText("Categories left out:")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /Categories: All categories/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: "travel" }));
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.getByRole("button", { name: /Categories: 1 hidden/ }).className).toContain("active");
  expect(screen.getByText("Categories left out:")).toBeTruthy();
  first.unmount();

  // Leaving Reports and coming back keeps travel out, and a category added since is counted.
  b.categories.push({ id: "rent", name: "rent", group: "Living", target: 0 });
  const again = render(<ToastProvider><Reports budget={b} budgetId="home" month="2026-10" onInspect={vi.fn()} /></ToastProvider>);
  await screen.findByRole("button", { name: "Inspect Food" });
  expect(sent().categoryIds).toEqual(["food", "rent", "__uncategorized"]);
  expect(screen.getByText("Categories left out:").parentElement!.textContent).toContain("travel");
  again.unmount();

  // Another budget leaves out nothing.
  const other = render(<ToastProvider><Reports budget={b} budgetId="work" month="2026-10" onInspect={vi.fn()} /></ToastProvider>);
  await screen.findByRole("button", { name: "Inspect Food" });
  expect(sent().categoryIds).toEqual([]);
  expect(screen.queryByText("Categories left out:")).toBeNull();
  other.unmount();

  render(<ToastProvider><Reports budget={b} budgetId="home" month="2026-10" onInspect={vi.fn()} /></ToastProvider>);
  fireEvent.click(await screen.findByRole("button", { name: "Show travel in reports" }));
  await waitFor(() => expect(sent().categoryIds).toEqual([]));
  expect(screen.queryByText("Categories left out:")).toBeNull();
  expect(JSON.parse(localStorage.getItem("lb:reports:hidden:home")!)).toEqual({ accounts: [], categories: [] });
});

it("names the few accounts still shown when most are left out, and not categories where they don't apply", async () => {
  localStorage.setItem("lb:reports:view", "income");
  localStorage.setItem("lb:reports:hidden:home", JSON.stringify({ accounts: ["savings", "gone"], categories: ["travel"] }));
  read.mockResolvedValue({ months: [], categories: {}, incomeSources: {}, income: 0, spending: 0,
    ageOfMoney: null, ageOfMoneyByMonth: {} });
  const b = budget();
  b.accounts.push({ id: "card", name: "card", kind: "credit", archived: false, openingBalance: 0 });
  render(<ToastProvider><Reports budget={b} budgetId="home" month="2026-10" onInspect={vi.fn()} /></ToastProvider>);
  await waitFor(() => expect(read).toHaveBeenCalled());
  expect(read.mock.calls.at(-1)![2].filter).toMatchObject({ accountIds: ["checking", "card"], categoryIds: [] });
  expect(screen.getByText("Accounts left out:").parentElement!.textContent).toContain("savings");
  // The income breakdown has no Categories filter, so nothing says categories are left out.
  expect(screen.queryByText("Categories left out:")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /Accounts: 1 hidden/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: "card" }));
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.getByText("Only these accounts:").parentElement!.textContent).toContain("checking");
  expect(screen.getByRole("button", { name: /Accounts: checking/ })).toBeTruthy();
  // A deleted account drops out once the choice changes.
  expect(JSON.parse(localStorage.getItem("lb:reports:hidden:home")!).accounts.sort()).toEqual(["card", "savings"]);
  fireEvent.click(screen.getByRole("button", { name: "Show all accounts" }));
  expect(screen.queryByText("Only these accounts:")).toBeNull();
  expect(screen.getByRole("button", { name: /Accounts: All accounts/ })).toBeTruthy();
});
