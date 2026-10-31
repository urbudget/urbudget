import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { compactMoney, heatLevels, niceTicks, squarify } from "./scale";
import { ColumnChart, Donut } from "./Charts";
import { money, shiftDate } from "../format";
import { today } from "../types";

const mocked = vi.hoisted(() => ({ read: vi.fn(), exportReports: vi.fn(), reveal: vi.fn() }));
vi.mock("../bridge", () => ({
  read: mocked.read,
  api: { exportReports: mocked.exportReports, reveal: mocked.reveal },
}));
import { Reports } from "../Reports";
import { foldPayees } from "../reports/Spending";
import { demoBudget } from "../demo";

afterEach(cleanup);
beforeEach(() => {
  localStorage.clear();
  mocked.read.mockReset();
});

const MONTH = {
  income: 0,
  spending: 0,
  netIncome: 0,
  assets: 0,
  debt: 0,
  netWorth: 0,
  inflow: 0,
  outflow: 0,
  categories: {},
  incomeSources: {},
  accounts: {},
};
const reportData = (extra: Record<string, unknown> = {}) => ({
  months: [{ month: "2026-10", ...MONTH }],
  categories: {},
  incomeSources: {},
  payees: {},
  days: {},
  income: 0,
  spending: 0,
  ageOfMoney: null,
  ageOfMoneyByMonth: { "2026-10": null },
  ...extra,
});

it("ticks are round, include zero and cover the data", () => {
  const ticks = niceTicks(-123_400, 9_876_500);
  expect(ticks).toContain(0);
  expect(ticks[0]).toBeLessThanOrEqual(-123_400);
  expect(ticks[ticks.length - 1]).toBeGreaterThanOrEqual(9_876_500);
  const steps = new Set(ticks.slice(1).map((t, i) => t - ticks[i]));
  expect(steps.size).toBe(1);
  expect(compactMoney(0)).toBe("0");
  expect(compactMoney(150_000)).toBe("1.5K");
  expect(compactMoney(2_500_000)).toBe("25K");
  expect(compactMoney(-1_234_500_000)).toBe("-12M");
});

it("stacks columns and shows every series with the total on focus", () => {
  render(
    <ColumnChart
      months={["2026-09", "2026-10"]}
      series={[
        { key: "a", label: "Bills", color: "red", values: [1000, 2000] },
        { key: "b", label: "Fun", color: "blue", values: [500, 0] },
      ]}
      label="Spending"
      format={money}
    />,
  );
  // Three non-zero segments; zero values draw nothing.
  expect(document.querySelectorAll("path[fill=red], path[fill=blue]")).toHaveLength(3);
  fireEvent.focus(document.querySelectorAll(".chart-hit")[0]);
  const tip = screen.getByRole("status").textContent!;
  expect(tip).toContain("Bills");
  expect(tip).toContain("Fun");
  expect(tip).toContain("15.00");
});

