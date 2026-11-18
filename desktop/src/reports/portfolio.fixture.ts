import type { Budget, Holdings, Portfolio } from "../types";
import type { ReportData } from "./shared";

// A budget with two investment accounts and the engine's report on them, for the report tests:
// Broker (Interactive Brokers, holding four kinds, cash and accrued income) and Pension (a
// positions file, joined in October 2026), over September to November 2026. Idle fund is typed
// as an investment and nothing fills it. Written to the wire shape in docs/plans/holdings-in-reports.md.

const held = (extra: Partial<Holdings> & Pick<Holdings, "accountId" | "source">): Holdings => ({
  sourceAccount: "",
  asOf: "2026-11-17",
  currency: "USD",
  rate: "1",
  total: 0,
  positions: [],
  cash: [],
  values: [],
  syncedAt: "2026-11-17T08:00:00.000Z",
  ...extra,
});

export const investingBudget = (): Budget => ({
  schemaVersion: 2,
  name: "Test",
  currency: "USD",
  assignments: [],
  categories: [],
  transactions: [],
  accounts: [
    { id: "checking", name: "Checking", kind: "cash", archived: false, openingBalance: 0 },
    { id: "broker", name: "Broker", kind: "tracking", accountType: "investment", archived: false, openingBalance: 0 },
    { id: "pension", name: "Pension", kind: "tracking", accountType: "retirement", archived: false, openingBalance: 0 },
    { id: "idle", name: "Idle fund", kind: "tracking", accountType: "investment", archived: false, openingBalance: 0 },
    { id: "house", name: "House", kind: "tracking", accountType: "property", archived: false, openingBalance: 0 },
    { id: "card", name: "Card", kind: "credit", archived: false, openingBalance: 0 },
  ],
  investments: [
    held({
      accountId: "broker",
      source: "ibkr",
      sourceAccount: "U1234567",
      total: 980_000,
    }),
    held({ accountId: "pension", source: "file", asOf: "2026-11-10", total: 210_000 }),
  ],
});

export const portfolio = (): Portfolio => ({
  from: "2026-09",
  to: "2026-11",
  total: { start: 900_000, end: 1_210_000, put: 240_000, gain: 70_000, rate: 0.068, yearly: null, days: 91, realised: null },
  months: [
    { month: "2026-09", value: 930_000, gain: 10_000, put: 20_000, realised: null, byAccount: { broker: 930_000 } },
    { month: "2026-10", value: 1_150_000, gain: 20_000, put: 200_000, realised: null, byAccount: { broker: 950_000, pension: 200_000 } },
    { month: "2026-11", value: 1_210_000, gain: 40_000, put: 20_000, realised: null, byAccount: { broker: 1_000_000, pension: 210_000 } },
  ],
  accounts: [
    {
      accountId: "broker",
      source: "ibkr",
      sourceAccount: "U1234567",
      asOf: "2026-11-17",
      joined: "2025-11-14",
      start: 900_000,
      end: 1_000_000,
      put: 40_000,
      gain: 60_000,
      rate: 0.065,
      realised: null,
      estimatedDays: 2,
      missing: ["Transfers"],
      sparse: false,
      startParts: null,
      endParts: {
        month: "2026-11",
        asOf: "2026-11-17",
        byKind: [
          { key: "Stocks", value: 400_000 },
          { key: "Funds", value: 200_000 },
          { key: "Bonds", value: 250_000 },
          { key: "Other holdings", value: 25_000 },
        ],
        cash: 100_000,
        accrued: 5_000,
        rest: 20_000,
      },
    },
    {
      accountId: "pension",
      source: "file",
      sourceAccount: "",
      asOf: "2026-11-10",
      joined: "2026-10-05",
      start: 0,
      end: 210_000,
      put: 200_000,
      gain: 10_000,
      rate: 0.05,
      realised: null,
      estimatedDays: 0,
      missing: [],
      sparse: true,
      startParts: null,
      endParts: { month: "2026-11", asOf: "2026-11-10", byKind: [{ key: "Funds", value: 210_000 }], cash: 0, accrued: 0, rest: 0 },
    },
  ],
  allocation: {
    asOf: "2026-11-10",
    total: 1_190_000,
    count: 4,
    cash: 100_000,
    accrued: 5_000,
    cost: 1_040_000,
    unrealised: 20_000,
    unrealisedRate: 0.0192,
    largestShare: 0.34454,
    topFiveShare: 0.9118,
    byKind: [
      { key: "Funds", value: 410_000, share: 0.34454 },
      { key: "Stocks", value: 400_000, share: 0.3361 },
      { key: "Bonds", value: 250_000, share: 0.2101 },
      { key: "Cash", value: 100_000, share: 0.084 },
      { key: "Other holdings", value: 25_000, share: 0.021 },
      { key: "Accrued", value: 5_000, share: 0.0042 },
    ],
    byAccount: [
      { key: "broker", value: 980_000, share: 0.82353 },
      { key: "pension", value: 210_000, share: 0.17647 },
    ],
    byCurrency: [
      { key: "USD", value: 1_090_000, share: 0.916 },
      { key: "EUR", value: 100_000, share: 0.084 },
    ],
  },
  holdings: [
    { key: "IE00BK5BQT80", symbol: "VWRA", name: "Vanguard FTSE All-World", group: "Funds", one: "Fund", currency: "USD", value: 410_000, cost: 380_000, gain: 30_000, rate: 0.0789, share: 0.34454, accounts: ["broker", "pension"] },
    { key: "US0378331005", symbol: "AAPL", name: "Apple Inc", group: "Stocks", one: "Stock", currency: "USD", value: 400_000, cost: 400_000, gain: 0, rate: 0, share: 0.3361, accounts: ["broker"] },
    { key: "US91282CFV81", symbol: "T 4 1/8 11/15/32", name: "T 4 1/8 11/15/32", group: "Bonds", one: "Bond", currency: "USD", value: 250_000, cost: 260_000, gain: -10_000, rate: -0.0385, share: 0.2101, accounts: ["broker"] },
    { key: "XYZ WT:EUR", symbol: "XYZ WT", name: "", group: "Other holdings", one: "Other", currency: "EUR", value: 25_000, cost: null, gain: null, rate: null, share: 0.021, accounts: ["broker"] },
  ],
  income: {
    months: [
      { month: "2026-09", dividends: 1_200, interest: 300, tax: -180, fees: -1_000, other: 0, commissions: null },
      { month: "2026-10", dividends: 0, interest: 310, tax: 0, fees: -1_000, other: 0, commissions: null },
      { month: "2026-11", dividends: 2_400, interest: 290, tax: -360, fees: -1_000, other: 0, commissions: null },
    ],
    dividends: 3_600,
    interest: 900,
    tax: -540,
    fees: -3_000,
    other: 0,
    yield: 0.0047,
    byHolding: [],
    byAccount: [{ accountId: "broker", dividends: 3_600, interest: 900, tax: -540, fees: -3_000, other: 0, commissions: -2_500 }],
    commissions: [{ accountId: "broker", from: "2025-11-18", to: "2026-11-17", amount: -2_500 }],
    reporting: ["broker"],
    silent: ["pension"],
  },
  unfilled: ["idle"],
});

