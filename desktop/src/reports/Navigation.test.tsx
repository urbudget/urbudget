import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { configureFormat, shiftDate, shiftMonth } from "../format";
import { today, type Budget } from "../types";

const read = vi.hoisted(() => vi.fn());
vi.mock("../bridge", () => ({ read, api: {} }));
import { Reports, placeOf } from "../Reports";
import { investingBudget, investingReport, portfolio } from "./portfolio.fixture";

/** Six months' figures for the overview: the fixture's balances, with income and spending in each month. */
const report = () => {
  const data = investingReport();
  data.months.forEach((m, i) => Object.assign(m, { income: 40_000, spending: 1_000 * (i + 1) }));
  return { ...data, income: 120_000, spending: 6_000 };
};
/** Today's balances and one bill in ten days. */
const forecast = () => ({
  balances: { checking: 520_000, card: -30_000 },
  events: [{ date: shiftDate(today(), 10), amount: -100_000, payee: "Landlord", accountId: "checking", peerId: null, categoryId: null, scheduleId: null }],
  recentSpending: 0,
});
beforeEach(() => {
  configureFormat({ currency: "USD" });
  localStorage.clear();
  sessionStorage.clear();
  read.mockImplementation(async (_budget: unknown, command: string) =>
    command === "investments_report" ? portfolio() : command === "forecast" ? forecast() : report(),
  );
});
afterEach(cleanup);

const open = (budget: Budget = investingBudget()) =>
  render(<Reports budget={budget} budgetId="test" month={today().slice(0, 7)} onInspect={vi.fn()} />);
const tab = (name: string) => screen.getByRole("tab", { name });
const selected = (name: string) => tab(name).getAttribute("aria-selected") === "true";
const views = () => within(screen.getByRole("tablist", { name: "View" })).getAllByRole("tab").map((view) => view.textContent);
const card = (name: string) => screen.getByRole("button", { name }).closest("article")!;
const filters = () => screen.getByRole("button", { name: /^Filters:/ }).getAttribute("aria-label");

it("opens on the overview, then a report from its card, with its views under its title", async () => {
  open();
  expect(selected("Overview")).toBe(true);
  expect(screen.queryByRole("heading", { level: 1 })).toBeNull();
  fireEvent.click(await screen.findByRole("button", { name: "Net worth" }));
  expect(selected("Net worth")).toBe(true);
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Net worth");
  expect(screen.getByText("What am I worth, and why did it change?")).toBeTruthy();
  expect(views()).toEqual(["Over time", "By account"]);
  expect(selected("Over time")).toBe(true);
  expect(screen.getByText("What you own minus what you owe, month by month.")).toBeTruthy();
  // A view keeps its own words under the switch.
  fireEvent.click(tab("By account"));
  expect(screen.getByText("Each account's balance at the end of every month.")).toBeTruthy();
  await screen.findByRole("heading", { level: 2, name: "All accounts" });
  // A report with one view has no switch.
  fireEvent.click(tab("Balance sheet"));
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Balance sheet");
  expect(screen.queryByRole("tablist", { name: "View" })).toBeNull();
  expect(screen.getByText("Assets and liabilities by type, with the ratios that matter.")).toBeTruthy();
});

it("returns to where the person was, and to the overview when the app starts again", () => {
  const first = open();
  fireEvent.click(tab("Income"));
  expect(views()).toEqual(["Vs. spending", "Sources", "In and out"]);
  fireEvent.click(tab("Sources"));
  first.unmount();

  // Leaving Reports and coming back, in the same run of the app.
  const again = open();
  expect(selected("Income")).toBe(true);
  expect(selected("Sources")).toBe(true);
  again.unmount();

  // The app starts again: the overview, and each report still on its last view.
  sessionStorage.clear();
  open();
  expect(selected("Overview")).toBe(true);
  fireEvent.click(tab("Income"));
  expect(selected("Sources")).toBe(true);
  fireEvent.click(tab("Spending"));
  expect(views()).toEqual(["Category", "Payee", "Over time", "Calendar"]);
  expect(selected("Category")).toBe(true);
  fireEvent.click(tab("Forecast"));
  expect(views()).toEqual(["Balances", "Cash age"]);
});