it("breaks spending down by group, then by category, then opens the register", async () => {
  const b = demoBudget();
  const [rent, groceries] = [b.categories.find((c) => c.id === "rent")!, b.categories.find((c) => c.id === "groceries")!];
  const categories = { [rent.id]: 2000, [groceries.id]: 1000 };
  mocked.read.mockResolvedValue(
    reportData({
      months: [{ ...MONTH, month: "2026-10", spending: 3000, netIncome: -3000, categories }],
      categories,
      spending: 3000,
    }),
  );
  const onInspect = vi.fn();
  render(<Reports budget={b} budgetId="test" month="2026-10" onInspect={onInspect} />);
  const groups = await screen.findByRole("list", { name: "Spending by group" });
  expect(within(groups).getAllByRole("listitem")[0].textContent).toContain(rent.group);
  fireEvent.click(within(groups).getByRole("button", { name: new RegExp(`^${rent.group}`) }));
  const inGroup = screen.getByRole("list", { name: `Spending in ${rent.group}` });
  fireEvent.click(within(inGroup).getByRole("button", { name: new RegExp(`^${rent.name}`) }));
  // A category lists its transactions first; the register is one more click away.
  const dialog = screen.getByRole("dialog", { name: rent.name });
  fireEvent.click(within(dialog).getByRole("button", { name: /Open in register/ }));
  expect(onInspect).toHaveBeenCalledWith(
    expect.objectContaining({
      reportQuery: expect.objectContaining({ categoryIds: new Set([rent.id]) }),
      from: expect.stringMatching(/-01$/),
      to: expect.any(String),
    }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("lays treemap tiles out in proportion, inside the box and never as slivers", () => {
  const tiles = squarify(
    [6, 6, 4, 3, 2, 2, 1].map((value) => ({ value })),
    { x: 0, y: 0, w: 600, h: 400 },
  );
  expect(tiles).toHaveLength(7);
  for (const t of tiles) {
    expect(t.w * t.h).toBeCloseTo((t.value / 24) * 600 * 400, 4);
    expect(t.x + t.w).toBeLessThanOrEqual(600 + 1e-6);
    expect(t.y + t.h).toBeLessThanOrEqual(400 + 1e-6);
  }
  expect(Math.max(...tiles.map((t) => Math.max(t.w / t.h, t.h / t.w)))).toBeLessThan(4);
});

it("shades by quintile, leaving zero and below unshaded", () => {
  const level = heatLevels([0, -5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
  expect([-5, 0, 10, 40, 60, 80, 100].map(level)).toEqual([0, 0, 1, 2, 3, 4, 5]);
  expect(heatLevels([])(10)).toBe(0);
});

it("draws a donut slice per positive part, with its share, and selects one", () => {
  const onSelect = vi.fn();
  render(
    <Donut
      slices={[
        { key: "a", label: "Rent", value: 3000, color: "red" },
        { key: "b", label: "Food", value: 1000, color: "blue" },
        { key: "c", label: "Refunds", value: -50, color: "green" },
      ]}
      label="Spending"
      format={money}
      center={{ value: "40.00", label: "Spent" }}
      onSelect={onSelect}
    />,
  );
  const slices = screen.getAllByRole("button");
  expect(slices.map((s) => s.getAttribute("aria-label"))).toEqual(["Rent: 30.00 SAR, 75.0%", "Food: 10.00 SAR, 25.0%"]);
  fireEvent.focus(slices[1]);
  expect(screen.getByRole("status").textContent).toContain("25.0%");
  fireEvent.click(slices[0]);
  expect(onSelect).toHaveBeenCalledWith("a");
});

it("keeps a large donut total inside the hole", () => {
  const slices = [{ key: "a", label: "Rent", value: 3000, color: "red" }];
  const total = (value: string) => {
    render(<Donut slices={slices} label="Spending" format={money} center={{ value, label: "Spent" }} />);
    const strong = document.querySelector(".donut-center strong") as HTMLElement;
    cleanup();
    return { size: Number.parseFloat(strong.style.fontSize), room: Number.parseFloat(strong.style.maxWidth) };
  };
  const small = total("40.00 SAR");
  expect(small.size).toBe(17);
  // It breaks before the currency and shrinks until the number fits in the hole's width.
  const large = total("1,861,797.94 SAR");
  expect(large.room).toBeLessThan(134);
  expect(large.size).toBeLessThan(17);
  expect("1,861,797.94".length * large.size * 0.6).toBeLessThanOrEqual(large.room);
});

it("puts grouped columns side by side, without a stacked total", () => {
  render(
    <ColumnChart
      grouped
      months={["2026-09", "2026-10"]}
      series={[
        { key: "i", label: "Income", color: "green", values: [5000, 6000] },
        { key: "s", label: "Spending", color: "red", values: [4000, 7000] },
      ]}
      label="Income and spending"
      format={money}
    />,
  );
  const bars = [...document.querySelectorAll("path[fill=green], path[fill=red]")];
  expect(bars).toHaveLength(4);
  fireEvent.focus(document.querySelectorAll(".chart-hit")[1]);
  const tip = screen.getByRole("status").textContent!;
  expect(tip).toContain("60.00");
  expect(tip).toContain("70.00");
  expect(tip).not.toContain("Total");
});

it("opens a payee's transactions from the payee treemap", async () => {
  mocked.read.mockResolvedValue(
    reportData({ payees: { Lulu: { amount: 3000, count: 3 }, Cafe: { amount: 1000, count: 4 } }, spending: 4000 }),
  );
  const onInspect = vi.fn();
  render(<Reports budget={demoBudget()} budgetId="test" month="2026-10" onInspect={onInspect} />);
  fireEvent.click(screen.getByRole("tab", { name: "By payee" }));
  const map = await screen.findByRole("list", { name: "Spending by payee" });
  const [first] = within(map).getAllByRole("listitem");
  expect(first.getAttribute("aria-label")).toContain("Lulu");
  expect(first.getAttribute("aria-label")).toContain("3 transactions · avg 10.00");
  // Hovering shows the figures; clicking lists the transactions, with a way into the register.
  fireEvent.focus(first);
  expect(within(map.parentElement!).getByRole("status").textContent).toContain("75.0% of spending");
  fireEvent.click(first);
  const dialog = screen.getByRole("dialog", { name: "Lulu" });
  fireEvent.click(within(dialog).getByRole("button", { name: /Open in register/ }));
  expect(onInspect).toHaveBeenCalledWith(expect.objectContaining({
    reportQuery: expect.objectContaining({ payee: "Lulu" }),
  }));
  // 40.00 over 7 transactions.
  expect(screen.getByText("Average per transaction").parentElement!.textContent).toContain("5.71");
});

it("forecasts from today's balance, known money and everyday spending", async () => {
  const b = demoBudget();
  mocked.read.mockImplementation(async (_b: unknown, command: string) =>
    command === "forecast"
      ? {
          balances: { checking: 100_000, credit: -20_000 },
          events: [
            {
              date: shiftDate(today(), 1),
              amount: -50_000,
              payee: "Landlord",
              accountId: "checking",
              peerId: null,
              categoryId: "rent",
              scheduleId: "s1",
            },
            {
              date: shiftDate(today(), 2),
              amount: -10_000,
              payee: "Card payment",
              accountId: "checking",
              peerId: "credit",
              categoryId: null,
              scheduleId: "s2",
            },
            {
              date: shiftDate(today(), 2),
              amount: 10_000,
              payee: "Card payment",
              accountId: "credit",
              peerId: "checking",
              categoryId: null,
              scheduleId: "s2",
            },
          ],
          recentSpending: 60_000,
        }
      : reportData(),
  );
  render(<Reports budget={b} budgetId="test" month="2026-10" onInspect={vi.fn()} />);
  fireEvent.click(screen.getByRole("tab", { name: "Wealth" }));
  fireEvent.click(screen.getByRole("tab", { name: "Forecast" }));
  await screen.findByText("Landlord");
  expect(mocked.read).toHaveBeenCalledWith(expect.anything(), "forecast", expect.objectContaining({ today: today() }), expect.any(AbortSignal));
  // The card payment moves money between two budget accounts, so it's left out.
  expect(screen.queryByText("Card payment")).toBeNull();
  expect(screen.getByText("Today", { selector: "dt" }).parentElement!.textContent).toContain("800.00");
  // 600.00 a month on average, less 500.00 rent spread over six months: about 516.67 for the rest.
  expect((screen.getByLabelText("Everyday spending a month") as HTMLInputElement).value).toBe("516.67 SAR");
});

it("opens every report without errors", async () => {
  const b = demoBudget();
  const months = ["2026-08", "2026-09", "2026-10"].map((month, i) => ({
    ...MONTH,
    month,
    income: 500_000,
    spending: 300_000 + i * 10_000,
    netIncome: 200_000 - i * 10_000,
    assets: 1_000_000,
    debt: 50_000,
    netWorth: 950_000 + i,
    inflow: 520_000,
    outflow: 320_000,
    categories: { rent: 200_000, groceries: 100_000 + i * 10_000 },
    incomeSources: { Employer: 500_000 },
    accounts: { checking: 700_000, savings: 300_000, credit: -50_000 },
  }));
  mocked.read.mockImplementation(async (_b: unknown, command: string) =>
    command === "forecast"
      ? { balances: { checking: 700_000 }, events: [], recentSpending: 300_000 }
      : reportData({
          months,
          categories: { rent: 600_000, groceries: 330_000 },
          incomeSources: { Employer: 1_500_000 },
          payees: { Landlord: { amount: 600_000, count: 3 } },
          days: { "2026-10-02": 12_000 },
          income: 1_500_000,
          spending: 930_000,
          ageOfMoney: 30,
          ageOfMoneyByMonth: { "2026-08": 20, "2026-09": 25, "2026-10": 30 },
          openingAccounts: { checking: 650_000, savings: 300_000, credit: -40_000 },
        }),
  );
  render(<Reports budget={b} budgetId="test" month="2026-10" onInspect={vi.fn()} />);
  const groups = within(screen.getByRole("tablist", { name: "Report group" })).getAllByRole("tab");
  expect(groups.map((g) => g.textContent)).toEqual(["Spending", "Income", "Wealth"]);
  let opened = 0;
  for (const group of groups) {
    fireEvent.click(group);
    const tabs = within(screen.getByRole("tablist", { name: `${group.textContent} reports` })).getAllByRole("tab");
    for (const tab of tabs) {
      fireEvent.click(tab);
      // A tab's short name ("By payee") ends the heading's full one ("Spending by payee").
      await screen.findByRole("heading", { level: 1, name: new RegExp(`${tab.textContent}$`, "i") });
      await waitFor(() => expect(document.querySelector(".report-tiles")).toBeTruthy());
      opened++;
    }
  }
  expect(opened).toBe(12);
});

it("reopens the last report in each group", () => {
  mocked.read.mockResolvedValue(reportData());
  const { unmount } = render(<Reports budget={demoBudget()} budgetId="test" month="2026-10" onInspect={vi.fn()} />);
  const tab = (name: string) => screen.getByRole("tab", { name });
  fireEvent.click(tab("Over time"));
  fireEvent.click(tab("Wealth"));
  // A group not opened before starts on its first report.
  expect(tab("Net worth").getAttribute("aria-selected")).toBe("true");
  fireEvent.click(tab("Balance sheet"));
  fireEvent.click(tab("Spending"));
  expect(tab("Over time").getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Spending over time");
  // And the next visit remembers the same.
  unmount();
  render(<Reports budget={demoBudget()} budgetId="test" month="2026-10" onInspect={vi.fn()} />);
  expect(tab("Over time").getAttribute("aria-selected")).toBe("true");
  fireEvent.click(tab("Wealth"));
  expect(tab("Balance sheet").getAttribute("aria-selected")).toBe("true");
});

it("puts an overdrawn account with what you owe and a card in credit with what you own", async () => {
  const accounts = { checking: -5_000, savings: 100_000, credit: 2_000 };
  mocked.read.mockResolvedValue(
    reportData({ months: [{ ...MONTH, month: "2026-10", accounts }], openingAccounts: accounts }),
  );
  localStorage.setItem("lb:reports:view", "balance");
  render(<Reports budget={demoBudget()} budgetId="test" month="2026-10" onInspect={vi.fn()} />);
  const owe = await screen.findByRole("list", { name: "Liabilities by type" });
  expect(owe.textContent).toContain("Overdrawn accounts");
  expect(owe.textContent).toContain("50.00");
  expect(screen.getByRole("list", { name: "Assets by type" }).textContent).toContain("Accounts in credit");
  expect(screen.getByText("Total assets", { selector: "dt" }).parentElement!.textContent).toContain("1,020.00");
  expect(screen.getByText("Net worth", { selector: "dt" }).parentElement!.textContent).toContain("970.00");
});

it("folds payees too small to read into one others tile", () => {
  const amounts = (list: { amount: number }[]) => list.map((p) => p.amount);
  // Ten big payees and forty small ones: the small ones fold.
  const list = [...Array.from({ length: 10 }, () => 1000), ...Array.from({ length: 40 }, () => 10)].map((amount) => ({ amount }));
  const { shown, rest } = foldPayees(list);
  expect(amounts(shown)).toEqual(Array(10).fill(1000));
  expect(rest).toHaveLength(40);
  // Never more than fourteen tiles, at least eight, and a single leftover keeps its own tile.
  expect(foldPayees(Array.from({ length: 30 }, () => ({ amount: 100 }))).shown).toHaveLength(14);
  expect(foldPayees(Array.from({ length: 30 }, (_, i) => ({ amount: i ? 1 : 100000 }))).shown).toHaveLength(8);
  expect(foldPayees(Array.from({ length: 9 }, () => ({ amount: 5 }))).rest).toEqual([]);
});

it("maps the top payees and points to the rest in the table", async () => {
  const payees = Object.fromEntries(
    Array.from({ length: 30 }, (_, i) => [`Payee ${i + 1}`, { amount: i < 10 ? 10_000 : 100, count: 1 }]),
  );
  mocked.read.mockResolvedValue(reportData({ payees, spending: 102_000 }));
  localStorage.setItem("lb:reports:view", "payees");
  render(<Reports budget={demoBudget()} budgetId="test" month="2026-10" onInspect={vi.fn()} />);
  const map = await screen.findByRole("list", { name: "Spending by payee" });
  // Only the ten payees big enough to read get tiles; no tile stands for the rest.
  expect(within(map).getAllByRole("listitem")).toHaveLength(10);
  expect(screen.getByText("Top 10 payees")).toBeTruthy();
  const scroll = vi.fn();
  const table = screen.getByRole("table");
  table.closest(".scroll-anchor")!.scrollIntoView = scroll;
  fireEvent.click(screen.getByRole("button", { name: /20 more payees · 20\.00/ }));
  expect(scroll).toHaveBeenCalled();
  // Everyone remains in the accessible table, while only the visible rows mount.
  expect(table.getAttribute("aria-rowcount")).toBe("31");
  expect(within(table).getAllByRole("row").length).toBeLessThan(31);
  fireEvent.scroll(screen.getByRole("region", { name: "Payee spending rows" }), { target: { scrollTop: 30 * 36 } });
  fireEvent.click(within(table).getByRole("button", { name: "Payee 30" }));
  expect(screen.getByRole("dialog", { name: "Payee 30" })).toBeTruthy();
});
