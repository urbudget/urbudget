import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import type { Budget, Holdings, HoldingsPage } from "./types";
import type { IbkrReview, IbkrReviewAccount, IbkrStatus } from "./ibkr";

// The page's figures are the engine's: each test says what the engine answers.
const engine = vi.hoisted(() => ({ page: null as HoldingsPage | null, error: null as Error | null }));
vi.mock("./bridge", async (original) => {
  const real = await original<typeof import("./bridge")>();
  return {
    ...real,
    api: {
      ...real.api,
      investmentPage: vi.fn(async () => {
        if (engine.error) throw engine.error;
        return engine.page;
      }),
    },
  };
});

vi.mock("./ibkr", () => ({
  ibkr: {
    status: vi.fn(),
    connect: vi.fn(),
    fetch: vi.fn(),
    file: vi.fn(),
    disconnect: vi.fn(),
    openGuide: vi.fn(async () => {}),
    review: vi.fn(),
    apply: vi.fn(),
    import: vi.fn(),
  },
}));
// The chart measures its own width, which a test page doesn't have.
vi.mock("./charts/Charts", () => ({
  LinesChart: ({ label, series, zero }: { label: string; series: { label: string; values: number[] }[]; zero?: boolean }) => (
    <figure aria-label={label} data-zero={String(zero ?? true)}>
      {series.map((s) => (
        <span key={s.label} data-line={s.label}>
          {s.values.join(" ")}
        </span>
      ))}
    </figure>
  ),
  Legend: ({ items }: { items: { label: string }[] }) => <ul aria-label="legend">{items.map((i) => <li key={i.label}>{i.label}</li>)}</ul>,
  ColumnChart: ({ label, months, series }: { label: string; months: string[]; series: { values: number[] }[] }) => (
    <figure aria-label={label}>{months.map((m, i) => `${m}:${series.reduce((sum, s) => sum + s.values[i], 0)}`).join(" ")}</figure>
  ),
  Donut: ({ label, center }: { label: string; center: { value: string; label: string } }) => (
    <figure aria-label={label}>
      {center.value} {center.label}
    </figure>
  ),
}));
import { ibkr } from "./ibkr";
import { api } from "./bridge";
import { brokerPage } from "./holdingsPage.fixture";
import {
  FLEX_REQUEST,
  HoldingsHeader,
  IbkrSettings,
  InvestmentActionsContext,
  dailySync,
  holdingsOf,
  plain,
  staleHoldings,
  useDailyIbkrSync,
} from "./Investments";
import { TransactionsPage } from "./Register";
import { demoBudget } from "./demo";
import { configureFormat } from "./format";
import { ToastProvider } from "./ui";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  configureFormat({ currency: "USD" });
  engine.page = brokerPage();
  engine.error = null;
});

const held: Holdings = {
  accountId: "broker",
  source: "ibkr",
  sourceAccount: "U1234567",
  asOf: "2026-11-13",
  currency: "USD",
  rate: "1",
  total: 2701250,
  positions: [
    { symbol: "VWRA", name: "VANG FTSE AW USDA", kind: "STK", currency: "USD", quantity: "120.5", price: "141.9087137", value: 1710000, cost: 1500000, gain: 210000 },
    { symbol: "2222", name: "SAUDI ARABIAN OIL CO", kind: "STK", currency: "SAR", quantity: "400", price: "27", value: 288000, cost: 320000, gain: -32000 },
    { symbol: "T 4 1/4 11/15/34", name: "US TREASURY NOTE", kind: "BOND", currency: "USD", quantity: "5000", price: "98.5", value: 492500, cost: 490000, gain: 2500 },
  ],
  cash: [{ currency: "USD", amount: "820", value: 82000 }],
  other: 1250,
  values: [
    ["2026-09-30", 2422000],
    ["2026-10-13", 2500000],
    ["2026-10-30", 2550000],
    ["2026-11-12", 2690000],
    ["2026-11-13", 2701250],
  ],
  flows: [
    ["2026-10-05", 100000],
    ["2026-11-02", 40000],
  ],
  income: [
    { month: "2026-10", interest: 120, fees: -1000 },
    { month: "2026-11", dividends: 650, tax: -98 },
  ],
  commissions: { from: "2026-09-01", to: "2026-11-13", amount: -350 },
  syncedAt: "2026-11-14T06:00:00.000Z",
};
const budget = (over: Partial<Budget> = {}): Budget => {
  const b = demoBudget();
  return {
    ...b,
    currency: "USD",
    accounts: [...b.accounts, { id: "broker", name: "Brokerage", kind: "tracking", accountType: "investment", openingBalance: 0, archived: false }],
    investments: [held],
    ...over,
  };
};
const status = (over: Partial<IbkrStatus> = {}): IbkrStatus => ({
  store: "Credential Manager",
  connected: true,
  logins: [{ query: "987654", accounts: ["U1234567"], error: null }],
  accounts: [],
  fetchedAt: null,
  fromFile: false,
  ...over,
});
const found = { id: "U1234567", alias: "Long term", currency: "USD", value: 2701250, asOf: "2026-11-13", positions: 3, missing: [] as string[], rates: true, generated: "2026-11-14 06:30" };
const reviewed = (over: Partial<IbkrReviewAccount> = {}): IbkrReview => ({
  accounts: [{ ...found, accountId: null, needsRate: null, rows: [], error: null, ...over }],
  fetchedAt: "2026-11-14T06:00:00.000Z",
});