it("opens a report at the view an old report id names", () => {
  expect(placeOf("income")).toEqual({ report: "income" });
  expect(placeOf("cashflow")).toEqual({ report: "income", view: "cashflow" });
  expect(placeOf("age")).toEqual({ report: "forecast", view: "age" });
  // Before reports had views, "income" was the income sources' own report.
  expect(placeOf("income", true)).toEqual({ report: "income", view: "sources" });
  expect(placeOf("balances", true)).toEqual({ report: "networth", view: "balances" });
  expect(placeOf("balance", true)).toEqual({ report: "balance", view: "balance" });
  expect(placeOf("wealth")).toBeNull();
  expect(placeOf(null)).toBeNull();

  // What was remembered then: the last report, and the last one in each group.
  localStorage.setItem("lb:reports:view", "income");
  localStorage.setItem("lb:reports:groupViews", JSON.stringify({ Spending: "payees", Income: "cashflow", Wealth: "age", Investments: "allocation", Gone: "nothing" }));
  const first = open();
  expect(selected("Overview")).toBe(true);
  expect(JSON.parse(localStorage.getItem("lb:reports:views")!)).toEqual({ spending: "payees", income: "sources", forecast: "age", investments: "allocation" });
  expect(localStorage.getItem("lb:reports:view")).toBeNull();
  expect(localStorage.getItem("lb:reports:groupViews")).toBeNull();
  fireEvent.click(tab("Spending"));
  expect(selected("Payee")).toBe(true);
  fireEvent.click(tab("Income"));
  expect(selected("Sources")).toBe(true);
  fireEvent.click(tab("Investments"));
  expect(selected("Allocation")).toBe(true);
  fireEvent.click(tab("Forecast"));
  expect(selected("Cash age")).toBe(true);
  first.unmount();

  // A place that names a view opens its report there.
  sessionStorage.setItem("lb:reports:place", "trends");
  open();
  expect(selected("Spending")).toBe(true);
  expect(selected("Over time")).toBe(true);
  expect(JSON.parse(localStorage.getItem("lb:reports:views")!).spending).toBe("payees");
});

it("shows a card for each report over the last six months", async () => {
  const budget = investingBudget();
  open(budget);
  const month = today().slice(0, 7);
  const six = { from: shiftMonth(month, -5), to: month, accountIds: [], categoryIds: [] };
  await waitFor(() => expect(card("Forecast").textContent).toContain("Lowest balance ahead"));
  expect(read).toHaveBeenCalledWith(budget, "calculate_reports", { filter: six }, expect.any(AbortSignal));
  expect(read).toHaveBeenCalledWith(budget, "investments_report", { filter: six }, expect.any(AbortSignal));
  expect(read).toHaveBeenCalledWith(budget, "forecast", { today: today(), through: shiftDate(today(), 0, 6), accountIds: [] }, expect.any(AbortSignal));
  expect([...document.querySelectorAll(".report-card h2")].map((h) => h.textContent)).toEqual([
    "Spending", "Income", "Net worth", "Balance sheet", "Investments", "Forecast",
  ]);
  const text = (name: string) => [...card(name).querySelectorAll("p")].map((p) => p.textContent);
  expect(text("Spending")).toEqual(["Where did my money go?", "$30.00", "This month · Monthly average $20.00"]);
  expect(text("Income")).toEqual(["Am I spending less than I earn?", "+$1,140.00", "Last 6 months"]);
  expect(text("Net worth")).toEqual(["What am I worth, and why did it change?", "$47,500.00", "+$3,200.00 · Last 6 months"]);
  expect(text("Balance sheet")).toEqual(["What do I own and owe?", "$47,800.00", "Total liabilities $300.00"]);
  expect(text("Investments")).toEqual(["How are my investments doing?", "$12,100.00", "Gain +$700.00"]);
  // 4,900.00 today in the budget accounts, less the 1,000.00 bill in ten days.
  expect(text("Forecast")).toEqual(["What's coming up?", "$3,900.00", `Lowest balance ahead · ${shiftDate(today(), 10)}`]);
  // Each has its small chart: columns, lines, or the one bar of owned and owed.
  expect(card("Spending").querySelectorAll(".spark path")).toHaveLength(3);
  expect(card("Income").querySelectorAll(".spark path")).toHaveLength(6);
  expect(card("Net worth").querySelectorAll(".spark path")).toHaveLength(1);
  expect(card("Balance sheet").querySelectorAll(".split-bar i")).toHaveLength(2);
  // A card opens its report.
  fireEvent.click(screen.getByRole("button", { name: "Balance sheet" }));
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Balance sheet");
});

it("leaves the investments card out without holdings, and says what the overview leaves out", async () => {
  localStorage.setItem("lb:reports:hidden:test", JSON.stringify({ accounts: ["card"], categories: [] }));
  const budget = { ...investingBudget(), investments: [] };
  open(budget);
  await waitFor(() => expect(card("Net worth").textContent).toContain("Last 6 months"));
  expect(screen.queryByRole("button", { name: "Investments" })).toBeNull();
  expect(read.mock.calls.some((call) => call[1] === "investments_report")).toBe(false);
  // The cards cover what the reports cover: the account left out is left out here, and named.
  expect(read.mock.calls.find((call) => call[1] === "calculate_reports")![2].filter.accountIds).toEqual(["checking", "broker", "pension", "idle", "house"]);
  expect(read.mock.calls.find((call) => call[1] === "forecast")![2].accountIds).toEqual(["checking"]);
  expect(screen.getByText("Accounts left out:").parentElement!.textContent).toContain("Card");
  expect(screen.queryByRole("button", { name: /^Filters:/ })).toBeNull();
});

