import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { configureFormat } from "../format";
import type { Budget, Portfolio } from "../types";
// The engine's own report for its test budget, as it sends it (crates/urbudget-core's investment_stats tests write it).
import engineBudget from "../../../shared/fixtures/investments-report.json";
import engineReport from "../../../shared/fixtures/investments-report-expected.json";

const mocked = vi.hoisted(() => ({ read: vi.fn(), exportInvestments: vi.fn(), exportReports: vi.fn(), reveal: vi.fn() }));
vi.mock("../bridge", () => ({
  read: mocked.read,
  api: { exportInvestments: mocked.exportInvestments, exportReports: mocked.exportReports, reveal: mocked.reveal },
}));
import { Reports } from "../Reports";
import { ToastProvider } from "../ui";
import { Allocation, Earnings, Performance } from "./Investing";
import { investingBudget, investingReport, portfolio, portfolioWithWhatsHeld } from "./portfolio.fixture";
import { cashByCurrency, decimal, inCurrency, ownAmount, whatItIs } from "../holdings";

beforeEach(() => {
  configureFormat({ currency: "USD" });
  localStorage.clear();
  sessionStorage.clear();
  mocked.read.mockImplementation(async (_budget: unknown, command: string) =>
    command === "investments_report" ? portfolio() : command === "forecast" ? { balances: {}, events: [], recentSpending: 0 } : investingReport(),
  );
});
afterEach(cleanup);

const tile = (label: string) =>
  [...document.querySelectorAll(".report-tiles > div")].find((node) => node.querySelector("dt")?.textContent === label)?.textContent;
/** A table row's cells as text, by its first cell. */
const cells = (table: HTMLElement, label: string) => {
  const row = within(table).getAllByRole("row").find((r) => r.querySelector("td, th")?.textContent === label);
  return row ? [...row.querySelectorAll("td, th")].slice(1, -1).map((cell) => cell.textContent) : null;
};
/** A holding's row in a table: the line under its name, and its other cells. */
const holding = (table: HTMLElement, name: string) => {
  const row = within(table).getAllByRole("row").find((r) => r.querySelector("td")?.firstChild?.textContent === name);
  return row ? { line: row.querySelector(".sub-line")?.textContent ?? null, cells: [...row.querySelectorAll("td")].slice(1, -1).map((c) => c.textContent) } : null;
};
const panel = (title: string) => screen.getByRole("heading", { level: 2, name: title }).closest("section")!;
const reportTabs = () => within(screen.getByRole("tablist", { name: "Reports" })).getAllByRole("tab").map((tab) => tab.textContent);
const reports = (budget = investingBudget(), onOpenAccount = vi.fn()) =>
  render(
    <ToastProvider>
      <Reports budget={budget} budgetId="test" month="2026-11" onInspect={vi.fn()} onOpenAccount={onOpenAccount} />
    </ToastProvider>,
  );

it("has no Investments report until an account has holdings", async () => {
  // Someone was in Investments; in a budget without holdings that isn't there, and the overview opens.
  sessionStorage.setItem("lb:reports:place", "allocation");
  const none = reports({ ...investingBudget(), investments: [] });
  expect(reportTabs()).toEqual(["Overview", "Spending", "Income", "Net worth", "Balance sheet", "Forecast"]);
  expect(screen.getByRole("tab", { name: "Overview" }).getAttribute("aria-selected")).toBe("true");
  expect(screen.queryByRole("button", { name: "Investments" })).toBeNull();
  await waitFor(() => expect(mocked.read).toHaveBeenCalled());
  expect(mocked.read.mock.calls.some((call) => call[1] === "investments_report")).toBe(false);
  none.unmount();

  mocked.read.mockClear();
  reports();
  expect(reportTabs()).toEqual(["Overview", "Spending", "Income", "Net worth", "Balance sheet", "Investments", "Forecast"]);
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Investments");
  const views = within(screen.getByRole("tablist", { name: "View" })).getAllByRole("tab");
  expect(views.map((tab) => tab.textContent)).toEqual(["Performance", "Allocation", "Income and costs"]);
  expect(views[1].getAttribute("aria-selected")).toBe("true");
});