it("reads holdings and shows the broker's numbers as written", () => {
  const b = budget();
  expect(holdingsOf(b, "broker")).toBe(held);
  expect(holdingsOf(b, "nope")).toBeUndefined();
  expect(staleHoldings(held, "2026-11-16")).toBe(false);
  expect(staleHoldings(held, "2026-11-20")).toBe(true);
  // Quantities and prices are the broker's text, trimmed for reading.
  expect(plain("141.9087137")).toBe("141.9087");
  expect(plain("120.5")).toBe("120.5");
  expect(plain("1234567")).toBe("1,234,567");
  expect(plain("820", 2)).toBe("820");
  expect(plain("n/a")).toBe("n/a");
  // Client Portal's query builder takes 200 characters.
  expect(FLEX_REQUEST.length).toBeLessThanOrEqual(200);
});

const settings = vi.fn();
function Header({ start, balance = held.total, account = "broker" }: { start: Budget; balance?: number; account?: string }) {
  const [b, setB] = useState(start);
  return (
    <ToastProvider>
      <InvestmentActionsContext.Provider value={{ transform: async (work) => setB(await work(b)), settings }}>
        <HoldingsHeader budget={b} account={b.accounts.find((a) => a.id === account)!} balance={balance} />
      </InvestmentActionsContext.Provider>
    </ToastProvider>
  );
}