const month = (name: string, netWorth: number, market: number | undefined, accounts: Record<string, number>) => ({
  month: name,
  income: 0,
  spending: 0,
  netIncome: 0,
  assets: netWorth + 30_000,
  debt: 30_000,
  netWorth,
  market,
  categories: {},
  incomeSources: {},
  accounts,
});
/** The report for the same months: balances that match the portfolio's, and what markets did each month. */
export const investingReport = (market = true): ReportData => ({
  months: [
    month("2026-09", 4_500_000, market ? 10_000 : undefined, { checking: 540_000, broker: 930_000, pension: 0, idle: 50_000, house: 3_000_000, card: -20_000 }),
    month("2026-10", 4_700_000, market ? 20_000 : undefined, { checking: 510_000, broker: 950_000, pension: 200_000, idle: 50_000, house: 3_000_000, card: -10_000 }),
    month("2026-11", 4_750_000, market ? 40_000 : undefined, { checking: 520_000, broker: 1_000_000, pension: 210_000, idle: 50_000, house: 3_000_000, card: -30_000 }),
  ],
  categories: {},
  incomeSources: {},
  income: 0,
  spending: 0,
  ageOfMoney: null,
  ageOfMoneyByMonth: {},
  openingAccounts: { checking: 500_000, broker: 900_000, pension: 0, idle: 50_000, house: 3_000_000, card: -20_000 },
});

/**
 * The same report from an engine that says what each holding is and what it is worth in its own
 * currency, and each account's cash by currency (section 17.1); `portfolio()` is the same from
 * one that doesn't yet.
 */
export const portfolioWithWhatsHeld = (): Portfolio => {
  const p = portfolio();
  const what: Record<string, { instrument: string; amount: string | null }> = {
    VWRA: { instrument: "ETF", amount: "4100.00" },
    AAPL: { instrument: "Stock", amount: "4000.00" },
    "T 4 1/8 11/15/32": { instrument: "Government bond", amount: "2500.00" },
    "XYZ WT": { instrument: "Right", amount: "230.00" },
  };
  p.holdings = p.holdings.map((h) => ({ ...h, ...what[h.symbol] }));
  p.accounts[0].cash = [
    { currency: "USD", amount: "800.00", value: 80_000 },
    { currency: "EUR", amount: "18400.5", value: 20_000 },
  ];
  p.accounts[1].cash = [];
  p.allocation.byCurrency = [
    { key: "USD", value: 1_090_000, share: 0.916, amount: "10900.00" },
    { key: "EUR", value: 100_000, share: 0.084, amount: "18630.5" },
  ];
  return p;
};