it("reads the investments report for the accounts with holdings, and opens an account's holdings from its row", async () => {
  const open = vi.fn();
  const budget = investingBudget();
  reports(budget, open);
  fireEvent.click(screen.getByRole("tab", { name: "Investments" }));
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Investments");
  expect(screen.getByRole("tab", { name: "Performance" }).getAttribute("aria-selected")).toBe("true");
  await screen.findByRole("heading", { level: 2, name: "Gain each month" });
  const sent = () => mocked.read.mock.calls.filter((call) => call[1] === "investments_report").at(-1)![2].filter;
  expect(sent()).toMatchObject({ accountIds: [], categoryIds: [] });
  // The shared report isn't asked for again while these reports show.
  expect(mocked.read.mock.calls.at(-1)![1]).toBe("investments_report");

  // The filter lists only the accounts with holdings.
  fireEvent.click(screen.getByRole("button", { name: /^Filters:/ }));
  fireEvent.click(screen.getByRole("button", { name: /Accounts: All accounts/ }));
  expect(screen.getAllByRole("checkbox").map((box) => box.parentElement!.textContent)).toEqual(["Tracking", "Broker", "Pension"]);
  fireEvent.click(screen.getByRole("checkbox", { name: "Pension" }));
  fireEvent.keyDown(document, { key: "Escape" });
  await waitFor(() => expect(sent().accountIds).toEqual(["broker"]));

  fireEvent.click(await screen.findByRole("button", { name: "Broker" }));
  expect(open).toHaveBeenCalledWith("broker");
});

it("shows today's holdings with no months to choose, and exports the investments' own file", async () => {
  mocked.exportInvestments.mockResolvedValue(null);
  reports();
  fireEvent.click(screen.getByRole("tab", { name: "Investments" }));
  await screen.findByRole("heading", { level: 2, name: "By account" });
  // The filter's label names the months where the view has any.
  expect(screen.getByRole("button", { name: /^Filters: .+ · All accounts$/ })).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "Allocation" }));
  await screen.findByText("Today's holdings. What was held on past days isn't kept.");
  fireEvent.click(screen.getByRole("button", { name: "Filters: All accounts" }));
  expect(screen.getByRole("button", { name: /^Accounts:/ })).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^Months:/ })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Export CSV" }));
  await waitFor(() => expect(mocked.exportInvestments).toHaveBeenCalled());
  expect(mocked.exportInvestments.mock.calls[0].slice(1)).toEqual([expect.objectContaining({ accountIds: [], categoryIds: [] })]);
  expect(mocked.exportReports).not.toHaveBeenCalled();
});