it("shows how the account did over a period, with the money put in left out of the gain", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  render(<Header start={budget()} />);
  const section = screen.getByRole("region", { name: "Holdings" });
  // The figures are the engine's page for this account.
  expect(await within(section).findByText("Gain over 1 year")).toBeTruthy();
  expect(api.investmentPage).toHaveBeenCalledWith(expect.anything(), "broker", expect.any(AbortSignal));
  // A year reaches back to the first valued day: 27,012.50 less 24,220.00 less 1,400.00 put in.
  expect(within(section).getByText("+$1,392.50")).toBeTruthy();
  expect(within(section).getByText("+5.5%")).toBeTruthy();
  // The chart opens on what the account is worth, on an axis fitted to it, over what it would
  // be worth with no gain since the period began: the gap at the end is the period's gain.
  const worth = within(section).getByRole("figure", { name: /The account's value from/ });
  expect(worth.getAttribute("data-zero")).toBe("false");
  expect([...worth.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"))).toEqual(["Value", "Value before gains"]);
  const last = (line: string) => Number(worth.querySelector(`[data-line="${line}"]`)!.textContent!.split(" ").pop());
  expect(last("Value")).toBe(held.total);
  expect(last("Value") - last("Value before gains")).toBe(139250);
  expect(within(section).getByRole("list", { name: "legend" }).textContent).toBe("ValueValue before gains");
  // The two figures at the top are the chart's tabs: the value is chosen to begin with, and
  // choosing the gain draws it alone and is kept for next time.
  const figures = within(within(section).getByRole("tablist", { name: "Chart" })).getAllByRole("tab");
  expect(figures.map((tab) => [tab.textContent, tab.getAttribute("aria-selected")])).toEqual([
    ["Value$27,012.50", "true"],
    ["Gain over 1 year+$1,392.50+5.5%", "false"],
  ]);
  fireEvent.click(within(section).getByRole("tab", { name: /^Gain over 1 year/ }));
  expect(within(section).queryByRole("figure", { name: /The account's value from/ })).toBeNull();
  expect(within(section).getByRole("figure", { name: /The account's gain from/ }).textContent).toMatch(/ 139250$/);
  expect(localStorage.getItem("urbudget.holdingsChart")).toBe("gain");
  // The last month starts from 13 October: 2,012.50 more, 400.00 of it a deposit. Its running
  // gain is the whole run's from that day on, less the figure on that day, so it starts at nothing.
  fireEvent.click(within(section).getByRole("tab", { name: "1M" }));
  expect(within(section).getByText("Gain over 1 month")).toBeTruthy();
  expect(within(section).getByText("+$1,612.50")).toBeTruthy();
  expect(within(section).getByRole("figure", { name: /The account's gain from/ }).textContent).toBe("0 50000 150000 161250");
  fireEvent.click(within(section).getByRole("tab", { name: "All" }));
  expect(within(section).getByText(/^Gain since /)).toBeTruthy();
  expect(within(section).getByRole("figure", { name: /The account's gain from/ })).toBeTruthy();
});

it("adds up what it holds, earned and was charged", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  render(<Header start={budget()} />);
  const section = screen.getByRole("region", { name: "Holdings" });
  await within(section).findByText("Gain over 1 year");
  // A figure by its name: the name is a <dt>, its amount and its detail the <dd>s beside it.
  const tile = (label: RegExp | string) =>
    within(section)
      .getAllByText(label)
      .find((el) => el.tagName === "DT")!
      .closest("div")!;
  // Under the chart, how the period went: the last day's change, the money put in, the best
  // and worst days, the furthest fall from a high, and how many of the days it moved it rose.
  expect(within(tile(/^Change on /)).getByText("+$112.50")).toBeTruthy();
  expect(within(tile("Money put in")).getByText("+$1,400.00")).toBeTruthy();
  expect(tile("Best day").textContent).toMatch(/^Best day\+\$/);
  expect(tile("Worst day").textContent).toMatch(/^Worst day-\$/);
  expect(tile("Largest drop").textContent).toMatch(/^Largest drop-\$[\d,.]+-\d+\.\d% · .+ to .+$/);
  expect(tile("Days up").textContent).toMatch(/^Days up\d+%\d+ of \d+ days$/);
  // Beside the ring, what is held: the gain against the cost, the cash, the largest holding,
  // and the best and worst of them.
  expect(within(tile("Gain on what you hold")).getByText("+$1,805.00")).toBeTruthy();
  expect(tile("Gain on what you hold").textContent).toContain("+7.8% on a cost of $23,100.00");
  expect(tile("Cash").textContent).toContain("$820.00");
  expect(tile("Cash").textContent).toContain("3% of the account");
  expect(tile("Largest holding").textContent).toBe("Largest holding63%VANG FTSE AW USDA");
  expect(tile("Best performer").textContent).toBe("Best performer+14.0%VANG FTSE AW USDA");
  expect(tile("Worst performer").textContent).toContain("SAUDI ARABIAN OIL CO");
  expect(within(section).getByRole("figure", { name: "What the account is made of, by kind" }).textContent).toBe("78% Stocks");
  // Over their lines, what it earned and was charged, each as a share of the account.
  expect(within(tile("Dividends and interest")).getByText("+$7.70")).toBeTruthy();
  expect(within(tile("Fees, tax and commissions")).getByText("-$14.48")).toBeTruthy();
  expect(tile("Fees, tax and commissions").textContent).toContain("0.1% of the account");
  // And the gain month by month, which adds up to the gain since the first valued day.
  const months = within(section).getByRole("figure", { name: "The account's gain in each month" }).textContent!.split(" ");
  expect(months.length).toBeGreaterThan(1);
  expect(months[months.length - 1]).toMatch(/^2026-11:/);
  // What it's made of, and each line of income and cost.
  expect(within(section).getByText("Stocks").textContent).toContain("78%");
  expect(within(section).getByText("Bonds").textContent).toContain("19%");
  fireEvent.click(within(section).getByText("Income and costs"));
  expect(within(section).getByText("Commissions on trades").closest("tr")!.textContent).toContain("-$3.50");
  expect(within(section).getByText("Tax withheld").closest("tr")!.textContent).toContain("-$0.98");
});

it("says what a statement that leaves sections out costs the page", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  render(<Header start={budget()} />);
  await screen.findByText("Gain over 1 year");
  expect(screen.queryByText(/leaves some sections out/)).toBeNull();
  cleanup();
  engine.page = { ...brokerPage(), missing: ["Account Information", "Open Positions", "Transfers"] };
  render(<Header start={budget()} />);
  const notice = (await screen.findByText(/^Your Flex Query leaves some sections out/)).closest("div")!;
  // One line for each section that changes what is shown; the account's own details don't.
  expect(within(notice).getAllByRole("listitem").map((li) => li.textContent!.split(":")[0])).toEqual(["Open Positions", "Transfers"]);
});

it("lists the holdings, sorted by what's asked, with each one's kind, share and return", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  render(<Header start={budget()} />);
  const section = screen.getByRole("region", { name: "Holdings" });
  await within(section).findByText("What it holds");
  const table = within(section).getAllByRole("table")[0];
  const names = () => within(table).getAllByRole("row").slice(1, 4).map((r) => r.querySelector(".holdings-name")!.textContent);
  expect(names()).toEqual(["VANG FTSE AW USDA", "US TREASURY NOTE", "SAUDI ARABIAN OIL CO"]);
  const detail = (text: string) => within(table).getByText((_, el) => el?.tagName === "SMALL" && el.textContent!.replace(/\s+/g, " ").trim() === text);
  expect(detail("Stock · VWRA · 120.5 × 141.9087 USD")).toBeTruthy();
  expect(within(table).getByText(/^Bond ·/)).toBeTruthy();
  expect(within(table).getByText("63%")).toBeTruthy();
  expect(within(table).getByText("+14.0%")).toBeTruthy();
  // A loss is shown as one.
  expect(within(table).getByText("-$320.00").closest("td")!.className).toContain("negative");
  fireEvent.click(within(table).getByRole("button", { name: /^Gain/ }));
  expect(names()).toEqual(["VANG FTSE AW USDA", "US TREASURY NOTE", "SAUDI ARABIAN OIL CO"]);
  fireEvent.click(within(table).getByRole("button", { name: /^Gain/ }));
  expect(names()).toEqual(["SAUDI ARABIAN OIL CO", "US TREASURY NOTE", "VANG FTSE AW USDA"]);
  fireEvent.click(within(table).getByRole("button", { name: /^Holding/ }));
  expect(names()).toEqual(["SAUDI ARABIAN OIL CO", "US TREASURY NOTE", "VANG FTSE AW USDA"]);
  // Each holding wears its kind's mark: a share's short ticker itself.
  expect([...table.querySelectorAll(".holdings-badge.ticker")].map((badge) => badge.textContent)).toContain("VWRA");
  // More than ten fold away until asked for.
  cleanup();
  const many = Array.from({ length: 12 }, (_, i) => ({ ...brokerPage().holdings[0], symbol: `S${i}`, name: `Holding ${i}`, value: 100000 - i }));
  engine.page = { ...brokerPage(), holdings: many };
  render(<Header start={budget()} />);
  await screen.findByText("Holding 9");
  const long = within(screen.getAllByRole("table")[0]);
  expect(long.getByText("Holding 9")).toBeTruthy();
  expect(long.queryByText("Holding 10")).toBeNull();
  fireEvent.click(long.getByRole("button", { name: "Show 2 more holdings" }));
  expect(long.getByText("Holding 11")).toBeTruthy();
  fireEvent.click(long.getByRole("button", { name: "Show fewer" }));
  expect(long.queryByText("Holding 11")).toBeNull();
});

function Page({ start, account = "broker", initialFilter }: { start: Budget; account?: string; initialFilter?: { payee: string } }) {
  const found = start.accounts.find((a) => a.id === account)!;
  return (
    <ToastProvider>
      <InvestmentActionsContext.Provider value={{ transform: vi.fn(), settings }}>
        <TransactionsPage
          budget={start}
          accountId={found.id}
          account={found}
          summary={{ id: found.id, balance: held.total, cleared: held.total, uncleared: 0 }}
          netWorth={0}
          edit={vi.fn()}
          transform={vi.fn()}
          initialFilter={initialFilter}
          onEditAccount={vi.fn()}
          onReconcile={vi.fn()}
          onImport={vi.fn()}
          onSync={vi.fn()}
          onEdit={vi.fn()}
          onAddDialog={vi.fn()}
        />
      </InvestmentActionsContext.Provider>
    </ToastProvider>
  );
}

it("says what each holding is where the engine names it more exactly", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  const page = brokerPage();
  page.holdings = page.holdings.map((h, i) => (i === 0 ? { ...h, instrument: "ETF", amount: "17100.00" } : h));
  engine.page = page;
  render(<Header start={budget()} />);
  const table = within(await screen.findByRole("region", { name: "Holdings" })).getAllByRole("table")[0];
  await within(table).findByText("VANG FTSE AW USDA");
  const detail = (text: string) => within(table).queryByText((_, el) => el?.tagName === "SMALL" && el.textContent!.replace(/\s+/g, " ").trim() === text);
  expect(detail("ETF · VWRA · 120.5 × 141.9087 USD")).toBeTruthy();
  // The others, from before the engine named them, are still one of their kind.
  expect(detail("Stock · 2222 · 400 × 27 SAR")).toBeTruthy();
});

it("opens an investment account on its holdings, in a view that scrolls, with its transactions a tab away", () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  const { container } = render(<Page start={budget()} />);
  // The holdings are a page of their own. They are never in the register's pinned header,
  // which stays put while the rows scroll: there, anything below the window can't be reached.
  const holdings = screen.getByRole("region", { name: "Holdings" });
  expect(holdings.closest(".holdings-page")).toBeTruthy();
  expect(container.querySelector(".register-pinned")).toBeNull();
  expect(screen.queryByRole("grid", { name: "Transactions" })).toBeNull();
  expect(screen.getByRole("tab", { name: "Holdings" }).getAttribute("aria-selected")).toBe("true");
  // The balances and the account's buttons stay on both views.
  expect(screen.getByText("Working balance")).toBeTruthy();
  expect(screen.getByRole("button", { name: /Reconcile/ })).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "Transactions" }));
  expect(screen.getByRole("grid", { name: "Transactions" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Holdings" })).toBeNull();
  expect(container.querySelector(".register-pinned .holdings-view")).toBeNull();
  expect(screen.getByText("Working balance")).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "Holdings" }));
  expect(screen.getByRole("region", { name: "Holdings" })).toBeTruthy();
});

it("opens on the transactions when sent to particular ones, and leaves other accounts as they were", () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  render(<Page start={budget()} initialFilter={{ payee: "Market change" }} />);
  expect(screen.getByRole("grid", { name: "Transactions" })).toBeTruthy();
  expect(screen.getByRole("tab", { name: "Transactions" }).getAttribute("aria-selected")).toBe("true");
  cleanup();
  // An account no broker fills has no views to choose between: just its register.
  const plain = budget();
  render(<Page start={plain} account={plain.accounts.find((a) => a.id !== "broker")!.id} />);
  expect(screen.queryByRole("tab", { name: "Holdings" })).toBeNull();
  expect(screen.getByRole("grid", { name: "Transactions" })).toBeTruthy();
  cleanup();
  // An investment account nothing fills yet keeps its register, with the offer over it.
  render(<Page start={budget({ investments: [] })} />);
  expect(screen.queryByRole("tab", { name: "Holdings" })).toBeNull();
  expect(screen.getByText("See what this account holds and how it's doing")).toBeTruthy();
  expect(screen.getByRole("grid", { name: "Transactions" })).toBeTruthy();
});

it("syncs from the account's page, and a sync that brings nothing new changes nothing", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  vi.mocked(ibkr.fetch).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.apply).mockImplementation(async (b) => b);
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ accountId: "broker" }));
  render(<Header start={budget()} />);
  fireEvent.click(await screen.findByRole("button", { name: "Sync" }));
  await waitFor(() => expect(ibkr.apply).toHaveBeenCalledTimes(1));
  expect(ibkr.fetch).toHaveBeenCalledTimes(1);
  expect(vi.mocked(ibkr.apply).mock.calls[0][1]).toEqual([]);
  expect(await screen.findByText("Brokerage is up to date.")).toBeTruthy();
});

it("puts the deposits a sync found first, and says what reviewing them changes", async () => {
  const row = { id: "ibkr:U1234567:9000001", date: "2026-10-05", payee: "Interactive Brokers", memo: "", amount: 100000, duplicate: false, candidates: [] };
  vi.mocked(ibkr.status).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ accountId: "broker", rows: [row, { ...row, id: "old", duplicate: true }] as IbkrReviewAccount["rows"] }));
  render(<Header start={budget()} />);
  const review = await screen.findByRole("button", { name: "Review 1 deposit" });
  expect(review.className).toContain("primary");
  expect(screen.getByText(/isn't in your budget yet/)).toBeTruthy();
});

