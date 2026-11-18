import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import type { Budget, Holdings } from "./types";
import type { PositionsFile } from "./holdings";
import { pensionPage } from "./holdingsPage.fixture";

// The page's figures are the engine's: here, its page for the value typed in.
vi.mock("./bridge", async (original) => {
  const real = await original<typeof import("./bridge")>();
  return { ...real, api: { ...real.api, investmentPage: vi.fn(async () => pensionPage()) } };
});

vi.mock("./ibkr", () => ({
  ibkr: { status: vi.fn(), connect: vi.fn(), fetch: vi.fn(), file: vi.fn(), disconnect: vi.fn(), openGuide: vi.fn(async () => {}), review: vi.fn(), apply: vi.fn(), import: vi.fn() },
}));
vi.mock("./holdings", async (original) => ({
  ...(await original<typeof import("./holdings")>()),
  holdings: { file: vi.fn(), record: vi.fn() },
  pickPositionsFile: vi.fn(),
}));
// A file's text comes through the app's own decoder; here it is read as it is.
vi.mock("./BankImport", async (original) => ({
  ...(await original<typeof import("./BankImport")>()),
  statementFile: vi.fn(async (file: File) => ({ text: await file.text(), sheets: [] })),
}));
// The chart measures its own width, which a test page doesn't have.
vi.mock("./charts/Charts", () => ({
  LinesChart: ({ label }: { label: string }) => <figure aria-label={label} />,
  Legend: () => null,
  ColumnChart: ({ label }: { label: string }) => <figure aria-label={label} />,
  Donut: ({ label }: { label: string }) => <figure aria-label={label} />,
}));
import { ibkr } from "./ibkr";
import { holdings, pickPositionsFile } from "./holdings";
import { PositionsFileDialog, TypedValueDialog } from "./HoldingsSetup";
import { HoldingsHeader, InvestmentActionsContext, InvestmentsSettings, typedValues } from "./Investments";
import { demoBudget } from "./demo";
import { configureFormat } from "./format";
import { ToastProvider } from "./ui";

afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  configureFormat({ currency: "USD" });
  vi.mocked(ibkr.status).mockResolvedValue({ store: "the test store", connected: false, logins: [], accounts: [], fetchedAt: null, fromFile: false });
});

const account = (id: string, name: string) => ({ id, name, kind: "tracking" as const, accountType: "investment", openingBalance: 0, archived: false });
const typedIn: Holdings = {
  accountId: "pension",
  source: "manual",
  sourceAccount: "",
  asOf: "2026-10-31",
  currency: "USD",
  rate: "1",
  total: 1000000,
  positions: [],
  cash: [],
  other: 0,
  values: [
    ["2026-09-30", 950000],
    ["2026-10-31", 1000000],
  ],
  syncedAt: "2026-11-01T06:00:00.000Z",
};
const budget = (investments: Holdings[] = []): Budget => {
  const b = demoBudget();
  return { ...b, currency: "USD", accounts: [...b.accounts, account("pension", "Pension"), account("isa", "Stocks ISA")], investments };
};
const file = new File(["Symbol,Value\nVWRA,17100\n"], "positions.csv", { type: "text/csv" });
const found = (over: Partial<PositionsFile> = {}): PositionsFile => ({
  format: "csv",
  asOf: "2026-11-13",
  currency: "",
  positions: [
    { symbol: "VWRA", name: "World fund", kind: "FUND", currency: "", quantity: "120.5", price: "141.91", value: "17100", cost: "15000" },
    { symbol: "T", name: "", kind: "BOND", currency: "", quantity: "", price: "", value: "4925", cost: "" },
  ],
  cash: "820",
  value: "22845",
  skipped: 0,
  ...over,
});