it("shows how the investments did, by month and by account", () => {
  const open = vi.fn();
  const p = portfolio();
  p.total.yearly = 0.31;
  render(<Performance portfolio={p} budget={investingBudget()} onOpenAccount={open} />);
  expect(tile("Value now")).toContain("$12,100.00");
  expect(tile("Value now")).toContain("end of November 2026");
  expect(tile("Gain")).toContain("+$700.00");
  expect(tile("Gain")).toContain("since September 2026 began");
  // No trades are known, so nothing is said of what was realised.
  expect(tile("Gain")).not.toContain("Realised");
  expect(tile("Return")).toContain("+6.8%");
  expect(tile("Return")).toContain("+31.0% a year");
  expect(tile("Money put in")).toContain("+$2,400.00");
  // Columns by account under the money put in: 9,000 at the start and 2,400 since.
  const value = within(panel("Value")).getByRole("img", { name: "Value" });
  const labels = [...value.querySelectorAll(".chart-hit")].map((hit) => hit.getAttribute("aria-label"));
  expect(labels[0]).toContain("Broker $9,300.00");
  expect(labels[0]).toContain("Money put in $9,200.00");
  expect(labels[2]).toContain("Pension $2,100.00");
  expect(labels[2]).toContain("Total $12,100.00");
  expect(labels[2]).toContain("Money put in $11,400.00");
  const table = within(panel("By account")).getByRole("table");
  expect(cells(table, "Broker")).toEqual(["Interactive Brokers", "$9,000.00", "+$400.00", "+$600.00", "+6.5%", "$10,000.00"]);
  expect(cells(table, "Pension")).toEqual(["Positions file", "$0.00", "+$2,000.00", "+$100.00", "+5.0%", "$2,100.00"]);
  expect(cells(table, "Total")).toEqual(["", "$9,000.00", "+$2,400.00", "+$700.00", "+6.8%", "$12,100.00"]);
  fireEvent.click(within(table).getByRole("button", { name: "Pension" }));
  expect(open).toHaveBeenCalledWith("pension");
  // What the figures rest on: sparse values, estimated days, missing sections, and accounts nothing fills.
  expect(screen.getByText("Valued only when updated, so a gain shows in the month of the next update: Pension.")).toBeTruthy();
  expect(screen.getByText(/^On 2 days the value jumped/)).toBeTruthy();
  expect(screen.getByText(/Transfers: without it money moved in from another account is only estimated/)).toBeTruthy();
  const unfilled = screen.getByText(/^Not counted here/);
  expect(unfilled.textContent).toBe("Not counted here, because nothing keeps them up to date: Idle fund.");
  fireEvent.click(within(unfilled).getByRole("button", { name: "Idle fund" }));
  expect(open).toHaveBeenLastCalledWith("idle");
});

it("shows what the accounts hold together, by kind, account and currency", () => {
  const p = portfolio();
  render(<Allocation portfolio={p} budget={investingBudget()} />);
  expect(tile("Holdings")).toContain("4");
  expect(tile("Largest holding")).toContain("34.5%");
  expect(tile("Largest holding")).toContain("Vanguard FTSE All-World");
  expect(tile("Top five holdings")).toContain("91.2%");
  expect(tile("Cash")).toContain("8.4%");
  const parts = () =>
    within(panel("What it holds")).getAllByRole("listitem").map((item) => [
      item.querySelector(".bar-list-label")!.textContent,
      item.querySelector(".muted")!.textContent,
    ]);
  // Cash and accrued income are listed with the kinds; a holding of no known kind is an other investment.
  expect(parts()).toEqual([
    ["Funds", "34.5%"], ["Stocks", "33.6%"], ["Bonds", "21.0%"], ["Cash", "8.4%"], ["Other investments", "2.1%"], ["Accrued", "0.4%"],
  ]);
  fireEvent.click(screen.getByRole("tab", { name: "By account" }));
  expect(parts()).toEqual([["Broker", "82.4%"], ["Pension", "17.6%"]]);
  fireEvent.click(screen.getByRole("tab", { name: "By currency" }));
  expect(parts()).toEqual([["USD", "91.6%"], ["EUR", "8.4%"]]);
  // A holding in two accounts is one row naming both.
  const table = within(panel("Largest holdings")).getByRole("table");
  expect(holding(table, "Vanguard FTSE All-World")).toEqual({ line: "Fund · VWRA", cells: ["Broker, Pension", "$4,100.00", "34.5%", "+$300.00", "+7.9%"] });
  expect(holding(table, "T 4 1/8 11/15/32")).toEqual({ line: "Bond", cells: ["Broker", "$2,500.00", "21.0%", "-$100.00", "-3.8%"] });
  expect(holding(table, "XYZ WT")).toEqual({ line: "Other", cells: ["Broker", "$250.00", "2.1%", "", ""] });
  expect(screen.getByText(/^Pension · Prices from 2026-11-10/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Show all/ })).toBeNull();
});