/** A purchase in `account` on `date`, or a transfer to `to`. */
const spent = (account: string, date: string, to: string | null = null): Budget["transactions"][number] => ({
  id: `spent:${account}:${date}:${to}`,
  date,
  accountId: account,
  payee: "Shop",
  memo: "",
  amount: -1000,
  status: "cleared",
  splits: [{ categoryId: null, amount: -1000 }],
  transferAccountId: to,
  transferStatus: to ? "cleared" : null,
});

it("offers as where a deposit came from only accounts whose history reaches back to it", async () => {
  // Deposits on 5 and 25 October. Everyday's history starts on 20 October: before that its
  // opening balance already has the money, so the first deposit can't be said to come from it.
  const row = { id: "ibkr:U1234567:9000001", date: "2026-10-05", payee: "Interactive Brokers", memo: "", amount: 100000, duplicate: false, candidates: [] };
  const later = { ...row, id: "ibkr:U1234567:9000002", date: "2026-10-25" };
  vi.mocked(ibkr.status).mockResolvedValue(status({ connected: false }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ accountId: "broker", rows: [row, later] as IbkrReviewAccount["rows"] }));
  const b = budget();
  const like = b.accounts.find((a) => a.id === "broker")!;
  b.accounts = [
    ...b.accounts,
    { ...like, id: "bank", name: "Everyday", kind: "cash" },
    { ...like, id: "saver", name: "Saver", kind: "cash" },
    { ...like, id: "new", name: "Opened today", kind: "cash" },
  ];
  // Saver's only transaction is the other side of a transfer, in September: that counts.
  b.transactions = [...b.transactions, spent("bank", "2026-10-20"), spent("broker", "2026-09-10", "saver")];
  render(<Header start={b} />);
  fireEvent.click(await screen.findByRole("button", { name: "Review 2 deposits" }));
  const dialog = await screen.findByRole("dialog");
  const mine = ["Everyday", "Saver", "Opened today"];
  const offered = (n: number) => {
    fireEvent.click(within(dialog).getByRole("combobox", { name: `Import action ${n}` }));
    const names = screen
      .getAllByRole("option")
      .map((o) => o.textContent!)
      .filter((name) => mine.some((account) => name === `Add as a transfer from ${account}`));
    fireEvent.keyDown(document, { key: "Escape" });
    return names;
  };
  // Before Everyday's first transaction, only Saver; after it, both; never the account with none.
  expect(offered(1)).toEqual(["Add as a transfer from Saver"]);
  expect(offered(2)).toEqual(["Add as a transfer from Saver", "Add as a transfer from Everyday"]);
});