it("says what is in force on the one filter control", async () => {
  const budget: Budget = {
    ...investingBudget(),
    categories: ["Food", "Travel", "Rent"].map((name) => ({ id: name.toLowerCase(), name, group: "Living", target: 0 })),
  };
  sessionStorage.setItem("lb:reports:place", "categories");
  open(budget);
  await screen.findByRole("heading", { level: 2, name: "Spending by group" });
  expect(filters()).toMatch(/^Filters: .+ · All accounts$/);
  const months = filters()!.slice("Filters: ".length, -" · All accounts".length);
  // The months, accounts and categories are changed from inside it.
  fireEvent.click(screen.getByRole("button", { name: /^Filters:/ }));
  expect(screen.getByRole("button", { name: `Months: ${months}` })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /Categories: All categories/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Travel" }));
  fireEvent.keyDown(document, { key: "Escape" });
  expect(filters()).toBe(`Filters: ${months} · All accounts · 3 categories`);
  fireEvent.click(screen.getByRole("button", { name: /Accounts: All accounts/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Tracking" }));
  // Escape closes the list of accounts, then the control itself.
  fireEvent.keyDown(document, { key: "Escape" });
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("button", { name: /^Accounts:/ })).toBeNull();
  // Several accounts are counted, and which they are is still listed under the control.
  expect(filters()).toBe(`Filters: ${months} · 2 accounts · 3 categories`);
  expect(screen.getByRole("button", { name: /^Filters:/ }).className).toContain("active");
  expect(screen.getByText("Only these accounts:").parentElement!.textContent).toContain("Checking");

  // The choice carries to the other reports; a view shows only the parts that apply to it.
  fireEvent.click(tab("Income"));
  fireEvent.click(tab("Sources"));
  expect(filters()).toBe(`Filters: ${months} · 2 accounts`);
  fireEvent.click(tab("Forecast"));
  expect(filters()).toBe("Filters: All budget accounts");
  // One account left is named.
  fireEvent.click(screen.getByRole("button", { name: /^Filters:/ }));
  fireEvent.click(screen.getByRole("button", { name: /Accounts: All budget accounts/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Card" }));
  fireEvent.keyDown(document, { key: "Escape" });
  expect(filters()).toBe("Filters: Checking");
  fireEvent.click(tab("Cash age"));
  expect(filters()).toBe(`Filters: ${months} · Checking`);
  expect(screen.getByRole("button", { name: "Export CSV" })).toBeTruthy();
});

it("says how much money moved into or out of investments Spending and Income leave out", async () => {
  // 2,500.00 moved into the investment accounts over the range, counted as neither spending nor income.
  let invested = 250_000;
  read.mockImplementation(async (_budget: unknown, command: string) =>
    command === "investments_report" ? portfolio() : command === "forecast" ? forecast() : { ...report(), invested },
  );
  sessionStorage.setItem("lb:reports:place", "categories");
  const first = open();
  const line = await screen.findByText("$2,500.00 moved into investments isn't counted here.");
  // Text in the row under the filter line, not a chip: there is nothing to put back.
  const row = line.closest(".report-left-out")!;
  expect(row.querySelector(".chip")).toBeNull();
  expect(row.textContent).toBe("$2,500.00 moved into investments isn't counted here.");
  // Every view of Spending and of Income says it; the other reports don't.
  fireEvent.click(tab("Payee"));
  expect(await screen.findByText(/moved into investments/)).toBeTruthy();
  fireEvent.click(tab("Income"));
  for (const view of ["Vs. spending", "Sources", "In and out"]) {
    fireEvent.click(tab(view));
    expect(await screen.findByText("$2,500.00 moved into investments isn't counted here.")).toBeTruthy();
  }
  fireEvent.click(tab("Net worth"));
  await screen.findByRole("heading", { level: 2, name: "Change each month" });
  expect(screen.queryByText(/into investments/)).toBeNull();
  fireEvent.click(tab("Overview"));
  await waitFor(() => expect(card("Net worth").textContent).toContain("Last 6 months"));
  expect(screen.queryByText(/into investments/)).toBeNull();
  first.unmount();

  // More taken out than put in; and beside accounts left out, it joins their row.
  invested = -40_000;
  localStorage.setItem("lb:reports:hidden:test", JSON.stringify({ accounts: ["card"], categories: [] }));
  sessionStorage.setItem("lb:reports:place", "incomeExpense");
  const second = open();
  const out = await screen.findByText("$400.00 taken out of investments isn't counted here.");
  expect(out.closest(".report-left-out")!.textContent).toContain("Accounts left out:");
  second.unmount();

  // None moved: nothing said.
  invested = 0;
  open();
  await screen.findByText("Accounts left out:");
  expect(screen.queryByText(/investments isn't counted here/)).toBeNull();
});