it("says what each holding is, and what it and each currency come to in their own currency", () => {
  const p = portfolioWithWhatsHeld();
  p.holdings.push({ ...p.holdings[3], key: "AAPL 261218C00250000", symbol: "AAPL 261218C00250000", name: "AAPL 18DEC26 250 C", group: "Options", one: "Option", instrument: "Call option", amount: "840.00", value: 0 });
  render(<Allocation portfolio={p} budget={investingBudget()} />);
  const table = within(panel("Largest holdings")).getByRole("table");
  // The amount in a holding's own currency only where that isn't the budget's (dollars here).
  // On a line of its own under the name, so neither is cut short by the other.
  expect(holding(table, "Vanguard FTSE All-World")).toEqual({ line: "ETF · VWRA", cells: ["Broker, Pension", "$4,100.00", "34.5%", "+$300.00", "+7.9%"] });
  expect(holding(table, "T 4 1/8 11/15/32")?.line).toBe("Government bond");
  expect(holding(table, "XYZ WT")?.line).toBe("Right · 230.00 EUR");
  // An option's long code only spells its name out again: it is left out.
  expect(holding(table, "AAPL 18DEC26 250 C")?.line).toBe("Call option · 840.00 EUR");
  fireEvent.click(screen.getByRole("tab", { name: "By currency" }));
  const currencies = within(panel("What it holds")).getAllByRole("listitem").map((item) => item.querySelector(".bar-list-label")!.textContent);
  expect(currencies).toEqual(["USD", "EUR18,630.5 EUR"]);
});

it("writes amounts in their own currency in the budget's number format, and only where they add something", () => {
  expect(decimal("61476.00")).toBe("61,476.00");
  expect(decimal("-1234567.5")).toBe("-1,234,567.5");
  expect(decimal("820")).toBe("820");
  expect(decimal("n/a")).toBe("n/a");
  expect(inCurrency("61476.00", "USD")).toBe("61,476.00 USD");
  expect(ownAmount("61476.00", "USD", "USD")).toBeNull();
  expect(ownAmount(null, "EUR", "USD")).toBeNull();
  expect(ownAmount(undefined, "EUR", "USD")).toBeNull();
  expect(ownAmount("184", "EUR", "USD")).toBe("184 EUR");
  expect(cashByCurrency([{ currency: "USD", amount: "796.50" }], "USD")).toBeNull();
  expect(cashByCurrency(undefined, "USD")).toBeNull();
  expect(cashByCurrency([{ currency: "USD", amount: "796.50" }, { currency: "SAR", amount: "1500.00" }], "SAR")).toBe("796.50 USD · 1,500.00 SAR");
  // What a holding is: its instrument, else one of its kind; translated where there is a translation.
  expect(whatItIs({ instrument: "Call option", one: "Option" })).toBe("Call option");
  expect(whatItIs({ one: "Fund" })).toBe("Fund");
  expect(whatItIs({ instrument: "", one: "" })).toBe("");
  configureFormat({ currency: "EUR", numberFormat: { decimal: ",", group: "." } });
  expect(inCurrency("61476.00", "USD")).toBe("61.476,00 USD");
});

it("folds the holdings past the tenth behind Show all", () => {
  const p = portfolio();
  p.holdings = Array.from({ length: 12 }, (_, i) => ({ ...p.holdings[1], key: `h${i}`, name: `Holding ${i + 1}`, symbol: `H${i + 1}` }));
  render(<Allocation portfolio={p} budget={investingBudget()} />);
  const body = () => within(panel("Largest holdings")).getByRole("table").querySelectorAll("tbody tr");
  expect(body()).toHaveLength(10);
  fireEvent.click(screen.getByRole("button", { name: "Show all 12" }));
  expect(body()).toHaveLength(12);
  fireEvent.click(screen.getByRole("button", { name: "Show fewer" }));
  expect(body()).toHaveLength(10);
});