it("offers the review from what the budget keeps, with no statement this session", async () => {
  // Another device synced: this one has no statement, and may have no token either.
  const row = { id: "ibkr:U1234567:9000001", date: "2026-10-05", payee: "Interactive Brokers", memo: "", amount: 100000, duplicate: false, candidates: [] };
  vi.mocked(ibkr.status).mockResolvedValue(status({ connected: false }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ accountId: "broker", rows: [row] as IbkrReviewAccount["rows"] }));
  const b = budget();
  b.accounts = [...b.accounts, { ...b.accounts.find((a) => a.id === "broker")!, id: "bank", name: "Everyday", kind: "cash" }];
  b.transactions = [...b.transactions, spent("bank", "2026-09-01")];
  render(<Header start={b} />);
  fireEvent.click(await screen.findByRole("button", { name: "Review 1 deposit" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(/joined to its other side in the account it came from/)).toBeTruthy();
  // Where it came from is the user's to say: every other account with history by then is offered.
  const action = within(dialog).getByRole("combobox", { name: "Import action 1" }) as HTMLInputElement;
  expect(action.value).toBe("Add with no other account");
  fireEvent.click(action);
  fireEvent.click(screen.getByRole("option", { name: "Add as a transfer from Everyday" }));
  vi.mocked(ibkr.import).mockImplementation(async (budget) => budget);
  fireEvent.click(within(dialog).getByRole("button", { name: "Import 1 transaction" }));
  await waitFor(() => expect(ibkr.import).toHaveBeenCalledTimes(1));
  expect(vi.mocked(ibkr.import).mock.calls[0].slice(1)).toEqual(["broker", [{ row: { ...row, transferAccountId: "bank" }, action: "new", matchId: null }]]);
});

it("says when the balance has drifted from the broker's value, and when prices are old", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  render(<Header start={budget()} balance={held.total + 5000} />);
  expect(screen.getByText(/The balance is \$50\.00 away from Interactive Brokers' value/)).toBeTruthy();
  // Days the value jumped with nothing listed are said to be an estimate, with what fixes it
  // and which days they were.
  await screen.findByText("Gain over 1 year");
  expect(screen.queryByText(/counted as money moved/)).toBeNull();
  cleanup();
  engine.page = { ...brokerPage(), estimated: [{ date: "2026-10-05", amount: 100000 }, { date: "2026-11-02", amount: 40000 }] };
  render(<Header start={budget()} />);
  expect(await screen.findByText(/^On 2 days the value jumped .* Add the Transfers section/)).toBeTruthy();
  const days = screen.getByText("The days counted as money moved").closest("details")!;
  expect(within(days).getAllByRole("row").map((r) => r.textContent)).toEqual([expect.stringContaining("+$1,000.00"), expect.stringContaining("+$400.00")]);
  cleanup();
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 25));
  render(<Header start={budget()} />);
  expect(screen.getByText(/These prices are a few days old/)).toBeTruthy();
});