it("shows what a positions file holds, and keeps it for the day it says", async () => {
  const b = budget();
  const saved = vi.fn();
  vi.mocked(holdings.file).mockResolvedValue(found());
  vi.mocked(holdings.record).mockImplementation(async (budget) => budget);
  render(<PositionsFileDialog budget={b} account={b.accounts.find((a) => a.id === "pension")!} file={file} transform={async (work) => void (await work(b))} close={vi.fn()} onDone={saved} />);
  expect(await screen.findByText("2 holdings worth $22,845.00 in all. $820.00 of it is cash.")).toBeTruthy();
  expect(holdings.file).toHaveBeenCalledWith("Symbol,Value\nVWRA,17100\n");
  const rows = screen.getAllByRole("row").map((r) => r.textContent);
  // A name with its symbol and what gives its value; cost only where the file has one.
  expect(rows[1]).toBe("World fundVWRA · 120.5 × 141.91$15,000.00$17,100.00");
  expect(rows[2]).toBe("T$4,925.00");
  // No rate is asked for in the budget's own currency, and nothing sends the file to the assistant when it read whole.
  expect(screen.queryByText("1 EUR in USD")).toBeNull();
  expect(screen.queryByRole("button", { name: "Ask the assistant to read it" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Save holdings" }));
  await waitFor(() => expect(saved).toHaveBeenCalled());
  expect(holdings.record).toHaveBeenCalledWith(b, "pension", {
    source: "file",
    sourceAccount: "positions.csv",
    asOf: "2026-11-13",
    currency: "",
    rate: "",
    positions: found().positions,
    cash: "820",
  });
});

it("asks for a rate in another currency, and hands a file it can't read whole to the assistant", async () => {
  const b = budget();
  const ask = vi.fn();
  const close = vi.fn();
  const pension = b.accounts.find((a) => a.id === "pension")!;
  vi.mocked(holdings.file).mockResolvedValue(found({ currency: "EUR", skipped: 2 }));
  vi.mocked(holdings.record).mockImplementation(async (budget) => budget);
  render(<PositionsFileDialog budget={b} account={pension} file={file} transform={async (work) => void (await work(b))} close={close} onDone={vi.fn()} ask={ask} />);
  expect(await screen.findByText("2 rows had no value UrBudget could read and are left out.")).toBeTruthy();
  // Euros in a dollar budget: what one is worth must be said before it can be kept.
  const save = screen.getByRole("button", { name: "Save holdings" }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("1 EUR in USD"), { target: { value: "1.1" } });
  expect(save.disabled).toBe(false);
  fireEvent.click(save);
  await waitFor(() => expect(holdings.record).toHaveBeenCalled());
  expect(vi.mocked(holdings.record).mock.calls[0][2]).toMatchObject({ currency: "EUR", rate: "1.1" });
  // Rows were left out, so the assistant is offered the file.
  fireEvent.click(screen.getByRole("button", { name: "Ask the assistant to read it" }));
  expect(ask).toHaveBeenCalledWith(file);
  expect(close).toHaveBeenCalled();
  cleanup();
  // A file with nothing it can read says so, and the assistant is the way on.
  vi.mocked(holdings.file).mockRejectedValue("UrBudget couldn't find holdings in this file.");
  render(<PositionsFileDialog budget={b} account={pension} file={file} transform={vi.fn()} close={vi.fn()} onDone={vi.fn()} ask={ask} />);
  expect(await screen.findByText("UrBudget couldn't find holdings in this file.")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Save holdings" })).toBeNull();
  expect(screen.getByRole("button", { name: "Ask the assistant to read it" })).toBeTruthy();
});

it("keeps a value typed in for the day it's given", async () => {
  const b = budget([typedIn]);
  const saved = vi.fn();
  vi.mocked(holdings.record).mockImplementation(async (budget) => budget);
  render(<TypedValueDialog budget={b} account={b.accounts.find((a) => a.id === "pension")!} transform={async (work) => void (await work(b))} close={vi.fn()} onDone={saved} />);
  expect(screen.getByText(/Last value: \$10,000\.00 on/)).toBeTruthy();
  const save = screen.getByRole("button", { name: "Save value" }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("Value"), { target: { value: "10,500.50" } });
  fireEvent.click(save);
  await waitFor(() => expect(saved).toHaveBeenCalled());
  expect(holdings.record).toHaveBeenCalledWith(b, "pension", { source: "manual", asOf: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/), total: "10500.50" });
});

function Panel({ start, ask }: { start: Budget; ask?: (request: { text: string; files?: File[] }) => void }) {
  const [b, setB] = useState(start);
  const transform = async (work: (b: Budget) => Promise<Budget>) => setB(await work(b));
  return (
    <ToastProvider>
      <InvestmentActionsContext.Provider value={{ transform, settings: vi.fn(), ask }}>
        <InvestmentsSettings budget={b} transform={transform} />
      </InvestmentActionsContext.Provider>
    </ToastProvider>
  );
}

it("lists every investment account with how it's kept up to date, Interactive Brokers among the ways", async () => {
  const ask = vi.fn();
  vi.mocked(pickPositionsFile).mockResolvedValue(file);
  vi.mocked(holdings.file).mockResolvedValue(found());
  render(<Panel start={budget([typedIn])} ask={ask} />);
  expect(screen.getByRole("heading", { name: "Investments" })).toBeTruthy();
  const row = (name: string) => screen.getByText(name).closest("tr")!;
  expect(row("Pension").textContent).toContain("Value you type in");
  expect(row("Pension").textContent).toContain("$10,000.00");
  expect(row("Stocks ISA").textContent).toContain("Nothing yet");
  // Interactive Brokers' own setup sits folded in the same panel, opened from an account's row.
  const broker = screen.getByText("Interactive Brokers", { selector: "summary" }).closest("details")!;
  expect(broker.open).toBe(false);
  fireEvent.click(within(row("Stocks ISA")).getByRole("button", { name: "Interactive Brokers" }));
  expect(broker.open).toBe(true);
  expect(await within(broker).findByText("Make a Flex Query")).toBeTruthy();
  // Typing a value in is asked for before it's offered on an account nothing fills.
  expect(within(row("Stocks ISA")).queryByRole("button", { name: "Type its value" })).toBeNull();
  fireEvent.click(screen.getByLabelText("Advanced: let me type an account's value myself"));
  expect(typedValues.on()).toBe(true);
  fireEvent.click(within(row("Stocks ISA")).getByRole("button", { name: "Type its value" }));
  expect(screen.getByRole("dialog", { name: "Value of Stocks ISA" })).toBeTruthy();
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
  // A positions file is chosen, read and shown before anything is kept; the assistant gets it on request.
  fireEvent.click(within(row("Stocks ISA")).getByRole("button", { name: "Positions file" }));
  expect(await screen.findByRole("dialog", { name: "Positions file · Stocks ISA" })).toBeTruthy();
  expect(await screen.findByText(/2 holdings worth/)).toBeTruthy();
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
  // Unlinking leaves the account and takes its holdings away.
  fireEvent.click(within(row("Pension")).getByRole("button", { name: "Unlink" }));
  await waitFor(() => expect(row("Pension").textContent).toContain("Nothing yet"));
});

it("gives an account kept by hand its own page, with nothing of Interactive Brokers' on it", async () => {
  const b = budget([typedIn]);
  render(
    <ToastProvider>
      <InvestmentActionsContext.Provider value={{ transform: vi.fn(), settings: vi.fn() }}>
        <HoldingsHeader budget={b} account={b.accounts.find((a) => a.id === "pension")!} balance={1000000} />
      </InvestmentActionsContext.Provider>
    </ToastProvider>,
  );
  const page = screen.getByRole("region", { name: "Holdings" });
  expect(await within(page).findByText("$10,000.00")).toBeTruthy();
  // No connection to ask about, no sync, and no list of what it holds: its value is all that's known.
  expect(ibkr.status).not.toHaveBeenCalled();
  expect(within(page).queryByRole("button", { name: "Sync" })).toBeNull();
  expect(within(page).queryByText("What it holds")).toBeNull();
  expect(within(page).getByText(/From the value you typed for/)).toBeTruthy();
  fireEvent.click(within(page).getByRole("button", { name: "Update value" }));
  expect(screen.getByRole("dialog", { name: "Value of Pension" })).toBeTruthy();
});