it("shows what the investments earned and cost, by month and by account", () => {
  render(<Earnings portfolio={portfolio()} budget={investingBudget()} />);
  expect(tile("Dividends and interest")).toContain("+$45.00");
  // Tax and fees over the months, and the statement's commissions.
  expect(tile("Fees, tax and commissions")).toContain("-$60.40");
  // The commissions aren't in the months' net, and the tile says how much they are.
  expect(tile("Fees, tax and commissions")).toContain("Commissions on trades -$25.00");
  expect(tile("Net")).toContain("-$15.40");
  expect(tile("Yield")).toContain("0.5%");
  expect(tile("Yield")).toContain("Income over the average value");
  const months = within(panel("By month")).getByRole("table");
  expect(cells(months, "Sept 2026") ?? cells(months, "Sep 2026")).toEqual(["+$12.00", "+$3.00", "-$1.80", "-$10.00", "+$3.20"]);
  expect(cells(months, "Total")).toEqual(["+$36.00", "+$9.00", "-$5.40", "-$30.00", "+$9.60"]);
  // Each account's row is the engine's, with its statement's commissions among the costs.
  const accounts = within(panel("By account")).getByRole("table");
  expect(cells(accounts, "Broker")).toEqual(["+$45.00", "-$60.40", "-$15.40"]);
  expect(cells(accounts, "Total")).toEqual(["+$45.00", "-$60.40", "-$15.40"]);
  expect(screen.getByText("No income or costs reported: Pension.")).toBeTruthy();
  expect(screen.getByText(/^Broker · Commissions on trades -\$25\.00/)).toBeTruthy();
});

it("lists the dividends by holding, ten at first, and draws commissions only where they are known by month", () => {
  const p = portfolio();
  p.income.byHolding = Array.from({ length: 12 }, (_, i) => ({
    key: `h${i}`, symbol: `H${i + 1}`, name: `Holding ${i + 1}`, dividends: 1_200 - i * 100, tax: -180, value: null, yield: null,
  }));
  render(<Earnings portfolio={p} budget={investingBudget()} />);
  const body = () => within(panel("By holding")).getByRole("table").querySelectorAll("tbody tr");
  expect(body()).toHaveLength(10);
  fireEvent.click(screen.getByRole("button", { name: "Show all 12" }));
  expect(body()).toHaveLength(12);
  // With no commissions by month there is no column for them, and the note is the statement's figure.
  expect(within(panel("By month")).queryByRole("columnheader", { name: "Commissions on trades" })).toBeNull();
  expect(tile("Fees, tax and commissions")).toContain("Commissions on trades -$25.00");
  cleanup();
  render(<Earnings portfolio={portfolio()} budget={investingBudget()} />);
  expect(screen.queryByRole("heading", { level: 2, name: "By holding" })).toBeNull();
});

it("says so when nothing reports income", () => {
  const p: Portfolio = portfolio();
  p.income = { months: [], byAccount: [], byHolding: [], dividends: 0, interest: 0, tax: 0, fees: 0, other: 0, yield: null, commissions: [], reporting: [], silent: ["broker", "pension"] };
  render(<Earnings portfolio={p} budget={investingBudget()} />);
  expect(tile("Yield")).toContain("—");
  expect(tile("Fees, tax and commissions")).not.toContain("Commissions on trades");
  expect(screen.queryByRole("table")).toBeNull();
  expect(screen.getByText("No income or costs reported: Broker, Pension.")).toBeTruthy();
});

