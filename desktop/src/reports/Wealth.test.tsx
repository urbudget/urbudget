import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { configureFormat } from "../format";
import type { Budget, Portfolio } from "../types";
import type { ReportData, Scope } from "./shared";

const read = vi.hoisted(() => vi.fn());
const drawn = vi.hoisted(() => ({ columns: [] as { series: { key: string; label: string; values: number[] }[] }[] }));
vi.mock("../bridge", () => ({ read, api: {} }));
// The column chart is recorded, not drawn: its series are what these tests check.
vi.mock("../charts/Charts", async (original) => ({
  ...(await original<typeof import("../charts/Charts")>()),
  ColumnChart: (props: (typeof drawn.columns)[number]) => {
    drawn.columns.push(props);
    return null;
  },
}));
import engineBudget from "../../../shared/fixtures/investments-report.json";
import engineReport from "../../../shared/fixtures/investments-report-expected.json";
import { ASSET_SECTIONS, balanceSheet, sectionTotal, type Sheet } from "./balanceSheet";
import { investingBudget, investingReport, portfolio, portfolioWithWhatsHeld } from "./portfolio.fixture";
import { BalanceSheet, BalanceStatement, NetWorth, netWorthChanges } from "./Wealth";

beforeEach(() => {
  // The fixture's range ends in November 2026: from December it is an earlier range. Tests about
  // a range that ends this month go back to November.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 11, 15));
  configureFormat({ currency: "USD" });
  localStorage.clear();
  drawn.columns = [];
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const scope = (data: ReportData, budget: Budget = investingBudget()): Scope => ({
  data,
  budget,
  from: "2026-09",
  to: "2026-11",
  accounts: new Set(),
  someCategories: false,
  groupOf: new Map(),
  nameOf: new Map(),
  spendingGroups: [],
  colorOf: () => "var(--series-1)",
  inspect: vi.fn(),
  show: vi.fn(),
});
const tile = (label: string) =>
  [...document.querySelectorAll(".report-tiles > div")].find((node) => node.querySelector("dt")?.textContent === label);
/** A statement row's cells as text, by the row's name. */
const cells = (label: string) => {
  const row = screen.getAllByRole("row").find((r) => r.querySelector("td, th")?.textContent === label);
  return row ? [...row.querySelectorAll("td")].slice(1, 5).map((cell) => cell.textContent) : null;
};
const sheetOf = (p: Portfolio | null, data = investingReport()) => balanceSheet(data, investingBudget().accounts, new Set(), p);
const rows = (sheet: Sheet, section: string) =>
  sheet.assets.filter((l) => l.section === section).map((l) => [l.a.name, l.part, l.start, l.end]);

it("splits each month's change in net worth into markets and everything else, which add up to it", () => {
  const data = investingReport();
  const changes = netWorthChanges(data, 4_430_000);
  expect(changes.map((c) => c.change)).toEqual([70_000, 200_000, 50_000]);
  expect(changes.map((c) => c.market)).toEqual([10_000, 20_000, 40_000]);
  for (const c of changes) expect(c.market + c.rest).toBe(c.change);

  render(<NetWorth scope={scope(data)} />);
  const chart = drawn.columns.find((c) => c.series.some((s) => s.key === "markets"))!;
  expect(chart.series.map((s) => s.label)).toEqual(["Markets", "Everything else"]);
  // The two series stack to the single series the chart drew before.
  expect(chart.series[0].values.map((v, i) => v + chart.series[1].values[i])).toEqual([70_000, 200_000, 50_000]);
  expect(tile("From markets")?.textContent).toContain("+$700.00");
  expect(tile("From markets")?.textContent).toContain("21.9% of the change");
  expect(screen.queryByText("Grew")).toBeNull();
});

it("leaves net worth as it was for a budget whose months have no market figure", () => {
  for (const data of [investingReport(false), { ...investingReport(), months: investingReport().months.map((m) => ({ ...m, market: 0 })) }]) {
    drawn.columns = [];
    render(<NetWorth scope={scope(data)} />);
    expect(tile("From markets")).toBeUndefined();
    expect(document.querySelectorAll(".report-tiles > div")).toHaveLength(4);
    expect(drawn.columns.some((c) => c.series.some((s) => s.key === "markets"))).toBe(false);
    const change = drawn.columns.find((c) => c.series.some((s) => s.key === "up"))!;
    expect(change.series.map((s) => s.label)).toEqual(["Grew", "Shrank"]);
    expect(screen.queryByText("Markets")).toBeNull();
    cleanup();
  }
});

it("spreads an investment account over the sections it holds, in rows that add up to its balance", () => {
  const sheet = sheetOf(portfolio());
  expect(rows(sheet, "Cash and bank")).toEqual([["Checking", "whole", 500_000, 520_000], ["Broker", "cash", null, 100_000]]);
  expect(rows(sheet, "Stocks")).toEqual([["Broker", "kind", null, 400_000]]);
  expect(rows(sheet, "Funds")).toEqual([["Broker", "kind", null, 200_000], ["Pension", "kind", 0, 210_000]]);
  expect(rows(sheet, "Bonds")).toEqual([["Broker", "kind", null, 250_000]]);
  // Kinds with no section of their own, what the balance moved since the prices, and an account nothing fills.
  expect(rows(sheet, "Other investments")).toEqual([
    ["Broker", "kind", null, 25_000],
    ["Broker", "rest", null, 20_000],
    ["Idle fund", "whole", 50_000, 50_000],
  ]);
  expect(rows(sheet, "Receivables")).toEqual([["Broker", "accrued", null, 5_000]]);
  expect(rows(sheet, "Property")).toEqual([["House", "whole", 3_000_000, 3_000_000]]);
  expect(sheet.assets.every((l) => ASSET_SECTIONS.includes(l.section))).toBe(true);
  const of = (id: string) => sheet.assets.filter((l) => l.a.id === id).reduce((n, l) => n + l.end, 0);
  expect(of("broker")).toBe(1_000_000);
  expect(of("pension")).toBe(210_000);
  expect(sheet.investments).toEqual({ change: 310_000, put: 240_000, gain: 70_000 });
});

it("keeps the totals the accounts give, however the assets are split", () => {
  const whole = sheetOf(null);
  expect(whole.totals).toEqual({ assets: { start: 4_450_000, end: 4_780_000 }, debts: { start: 20_000, end: 30_000 } });
  expect(sheetOf(portfolio()).totals).toEqual(whole.totals);
  // Unsplit, an investment account is one row among the other investments.
  expect(rows(whole, "Other investments")).toEqual([
    ["Broker", "whole", 900_000, 1_000_000],
    ["Pension", "whole", 0, 210_000],
    ["Idle fund", "whole", 50_000, 50_000],
  ]);
  expect(whole.investments).toBeNull();
  // Parts that fall short of the balance leave the difference as its own row.
  const short = portfolio();
  short.accounts[0].endParts!.rest = 0;
  const sheet = sheetOf(short);
  expect(sheet.totals).toEqual(whole.totals);
  expect(rows(sheet, "Other investments")).toContainEqual(["Broker", "rest", null, 20_000]);
});

it("shows a start only where what the account held then is known", () => {
  const unknown = sheetOf(portfolio());
  expect(unknown.startUnknown.map((a) => a.name)).toEqual(["Broker"]);
  // A section with an unknown row has no start of its own; one without still has.
  const section = (sheet: Sheet, name: string) => sectionTotal(sheet.assets.filter((l) => l.section === name));
  expect(section(unknown, "Funds")).toEqual({ start: null, end: 410_000 });
  expect(section(unknown, "Cash and bank")).toEqual({ start: null, end: 620_000 });
  expect(section(unknown, "Property")).toEqual({ start: 3_000_000, end: 3_000_000 });

  const known = portfolio();
  known.accounts[0].startParts = {
    month: "2026-08",
    asOf: "2026-08-31",
    byKind: [{ key: "Stocks", value: 500_000 }, { key: "Crypto", value: 100_000 }],
    cash: 300_000,
    accrued: 0,
    rest: 0,
  };
  const sheet = sheetOf(known);
  expect(sheet.startUnknown).toEqual([]);
  expect(rows(sheet, "Stocks")).toEqual([["Broker", "kind", 500_000, 400_000]]);
  // Sold by the end, it is still a row; bought since, a row starts at nothing.
  expect(rows(sheet, "Crypto")).toEqual([["Broker", "kind", 100_000, 0]]);
  expect(rows(sheet, "Bonds")).toEqual([["Broker", "kind", 0, 250_000]]);
  expect(section(sheet, "Cash and bank")).toEqual({ start: 800_000, end: 620_000 });
  expect(sheet.assets.filter((l) => l.a.id === "broker").reduce((n, l) => n + l.start!, 0)).toBe(900_000);
});

it("leaves an account whole when it can't be split, and on the side its balance puts it", () => {
  const p = portfolio();
  p.accounts[1].endParts = null;
  const sheet = sheetOf(p);
  expect(sheet.unsplit.map((a) => a.name)).toEqual(["Pension"]);
  expect(rows(sheet, "Other investments")).toContainEqual(["Pension", "whole", 0, 210_000]);
  expect(rows(sheet, "Funds")).toEqual([["Broker", "kind", null, 200_000]]);

  // An account whose value alone is known is one row too, with nothing to say about it.
  const typed = portfolio();
  typed.accounts[1].endParts = { month: "2026-11", asOf: "2026-11-10", byKind: [], cash: 0, accrued: 0, rest: 210_000 };
  const plain = sheetOf(typed);
  expect(rows(plain, "Other investments")).toContainEqual(["Pension", "whole", 0, 210_000]);
  expect(plain.unsplit).toEqual([]);

  // Money borrowed from the broker is a negative row among the assets.
  const margin = portfolio();
  margin.accounts[0].endParts = { ...margin.accounts[0].endParts!, cash: -100_000, rest: 220_000 };
  expect(rows(sheetOf(margin), "Cash and bank")).toContainEqual(["Broker", "cash", null, -100_000]);

  // Below zero as a whole, it is owed, in one row, as before.
  const data = investingReport();
  data.months[2].accounts.broker = -40_000;
  const owed = sheetOf(portfolio(), data);
  expect(owed.debts.map((l) => [l.a.name, l.section, l.part, l.end])).toContainEqual(["Broker", "Overdrawn accounts", "whole", 40_000]);
  expect(owed.assets.some((l) => l.a.id === "broker")).toBe(false);
});

it("draws the statement by what the money is in, leaving out what isn't known", () => {
  render(<BalanceStatement scope={scope(investingReport())} portfolio={portfolio()} />);
  expect(tile("Total assets")?.textContent).toContain("$47,800.00");
  expect(tile("Net worth")?.textContent).toContain("$47,500.00");
  // Cash at the broker counts as cash on hand.
  expect(tile("Cash on hand")?.textContent).toContain("$6,200.00");
  const bars = within(screen.getByRole("list", { name: "Assets by type" })).getAllByRole("listitem");
  expect(bars.map((b) => b.querySelector(".bar-list-label")?.textContent)).toEqual([
    "Cash and bank", "Stocks", "Funds", "Bonds", "Other investments", "Property", "Receivables",
  ]);
  expect(cells("Cash and bank")).toEqual(["", "$6,200.00", "", "13.0%"]);
  expect(cells("Cash at Broker")).toEqual(["", "$1,000.00", "", "2.1%"]);
  expect(cells("Accrued income at Broker")).toEqual(["", "$50.00", "", "0.1%"]);
  expect(cells("Since prices from 2026-11-17 · Broker")).toEqual(["", "$200.00", "", "0.4%"]);
  expect(cells("Pension")).toEqual(["$0.00", "$2,100.00", "$2,100.00", "4.4%"]);
  expect(cells("Property")).toEqual(["$30,000.00", "$30,000.00", "$0.00", "62.8%"]);
  // The totals come from the accounts, so they are whole.
  expect(cells("Total assets")).toEqual(["$44,500.00", "$47,800.00", "$3,300.00", ""]);
  expect(cells("Net worth")).toEqual(["$44,300.00", "$47,500.00", "$3,200.00", ""]);
  expect(screen.getByText("Investments changed by +$3,100.00: +$2,400.00 put in, +$700.00 from markets.")).toBeTruthy();
  expect(screen.getByText("Start and change not shown by type, because what was held then isn't kept: Broker.")).toBeTruthy();
  expect(screen.queryByText(/^Not broken down/)).toBeNull();
  expect(screen.queryByText("Investments", { selector: "td" })).toBeNull();
});

it("reads the portfolio with the balance sheet, and shows the accounts whole when it can't be split", async () => {
  const p = portfolio();
  p.accounts[1].endParts = null;
  read.mockResolvedValue(p);
  const s = scope(investingReport());
  s.accounts = new Set(["broker", "pension", "checking"]);
  render(<BalanceSheet scope={s} />);
  await screen.findByText("Statement");
  expect(read).toHaveBeenCalledWith(
    s.budget,
    "investments_report",
    { filter: { from: "2026-09", to: "2026-11", accountIds: ["broker", "pension", "checking"], categoryIds: [] } },
    expect.any(AbortSignal),
  );
  expect(cells("Pension")).toEqual(["$0.00", "$2,100.00", "$2,100.00", "12.1%"]);
  expect(screen.getByText("Not broken down, because what was held then isn't kept: Pension.")).toBeTruthy();
});

it("asks for no portfolio in a budget without holdings", () => {
  const budget = { ...investingBudget(), investments: [] };
  render(<BalanceSheet scope={scope(investingReport(false), budget)} />);
  expect(read).not.toHaveBeenCalled();
  expect(cells("Other investments")).toEqual(["$9,500.00", "$12,600.00", "$3,100.00", "26.4%"]);
  expect(cells("Broker")).toEqual(["$9,000.00", "$10,000.00", "$1,000.00", "20.9%"]);
  expect(screen.queryByText(/Investments changed by/)).toBeNull();
});

it("splits the engine's own report for its test budget into rows that add up", () => {
  const p: Portfolio = engineReport;
  const { accounts } = engineBudget.budget as unknown as Budget;
  // The balances the parts were split from: each account's value at the range's end, and its parts' sum before it.
  const before = (id: string) => {
    const parts = p.accounts.find((a) => a.accountId === id)?.startParts;
    return parts ? parts.byKind.reduce((n, k) => n + k.value, 0) + parts.cash + parts.accrued + parts.rest : 0;
  };
  const balances = (at: (id: string) => number) => Object.fromEntries(accounts.map((a) => [a.id, at(a.id)]));
  const data: ReportData = {
    ...investingReport(),
    months: [
      { ...investingReport().months[0], month: p.from, accounts: {} },
      { ...investingReport().months[2], month: p.to, accounts: balances((id) => p.accounts.find((a) => a.accountId === id)?.end ?? 0) },
    ],
    openingAccounts: balances(before),
  };
  const sheet = balanceSheet(data, accounts, new Set(), p);
  for (const a of p.accounts) {
    const lines = sheet.assets.filter((l) => l.a.id === a.accountId);
    expect(lines.reduce((n, l) => n + l.end, 0), a.accountId).toBe(a.end);
    // What is left over the parts is the engine's own figure for it.
    expect(lines.find((l) => l.part === "rest")?.end ?? 0, a.accountId).toBe(lines.some((l) => l.part === "whole") ? 0 : a.endParts!.rest);
  }
  // A value typed in is one row, as is an account nothing fills; an account that joined in the
  // range starts at nothing; a short option is a negative row.
  expect(rows(sheet, "Other investments")).toEqual([
    ["Pension", "whole", 2_000_000, 2_300_000],
    ["Euro funds", "rest", 0, 20_000],
    ["Old workplace plan", "whole", 0, 0],
  ]);
  expect(rows(sheet, "Options")).toEqual([["Long term", "kind", 0, -50_000]]);
  expect(rows(sheet, "Stocks")).toEqual([["Long term", "kind", 3_250_000, 4_100_000], ["Trading", "kind", 0, 1_130_000]]);
  expect(sheet.startUnknown).toEqual([]);
  expect(sheet.unsplit).toEqual([]);
});

/** A statement row's name and the line under it, by its name. */
const named = (label: string) => {
  const row = screen.getAllByRole("row").find((r) => r.querySelector("td")?.firstChild?.textContent === label);
  return row ? { line: row.querySelector(".sub-line")?.textContent ?? null, cells: [...row.querySelectorAll("td")].slice(1, 5).map((c) => c.textContent) } : null;
};
const now = () => vi.setSystemTime(new Date(2026, 10, 18));

it("lists today's holdings in their sections when the range ends this month", () => {
  now();
  render(<BalanceStatement scope={scope(investingReport())} portfolio={portfolioWithWhatsHeld()} />);
  // Each holding: its name, then what it is, its symbol, its own-currency amount where that isn't
  // the budget's, and which account holds it, there being two; its value and share, no start or change.
  expect(named("Vanguard FTSE All-World")).toEqual({ line: "ETF · VWRA · Broker, Pension", cells: ["", "$4,100.00", "", "8.6%"] });
  expect(named("Apple Inc")).toEqual({ line: "Stock · AAPL · Broker", cells: ["", "$4,000.00", "", "8.4%"] });
  // A bond named by its symbol isn't named twice; a holding with no name goes by its symbol.
  expect(named("T 4 1/8 11/15/32")?.line).toBe("Government bond · Broker");
  expect(named("XYZ WT")?.line).toBe("Right · 230.00 EUR · Broker");
  // They stand in for the accounts' lines of their kind; the sections add up as before.
  expect(named("Pension")).toBeNull();
  expect(named("Funds")?.cells).toEqual(["", "$4,100.00", "", "8.6%"]);
  expect(cells("Total assets")).toEqual(["$44,500.00", "$47,800.00", "$3,300.00", ""]);
  // What the holdings don't explain stays the account's, and cash at the broker says its currencies.
  expect(named("Since prices from 2026-11-17")?.cells).toEqual(["", "$200.00", "", "0.4%"]);
  expect(named("Idle fund")?.cells).toEqual(["$500.00", "$500.00", "$0.00", "1.0%"]);
  expect(named("Cash at Broker")?.line).toBe("800.00 USD · 18,400.5 EUR");
  expect(named("Checking")?.line).toBeNull();
});

it("leaves a long code out of a holding's line, where it only spells the name out again", () => {
  now();
  const p = portfolioWithWhatsHeld();
  // The bonds' 2,500.00 as a call option and the bond, in Broker.
  p.holdings = p.holdings.flatMap((h) =>
    h.group === "Bonds"
      ? [
          { ...h, value: 240_000 },
          { ...h, key: "AAPL 261218C00250000", symbol: "AAPL 261218C00250000", name: "AAPL 18DEC26 250 C", group: "Bonds", instrument: "Call option", currency: "EUR", amount: "840.00", value: 10_000 },
        ]
      : [h],
  );
  render(<BalanceStatement scope={scope(investingReport())} portfolio={p} />);
  expect(named("AAPL 18DEC26 250 C")?.line).toBe("Call option · 840.00 EUR · Broker");
  expect(named("Vanguard FTSE All-World")?.line).toBe("ETF · VWRA · Broker, Pension");
});

it("names no account for a holding when only one account holds investments, and says nothing it doesn't know", () => {
  now();
  // An engine from before 17.1: no instrument, no amount, no cash by currency.
  const old = portfolio();
  render(<BalanceStatement scope={scope(investingReport())} portfolio={old} />);
  expect(named("Vanguard FTSE All-World")?.line).toBe("Fund · VWRA · Broker, Pension");
  expect(named("XYZ WT")?.line).toBe("Other · Broker");
  expect(named("Cash at Broker")?.line).toBeNull();
  cleanup();

  const one = portfolioWithWhatsHeld();
  one.accounts = [one.accounts[0]];
  one.holdings = one.holdings.map((h) => (h.symbol === "VWRA" ? { ...h, value: 200_000, accounts: ["broker"] } : h));
  render(<BalanceStatement scope={scope(investingReport())} portfolio={one} />);
  expect(named("Vanguard FTSE All-World")).toEqual({ line: "ETF · VWRA", cells: ["", "$2,000.00", "", "4.2%"] });
  // The other account isn't split, so it is one line, as before.
  expect(named("Pension")?.cells).toEqual(["$0.00", "$2,100.00", "$2,100.00", "4.4%"]);
});

it("folds a section of nine holdings or more to six", () => {
  now();
  const p = portfolioWithWhatsHeld();
  // Broker's 4,000.00 of stocks, in ten holdings.
  const stocks = Array.from({ length: 10 }, (_, i) => ({ ...p.holdings[1], key: `S${i}`, symbol: `S${i}`, name: `Share ${i}`, value: 40_000 }));
  p.holdings = [...p.holdings.filter((h) => h.group !== "Stocks"), ...stocks];
  render(<BalanceStatement scope={scope(investingReport())} portfolio={p} />);
  const shown = () => screen.getAllByRole("row").filter((r) => /^Share \d$/.test(r.querySelector("td")?.firstChild?.textContent ?? "")).length;
  expect(shown()).toBe(6);
  fireEvent.click(screen.getByRole("button", { name: "Show all 10" }));
  expect(shown()).toBe(10);
  fireEvent.click(screen.getByRole("button", { name: "Show fewer" }));
  expect(shown()).toBe(6);
  // A section of fewer than nine shows them all, with nothing to unfold.
  expect(named("Vanguard FTSE All-World")).not.toBeNull();
  expect(screen.getAllByRole("button", { name: /^Show all/ })).toHaveLength(1);
});

it("keeps the accounts' lines for an earlier range, and where the holdings don't add up to them", () => {
  // December: the range ended last month.
  render(<BalanceStatement scope={scope(investingReport())} portfolio={portfolioWithWhatsHeld()} />);
  expect(named("Vanguard FTSE All-World")).toBeNull();
  expect(named("Pension")?.cells).toEqual(["$0.00", "$2,100.00", "$2,100.00", "4.4%"]);
  expect(named("Cash at Broker")?.line).toBeNull();
  cleanup();

  now();
  const off = portfolioWithWhatsHeld();
  off.holdings = off.holdings.map((h) => (h.symbol === "VWRA" ? { ...h, value: 400_000 } : h));
  render(<BalanceStatement scope={scope(investingReport())} portfolio={off} />);
  // Funds keeps its accounts; the other sections list their holdings.
  expect(named("Vanguard FTSE All-World")).toBeNull();
  expect(named("Pension")).not.toBeNull();
  expect(named("Apple Inc")).not.toBeNull();
});