it("waits for the page, says when it can't be read, and shows an account never valued without a chart", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status());
  let answer!: (page: HoldingsPage) => void;
  vi.mocked(api.investmentPage).mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)));
  render(<Header start={budget()} />);
  const section = screen.getByRole("region", { name: "Holdings" });
  // The account's own line is there at once; the figures follow.
  expect(await within(section).findByRole("button", { name: "Sync" })).toBeTruthy();
  expect(within(section).queryByText("Value")).toBeNull();
  await act(async () => answer(brokerPage()));
  expect(within(section).getByText("Gain over 1 year")).toBeTruthy();
  cleanup();

  engine.error = new Error("The engine is busy.");
  render(<Header start={budget()} />);
  expect((await screen.findByRole("alert")).textContent).toContain("The engine is busy.");
  engine.error = null;
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Gain over 1 year")).toBeTruthy();
  cleanup();

  // Before the first value there are no periods: the value, and nothing to chart.
  engine.page = { ...brokerPage(), days: [], values: [], gains: [], periods: [], dayChange: null, monthly: [] };
  render(<Header start={budget()} />);
  const empty = screen.getByRole("region", { name: "Holdings" });
  expect(await within(empty).findByText("What it holds")).toBeTruthy();
  expect(within(empty).getByText("$27,012.50")).toBeTruthy();
  expect(within(empty).queryByText(/^Gain over/)).toBeNull();
  expect(within(empty).queryByRole("tablist", { name: "Chart" })).toBeNull();
  expect(within(empty).queryByRole("figure", { name: /The account's value from/ })).toBeNull();
  expect(within(empty).queryByText("Money put in")).toBeNull();
});

it("offers the connection on an investment account nothing fills, until it's turned down", () => {
  const b = budget({ investments: [] });
  render(<Header start={b} />);
  expect(screen.queryByRole("region", { name: "Holdings" })).toBeNull();
  expect(screen.getByText("See what this account holds and how it's doing")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Interactive Brokers" }));
  expect(settings).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Not now" }));
  expect(screen.queryByText("See what this account holds and how it's doing")).toBeNull();
  // Turned down once, it stays away; and other kinds of account never see it.
  cleanup();
  render(<Header start={b} />);
  expect(screen.queryByText("See what this account holds and how it's doing")).toBeNull();
  localStorage.clear();
  cleanup();
  const other = b.accounts.find((a) => a.id !== "broker")!;
  render(<Header start={b} account={other.id} />);
  expect(screen.queryByText("See what this account holds and how it's doing")).toBeNull();
});

function Settings({ start }: { start: Budget }) {
  const [b, setB] = useState(start);
  return (
    <ToastProvider>
      <IbkrSettings budget={b} transform={async (work) => setB(await work(b))} />
    </ToastProvider>
  );
}

it("walks through the setup, connects with a query and a token, then adds the account in one step", async () => {
  const empty = budget({ investments: [] });
  const copy = vi.fn(async () => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText: copy }, configurable: true });
  vi.mocked(ibkr.status).mockResolvedValue(status({ connected: false }));
  vi.mocked(ibkr.connect).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed());
  vi.mocked(ibkr.apply).mockImplementation(async (b, links) => ({
    ...b,
    accounts: [...b.accounts, { id: links![0].account, name: "Long term", kind: "tracking", accountType: "investment", openingBalance: 0, archived: false }],
    investments: [{ ...held, accountId: links![0].account }],
  }));
  render(<Settings start={empty} />);
  const connect = await screen.findByRole("button", { name: "Connect" });
  expect((connect as HTMLButtonElement).disabled).toBe(true);
  // The three steps, with the request to paste into Client Portal's own query builder.
  for (const step of ["Make a Flex Query", "Make a token", "Paste both here"]) expect(screen.getByText(step)).toBeTruthy();
  expect(screen.getByText(FLEX_REQUEST)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Copy" }));
  await waitFor(() => expect(copy).toHaveBeenCalledWith(FLEX_REQUEST));
  expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();
  // By hand: numbered steps, the sections as a list in Client Portal's own words.
  const byHand = screen.getByText("Or set it up by hand").closest("details")!;
  const steps = byHand.querySelector("ol")!;
  expect([...steps.children].map((li) => li.firstChild!.textContent)).toEqual([
    "Give the query a name, such as UrBudget.",
    "Under Sections, tick each of these and choose Select All in it:",
    "Under Delivery Configuration, set Format to XML and Period to Last 365 Calendar Days.",
    "Under General Configuration, set Include Currency Rates to Yes.",
    "Save it with Continue, then Create.",
  ]);
  expect([...steps.querySelectorAll("ul li")].map((li) => li.textContent)).toEqual([
    "Account Information", "Cash Report", "Cash Transactions", "Net Asset Value (NAV) in Base",
    "Open Positions, choosing Summary", "Trades, choosing Executions", "Transfers",
  ]);
  // Where the Query ID is, at the step that asks for it.
  expect(screen.getByText(/the info \(i\) icon beside your query opens a box with the Query ID at the top/)).toBeTruthy();
  expect(screen.getByText("How it works")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Query ID"), { target: { value: "987654" } });
  fireEvent.change(screen.getByLabelText("Token"), { target: { value: "12345678901234567890" } });
  fireEvent.click(connect);
  // The account it found, going into a new investment account unless told otherwise.
  const choice = (await screen.findByLabelText("Account for Long term")) as HTMLSelectElement;
  expect(ibkr.connect).toHaveBeenCalledWith("12345678901234567890", "987654");
  expect(choice.value).toBe("new");
  expect(within(choice).getByText("Brokerage")).toBeTruthy();
  expect(screen.getByText(/3 holdings/)).toBeTruthy();
  // What the statement brought, each part ticked.
  const includes = screen.getByRole("list", { name: "What the statement includes" });
  expect(within(includes).getAllByRole("listitem").map((li) => li.className)).toEqual(["there", "there", "there", "there", "there", "there", "there"]);
  expect(within(includes).getByText("Trades").className).toBe("there");
  // And when Interactive Brokers made it, since it can send the same one again for a while.
  expect(screen.getByText(/made this statement on/).textContent).toContain("2026-11-14 06:30");
  vi.mocked(ibkr.review).mockImplementation(async (b) => reviewed({ accountId: b.investments?.[0]?.accountId ?? null }));
  vi.mocked(ibkr.status).mockResolvedValue(status({ accounts: [found] }));
  fireEvent.click(screen.getByRole("button", { name: "Add to my budget" }));
  await waitFor(() => expect(ibkr.apply).toHaveBeenCalledTimes(1));
  const [link] = vi.mocked(ibkr.apply).mock.calls[0][1]!;
  expect(link).toMatchObject({ source: "U1234567", create: true });
  // The statement from connecting is the one brought in: Interactive Brokers isn't asked twice.
  expect(ibkr.fetch).not.toHaveBeenCalled();
  expect(await screen.findByText("Added 1 account from Interactive Brokers.")).toBeTruthy();
  expect(await screen.findByRole("button", { name: /Sync now/ })).toBeTruthy();
  // Connected, it syncs by itself once a day unless that's turned off.
  const daily = screen.getByLabelText("Sync once a day when UrBudget opens") as HTMLInputElement;
  expect(daily.checked).toBe(true);
  fireEvent.click(daily);
  expect(dailySync.on()).toBe(false);
});

it("keeps a second login beside the first, says which one failed, and removes one on its own", async () => {
  const other = { ...found, id: "U7654321", alias: "Trading" };
  const both = status({
    accounts: [found, other],
    logins: [
      { query: "987654", accounts: ["U1234567"], error: null },
      { query: "111222", accounts: ["U7654321"], error: null },
    ],
  });
  vi.mocked(ibkr.status).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ accountId: "broker" }));
  render(<Settings start={budget()} />);
  // With one login there is no list of them: only the way to add another.
  await screen.findByText("Long term");
  expect(screen.queryByRole("list", { name: "Logins" })).toBeNull();
  fireEvent.click(screen.getByText("Add another login"));
  fireEvent.change(screen.getByLabelText("Query ID"), { target: { value: "111222" } });
  fireEvent.change(screen.getByLabelText("Token"), { target: { value: "ABCDEFGHIJKLMNOPQRST" } });
  vi.mocked(ibkr.connect).mockResolvedValue(both);
  vi.mocked(ibkr.review).mockResolvedValue({ ...reviewed({ accountId: "broker" }), accounts: [...reviewed({ accountId: "broker" }).accounts, ...reviewed({ ...other }).accounts] });
  fireEvent.click(screen.getByRole("button", { name: "Connect" }));
  // Now both are listed, each by the accounts it brings and its query.
  const logins = await screen.findByRole("list", { name: "Logins" });
  expect(ibkr.connect).toHaveBeenCalledWith("ABCDEFGHIJKLMNOPQRST", "111222");
  expect([...logins.querySelectorAll("li > div")].map((login) => login.textContent)).toEqual(["Long termQuery 987654", "TradingQuery 111222"]);
  // The connection as a whole is no longer removed in one go.
  expect(screen.queryByRole("button", { name: "Remove connection" })).toBeNull();
  // A sync where the second login is refused says so on its line, and nothing claims all is well.
  const refused = { ...both, logins: [both.logins[0], { ...both.logins[1], error: "Your Flex Web Service token has expired." }] };
  vi.mocked(ibkr.fetch).mockResolvedValue(refused);
  vi.mocked(ibkr.status).mockResolvedValue(refused);
  vi.mocked(ibkr.apply).mockImplementation(async (b) => b);
  vi.mocked(ibkr.review).mockResolvedValue({
    ...reviewed({ accountId: "broker" }),
    accounts: [...reviewed({ accountId: "broker" }).accounts, ...reviewed({ ...other, accountId: "other" }).accounts],
  });
  fireEvent.click(screen.getByRole("button", { name: /Sync now|Add to my budget/ }));
  expect(await within(await screen.findByRole("list", { name: "Logins" })).findByText("Your Flex Web Service token has expired.")).toBeTruthy();
  expect(screen.queryByText("Your Interactive Brokers accounts are up to date.")).toBeNull();
  // Removing that login asks first, then forgets only it.
  fireEvent.click(within(screen.getByRole("list", { name: "Logins" })).getAllByRole("button", { name: "Remove" })[1]);
  expect(screen.getByText(/^Remove this login\?/)).toBeTruthy();
  vi.mocked(ibkr.disconnect).mockResolvedValue(status({ accounts: [found] }));
  fireEvent.click(screen.getByRole("button", { name: "Remove login" }));
  await waitFor(() => expect(ibkr.disconnect).toHaveBeenCalledWith("111222"));
  await waitFor(() => expect(screen.queryByRole("list", { name: "Logins" })).toBeNull());
  // An account the budget holds from a login that isn't here stays listed, to be seen and unlinked.
  cleanup();
  const orphan: Holdings = { ...held, accountId: "other", sourceAccount: "U7654321" };
  const two = budget({ investments: [held, orphan] });
  two.accounts = [...two.accounts, { ...two.accounts.find((a) => a.id === "broker")!, id: "other", name: "Trading" }];
  vi.mocked(ibkr.status).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ accountId: "broker" }));
  render(<Settings start={two} />);
  const row = (await screen.findByText("Its login isn't on this computer, so it doesn't update here.")).closest("tr")!;
  expect(row.textContent).toContain("U7654321");
  expect(within(row).getByRole("button", { name: "Unlink" })).toBeTruthy();
});