it("draws the engine's own report for its test budget", () => {
  // Assigned as it is: the engine's output has the shape these reports read.
  const p: Portfolio = engineReport;
  const budget = engineBudget.budget as unknown as Budget;
  const performance = render(<Performance portfolio={p} budget={budget} />);
  expect(tile("Value now")).toContain("$103,770.00");
  // One account has trades, so what they realised is known.
  expect(tile("Gain")).toContain("Realised +$170.00");
  expect(tile("Return")).toContain("+12.6%");
  const byAccount = within(panel("By account")).getByRole("table");
  expect(cells(byAccount, "Long term")).toEqual(["Interactive Brokers", "$41,000.00", "+$4,000.00", "+$8,000.00", "+18.1%", "$53,000.00"]);
  expect(cells(byAccount, "Pension")?.[0]).toBe("Value you type in");
  expect(screen.getByText(/^Not counted here, because nothing keeps them up to date: Old workplace plan\.$/)).toBeTruthy();
  performance.unmount();

  const allocation = render(<Allocation portfolio={p} budget={budget} />);
  expect(tile("Holdings")).toContain("7");
  expect(tile("Largest holding")).toContain("37.5%");
  expect(holding(within(panel("Largest holdings")).getByRole("table"), "World fund")).toMatchObject({ line: "ETF · VWRA", cells: ["Long term, Trading", "$39,000.00", "37.5%", "+$5,500.00", "+16.4%"] });
  allocation.unmount();

  render(<Earnings portfolio={p} budget={budget} />);
  expect(tile("Dividends and interest")).toContain("+$100.00");
  // Months with nothing in them are left out of the table; the total is of them all.
  const months = within(panel("By month")).getByRole("table");
  expect(cells(months, "Jan 2026")).toBeNull();
  // Commissions are a column where a statement gives them by month, a dash in the months before
  // it does, and part of each month's net; a month with nothing but commissions is still a row.
  expect(within(months).getByRole("columnheader", { name: "Commissions on trades" })).toBeTruthy();
  expect(cells(months, "Mar 2026")).toEqual(["+$40.00", "$0.00", "-$6.00", "$0.00", "—", "+$34.00"]);
  expect(cells(months, "Oct 2026")).toEqual(["+$7.00", "$0.00", "-$1.05", "$0.00", "-$1.50", "+$4.45"]);
  expect(cells(months, "Nov 2026")).toEqual(["$0.00", "$0.00", "$0.00", "$0.00", "-$0.90", "-$0.90"]);
  expect(cells(months, "Total")).toEqual(["+$92.00", "+$8.00", "-$13.80", "-$10.00", "-$4.20", "+$72.00"]);
  // They are a series on the chart too.
  const chart = within(panel("By month")).getByRole("img", { name: "Income and costs" });
  expect(chart.querySelectorAll(".chart-hit")[9].getAttribute("aria-label")).toContain("Commissions on trades -$1.50");
  // The tile's note is all of them: 4.20 by month and the other account's 23.50 for its statement.
  expect(tile("Fees, tax and commissions")).toContain("-$51.50");
  expect(tile("Fees, tax and commissions")).toContain("Commissions on trades -$27.70");
  expect(months.querySelectorAll("tbody tr").length).toBeLessThan(p.income.months.length);
  const accounts = within(panel("By account")).getByRole("table");
  // 85.00 + 8.00 earned; 12.75 tax, 10.00 fees and 23.50 commissions paid.
  expect(cells(accounts, "Long term")).toEqual(["+$93.00", "-$46.25", "+$46.75"]);
  // Trading's commissions are the months' 4.20, so the rows add up to the tiles' figures.
  expect(cells(accounts, "Trading")).toEqual(["+$7.00", "-$5.25", "+$1.75"]);
  expect(cells(accounts, "Total")).toEqual(["+$100.00", "-$51.50", "+$48.50"]);
  const added = (pick: (a: (typeof p.income.byAccount)[number]) => number) => p.income.byAccount.reduce((n, a) => n + pick(a), 0);
  expect(added((a) => a.dividends + a.interest)).toBe(10_000);
  expect(added((a) => a.tax + a.fees + (a.commissions ?? 0))).toBe(-5_150);
  expect(tile("Net")).toContain("+$48.50");
  // Only the account whose commissions are one figure for its statement has a dated line for them.
  expect(screen.getByText(/^Long term · Commissions on trades -\$23\.50/)).toBeTruthy();
  expect(screen.queryByText(/^Trading · Commissions on trades/)).toBeNull();
  const holdings = within(panel("By holding")).getByRole("table");
  expect(within(holdings).getAllByRole("columnheader").map((h) => h.textContent).filter(Boolean)).toEqual(["Holding", "Dividends", "Tax withheld"]);
  expect(cells(holdings, "World fund · VWRA")).toEqual(["+$4.00", "-$0.60"]);
  expect(cells(holdings, "Nvidia · NVDA")).toEqual(["+$1.00", "-$0.15"]);
  expect(screen.getByText("No income or costs reported: Pension, Euro funds.")).toBeTruthy();
});