it("says on the account's page when its own login was refused while others synced", async () => {
  const refused = status({ accounts: [found], logins: [{ query: "987654", accounts: ["U1234567"], error: "Your Flex Web Service token has expired." }] });
  vi.mocked(ibkr.status).mockResolvedValueOnce(status()).mockResolvedValue(refused);
  vi.mocked(ibkr.fetch).mockResolvedValue(refused);
  vi.mocked(ibkr.apply).mockImplementation(async (b) => b);
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ accountId: "broker" }));
  render(<Header start={budget()} />);
  fireEvent.click(await screen.findByRole("button", { name: "Sync" }));
  expect(await screen.findByText("Your Flex Web Service token has expired.")).toBeTruthy();
  expect(screen.queryByText("Brokerage is up to date.")).toBeNull();
});

it("says which parts a query leaves out", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ missing: ["Cash Transactions", "Transfers"], rates: false }));
  render(<Settings start={budget({ investments: [] })} />);
  const includes = await screen.findByRole("list", { name: "What the statement includes" });
  expect(within(includes).getByText("Deposits, income and fees").className).toBe("left-out");
  expect(within(includes).getByText("Currency rates").className).toBe("left-out");
  expect(within(includes).getByText("Transfers").className).toBe("left-out");
  expect(within(includes).getByText("Holdings").className).toBe("there");
  expect(screen.getByText(/Your Flex Query leaves out Cash Transactions, Transfers/)).toBeTruthy();
  expect(screen.getByText(/Set Include Currency Rates to Yes/)).toBeTruthy();
});

it("lists what the budget already holds before a sync, and connects a computer without a token", async () => {
  // Synced here from another device: the accounts show, with a way to connect.
  vi.mocked(ibkr.status).mockResolvedValue(status({ connected: false }));
  vi.mocked(ibkr.connect).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ accountId: "broker" }));
  render(<Settings start={budget()} />);
  expect(await screen.findByText("U1234567")).toBeTruthy();
  expect(screen.getByText("Brokerage")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Sync now/ })).toBeNull();
  fireEvent.click(screen.getByText("Connect with a token"));
  fireEvent.change(screen.getByLabelText("Query ID"), { target: { value: "987654" } });
  fireEvent.change(screen.getByLabelText("Token"), { target: { value: "12345678901234567890" } });
  fireEvent.click(screen.getByRole("button", { name: "Connect" }));
  expect(await screen.findByRole("button", { name: /Sync now/ })).toBeTruthy();
  cleanup();
  // With a token, the same list has Sync now, which asks Interactive Brokers.
  vi.mocked(ibkr.status).mockResolvedValue(status());
  vi.mocked(ibkr.fetch).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.apply).mockImplementation(async (b) => b);
  render(<Settings start={budget()} />);
  fireEvent.click(await screen.findByRole("button", { name: /Sync now/ }));
  await waitFor(() => expect(ibkr.fetch).toHaveBeenCalledTimes(1));
  expect(await screen.findByText("Your Interactive Brokers accounts are up to date.")).toBeTruthy();
});

it("asks for an exchange rate only when the statement can't give one", async () => {
  vi.mocked(ibkr.status).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ needsRate: ["USD", "JPY"] }));
  vi.mocked(ibkr.apply).mockImplementation(async (b) => ({ ...b }));
  render(<Settings start={budget({ investments: [] })} />);
  const add = (await screen.findByRole("button", { name: "Add to my budget" })) as HTMLButtonElement;
  expect(add.disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("1 USD in JPY"), { target: { value: "150,5" } });
  expect(add.disabled).toBe(false);
  fireEvent.click(add);
  await waitFor(() => expect(ibkr.apply).toHaveBeenCalledTimes(1));
  expect(vi.mocked(ibkr.apply).mock.calls[0][1]![0]).toMatchObject({ source: "U1234567", create: true, rate: "150.5" });
});

function Daily({ start }: { start: Budget }) {
  const [b, setB] = useState(start);
  useDailyIbkrSync(b, async (work) => setB(await work(b)));
  return <output>{b.investments?.[0]?.asOf}</output>;
}

it("syncs once a day when the app opens, and only with a token and the setting on", async () => {
  vi.useFakeTimers();
  vi.mocked(ibkr.status).mockResolvedValue(status());
  vi.mocked(ibkr.fetch).mockResolvedValue(status({ accounts: [found] }));
  vi.mocked(ibkr.review).mockResolvedValue(reviewed({ accountId: "broker", asOf: "2026-11-16" }));
  vi.mocked(ibkr.apply).mockImplementation(async (b) => ({ ...b, investments: [{ ...held, asOf: "2026-11-16" }] }));
  const open = (b: Budget) =>
    render(
      <ToastProvider>
        <Daily start={b} />
      </ToastProvider>,
    );
  open(budget());
  // Not at once: the app has just opened.
  expect(ibkr.fetch).not.toHaveBeenCalled();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(4100);
  });
  expect(ibkr.fetch).toHaveBeenCalledTimes(1);
  expect(screen.getByText("2026-11-16")).toBeTruthy();
  expect(screen.getByText(/Interactive Brokers: prices from/)).toBeTruthy();
  expect(dailySync.due()).toBe(false);
  // Opened again the same day, turned off, without a token, or with nothing to sync: nothing is asked.
  const quiet = async (b: Budget) => {
    cleanup();
    open(b);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4100);
    });
  };
  await quiet(budget());
  localStorage.clear();
  dailySync.set(false);
  await quiet(budget());
  dailySync.set(true);
  await quiet(budget({ investments: [] }));
  vi.mocked(ibkr.status).mockResolvedValue(status({ connected: false }));
  await quiet(budget());
  expect(ibkr.fetch).toHaveBeenCalledTimes(1);
  // A day it couldn't connect isn't counted as tried.
  expect(dailySync.due()).toBe(true);
});
