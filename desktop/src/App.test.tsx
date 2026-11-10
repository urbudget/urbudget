import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { demoBudget } from "./demo";
import { today, type Budget } from "./types";
// Whether the budget has loaded yet; until it has, App shows its loading screen.
const state = vi.hoisted(() => ({ loaded: true, transform: vi.fn(), edit: vi.fn(), retry: vi.fn(), budget: null as (() => Budget) | null,
  calculated: true, display: false, calculationError: "", rejectedMonth: "", canUndo: false,
  undo: vi.fn(), fetch: vi.fn(), dirty: false }));
vi.mock("./useBudget", async (importOriginal) => ({
  ...await importOriginal<typeof import("./useBudget")>(),
  useBudget: (month: string) => ({
    budget: state.loaded ? (state.budget?.() ?? demoBudget()) : null,
    summary: state.loaded && state.calculated && month !== state.rejectedMonth ? {
      readyToAssign: 0,
      assigned: 0,
      activity: 0,
      available: 0,
      income: 0,
      netWorth: 0,
      accounts: [],
      categories: [],
    } : null,
    displaySummary: state.loaded && state.display ? { readyToAssign: 0, assigned: 0, activity: 0, available: 0, income: 0, netWorth: 0, accounts: [], categories: [] } : null,
    calculationError: month === state.rejectedMonth ? "This calculation exceeds the supported history size." : state.calculationError,
    retryCalculation: state.retry,
    sync: "pending",
    error: "",
    metadata: null,
    edit: state.edit,
    transform: state.transform,
    canUndo: state.canUndo,
    undo: state.undo,
    fetchDatabase: state.fetch,
    dirty: () => state.dirty,
    setExitTask: () => {},
    setExitDrain: () => {},
    refreshDatabase: state.fetch,
    canAdoptIncoming: () => !state.dirty,
  }),
}));
vi.mock("./bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./bridge")>();
  const projection = { months: 13, totalInterest: 41234, totalFees: 0, payoffDate: "2027-10-01", schedule: [] };
  return { ...actual, api: { ...actual.api, projectLoan: vi.fn(async () => projection), export: vi.fn(), exportRecoveryDraft: vi.fn() } };
});
// What App tells automatic updates is safe to interrupt.
const automatic = vi.hoisted(() => ({ safe: null as (() => boolean) | null, install: vi.fn(), offer: false }));
vi.mock("./updates", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./updates")>();
  return {
    ...actual,
    useUpdates: () => {
      const updates = actual.useUpdates();
      return automatic.offer ? { ...updates, channel: "direct" as const, available: { version: "1.0.11", notes: "" }, ready: true, install: automatic.install } : updates;
    },
    useAutomaticUpdate: (_ready: boolean, safe: () => boolean) => {
      automatic.safe = safe;
    },
  };
});
vi.mock("./SettingsPage", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./SettingsPage")>();
  return {
    ...actual,
    SettingsPage: (props: Parameters<typeof actual.SettingsPage>[0]) => automatic.offer
      ? <button onClick={props.onInstallUpdate}>Restart to update</button>
      : <actual.SettingsPage {...props} />,
  };
});
// The assistant's side of the app, signed in, for the panel the window hosts.
vi.mock("./assistant/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./assistant/api")>();
  const status = { provider: "chatgpt", ready: true, account: null, model: null, shareMemos: true, keepHistory: true, planNoticeSeen: true,
    noticeDue: false, hasOpenaiKey: false, hasAnthropicKey: false, storage: null, signingIn: false, mcp: { command: "", args: [], config: {} } };
  return {
    ...actual,
    assistant: {
      ...actual.assistant,
      status: vi.fn(async () => status),
      attach: vi.fn(async (name: string) => ({ id: "file_1", name, mime: "text/csv", size: 11, kind: "csv" })),
      onEvent: vi.fn(async () => () => {}),
    },
  };
});
import App from "./App";
import { assistant } from "./assistant/api";
import { forgetComposers } from "./assistant/composer";
import { api } from "./bridge";
import { ToastProvider } from "./ui";
import { AppliedChangeError } from "./useBudget";

beforeEach(() => { state.loaded = true; state.calculated = true; state.display = false; state.calculationError = ""; state.rejectedMonth = ""; state.canUndo = false; state.dirty = false; state.edit.mockClear(); state.retry.mockClear(); state.undo.mockReset(); state.fetch.mockReset(); automatic.offer = false; automatic.install.mockReset(); });
afterEach(cleanup);
it("doesn't update by itself over an unfinished import, on its page or after leaving it (Codex review R2)", async () => {
  render(<ToastProvider><App /></ToastProvider>);
  expect(automatic.safe!()).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: /^Everyday account\d/ }));
  fireEvent.click(screen.getByRole("button", { name: "Import" }));
  const statement = new File(["%PDF-1.4"], "statement.pdf", { type: "application/pdf" });
  fireEvent.change(screen.getByLabelText("Bank file"), { target: { files: [statement] } });
  await waitFor(() => expect(screen.getAllByText("statement.pdf").length).toBeGreaterThan(0));
  (document.activeElement as HTMLElement | null)?.blur();
  expect(automatic.safe!()).toBe(false);
  // Away on another page, the import is still waiting.
  fireEvent.click(screen.getByRole("button", { name: "Budget" }));
  expect(automatic.safe!()).toBe(false);
});

it("preserves an unfinished import when an update is requested from Settings", async () => {
  automatic.offer = true;
  render(<ToastProvider><App /></ToastProvider>);
  fireEvent.click(screen.getByRole("button", { name: /^Everyday account\d/ }));
  fireEvent.click(screen.getByRole("button", { name: "Import" }));
  const statement = new File(["%PDF-1.4"], "statement.pdf", { type: "application/pdf" });
  fireEvent.change(screen.getByLabelText("Bank file"), { target: { files: [statement] } });
  await waitFor(() => expect(screen.getAllByText("statement.pdf").length).toBeGreaterThan(0));
  fireEvent.click(screen.getByRole("button", { name: "Settings & data" }));
  fireEvent.click(screen.getByRole("button", { name: "Restart to update" }));
  expect(automatic.install).not.toHaveBeenCalled();
  expect(await screen.findByText("Finish your import or assistant work before installing an update.")).toBeTruthy();
});

it("hands a dropped file to the assistant once, however often its panel closes and reopens", async () => {
  try {
    render(<ToastProvider><App /></ToastProvider>);
    const statement = new File(["date,amount"], "statement.csv", { type: "text/csv" });
    fireEvent.drop(window, { dataTransfer: { files: [statement], types: ["Files"] } });
    // The whole app renders and the file is read before its chip shows: on a busy machine
    // that takes longer than the second a find waits by default.
    expect(await screen.findByRole("button", { name: "Remove statement.csv" }, { timeout: 8000 })).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Message the assistant"), { target: { value: "Import this into Everyday" } });
    // Ctrl+J closes the panel and opens it again: the file and the text are as they were left,
    fireEvent.keyDown(window, { key: "j", ctrlKey: true });
    expect(screen.queryByLabelText("Message the assistant")).toBeNull();
    // (still unsent, which an update by itself waits for)
    expect(automatic.safe!()).toBe(false);
    fireEvent.keyDown(window, { key: "j", ctrlKey: true });
    expect(await screen.findByLabelText("Message the assistant")).toHaveProperty("value", "Import this into Everyday");
    expect(screen.getAllByText("statement.csv")).toHaveLength(1);
    // and the drop isn't made a second time.
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByText("Attaching…")).toBeNull();
    expect(assistant.attach).toHaveBeenCalledTimes(1);
  } finally {
    localStorage.removeItem("lb:assistant:open");
    forgetComposers();
  }
}, 20000);

it("takes the keyboard to the assistant's message field when it's opened, and back to its button when it closes", async () => {
  try {
    render(<ToastProvider><App /></ToastProvider>);
    fireEvent.keyDown(window, { key: "j", ctrlKey: true });
    const box = await screen.findByLabelText("Message the assistant");
    await waitFor(() => expect(document.activeElement).toBe(box));
    // Esc in the panel closes it, as its own button and Ctrl+J do.
    fireEvent.keyDown(box, { key: "Escape" });
    expect(screen.queryByLabelText("Message the assistant")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Assistant" })));
  } finally {
    localStorage.removeItem("lb:assistant:open");
    forgetComposers();
  }
});

it("tells the assistant which category is selected on the Budget page, when it's the only one", async () => {
  try {
    render(<ToastProvider><App /></ToastProvider>);
    fireEvent.keyDown(window, { key: "j", ctrlKey: true });
    const line = await screen.findByRole("group", { name: "With this message" });
    expect(line.textContent).toContain("Budget");
    expect(line.textContent).not.toContain("Groceries");
    fireEvent.click(screen.getByRole("checkbox", { name: "Select Groceries" }));
    await waitFor(() => expect(line.textContent).toContain("Groceries"));
    // With two selected, no one category is what's in view.
    fireEvent.click(screen.getByRole("checkbox", { name: "Select Rent" }));
    await waitFor(() => expect(line.textContent).not.toContain("Groceries"));
    expect(line.textContent).not.toContain("Rent");
  } finally {
    localStorage.removeItem("lb:assistant:open");
    forgetComposers();
  }
});

it("renders newly created categories while Rust is still calculating their totals", () => {
  render(<App />);
  expect(screen.getByText("Groceries")).toBeTruthy();
  expect(screen.getByRole("grid", { name: "Monthly plan" })).toBeTruthy();
});

it("opens the budget after its loading screen, calling the same hooks either way", () => {
  state.loaded = false;
  const { rerender } = render(<App />);
  expect(screen.getByText("Opening your budget…")).toBeTruthy();
  state.loaded = true;
  rerender(<App />);
  expect(screen.getByRole("grid", { name: "Monthly plan" })).toBeTruthy();
});

it("keeps the editor mounted but blocks stale assignments while recalculating", async () => {
  const view = render(<App />);
  const amount = screen.getByLabelText("Assigned to Groceries");
  fireEvent.focus(amount);
  fireEvent.change(amount, { target: { value: "123" } });
  state.calculated = false;
  state.display = true;
  view.rerender(<App />);
  expect(screen.getByLabelText("Assigned to Groceries")).toBe(amount);
  expect(view.container.querySelector(".page-scroll")?.hasAttribute("inert")).toBe(true);
  // A quick recalculation (most edits) doesn't flash the banner; a slow one says why the page is blocked.
  expect(screen.queryByRole("status")).toBeNull();
  expect((await screen.findByRole("status")).textContent).toContain("Updating");
  fireEvent.blur(amount);
  expect(state.edit).not.toHaveBeenCalled();
});

it("removes stale financial controls after a calculation failure and offers calculation retry", () => {
  const view = render(<App />);
  state.calculated = false;
  state.calculationError = "Calculation unavailable";
  view.rerender(<App />);
  expect(screen.queryByLabelText("Assigned to Groceries")).toBeNull();
  expect(screen.queryByRole("grid", { name: "Monthly plan" })).toBeNull();
  expect(screen.getByText(/Could not calculate.*Calculation unavailable/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Retry calculation" }));
  expect(state.retry).toHaveBeenCalledTimes(1);
  expect(state.edit).not.toHaveBeenCalled();
});

it("recovers from a deterministic month rejection by navigating to another month", () => {
  state.rejectedMonth = today().slice(0, 7);
  render(<App />);
  expect(screen.queryByRole("grid", { name: "Monthly plan" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Previous month" }));
  expect(screen.queryByLabelText("Calculation recovery")).toBeNull();
  expect(screen.getByText("Groceries")).toBeTruthy();
  expect(state.retry).not.toHaveBeenCalled();
});

it("allows Undo and protects unsaved changes before reloading a rejected draft", async () => {
  state.calculated = false;
  state.calculationError = "Invalid target amount";
  state.canUndo = true;
  state.dirty = true;
  render(<App />);
  fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  expect(state.undo).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Export draft" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Reload saved budget" }));
  expect(state.fetch).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(state.fetch).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Reload saved budget" }));
  fireEvent.click(screen.getByRole("button", { name: "Discard changes & reload" }));
  await waitFor(() => expect(state.fetch).toHaveBeenCalledTimes(1));
  expect(state.edit).not.toHaveBeenCalled();
});

it("exports a rejected draft through recovery export rather than validated backup export", async () => {
  state.calculated = false;
  state.calculationError = "Invalid target amount";
  vi.mocked(api.exportRecoveryDraft).mockResolvedValue("/exports/recovery-draft.json");
  vi.mocked(api.export).mockClear();
  render(<ToastProvider><App /></ToastProvider>);
  fireEvent.click(screen.getByRole("button", { name: "Export draft" }));
  await screen.findByText("Recovery draft exported.");
  expect(api.exportRecoveryDraft).toHaveBeenCalledWith(expect.objectContaining({ schemaVersion: 2 }));
  expect(api.export).not.toHaveBeenCalled();
});

it.each(["monthly", "yearly"])("keeps an inline draft's repeat through More options, then saves %s", async (frequency) => {
  let saved: Budget | undefined;
  state.transform.mockImplementation(async (fn: (b: Budget) => Promise<Budget>) => {
    saved = await fn(demoBudget());
  });
  render(<App />);
  fireEvent.click(screen.getByRole("button", { name: /^All transactions/ }));
  fireEvent.click(screen.getByRole("button", { name: "Add transaction" }));
  const row = screen.getByRole("row", { name: "New transaction" });
  const date = within(row).getByLabelText("Date");
  fireEvent.focus(date);
  fireEvent.change(date, { target: { value: "2999-01-15" } });
  fireEvent.blur(date);
  fireEvent.change(within(row).getByLabelText("Inflow"), { target: { value: "25" } });
  fireEvent.change(within(row).getByLabelText("Repeat"), { target: { value: "monthly" } });
  fireEvent.click(within(row).getByRole("button", { name: "More options" }));

  const dialog = screen.getByRole("dialog", { name: "New transaction" });
  const repeat = within(dialog).getByLabelText("Repeat");
  expect(repeat).toHaveProperty("value", "monthly");
  if (frequency !== "monthly") fireEvent.change(repeat, { target: { value: frequency } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Schedule" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "New transaction" })).toBeNull());
  expect(saved?.schedules).toEqual([
    expect.objectContaining({
      frequency,
      nextDate: "2999-01-15",
      transaction: expect.objectContaining({ date: "2999-01-15", amount: 2500 }),
    }),
  ]);
  expect(saved?.transactions).toHaveLength(demoBudget().transactions.length);
});

it("opens a loan's register and payoff planner from its account menu, with no Loan planner page", async () => {
  state.budget = () => {
    const b = demoBudget();
    b.accounts.push({ id: "loan", name: "Car loan", kind: "tracking", accountType: "autoLoan", openingBalance: -1200000, archived: false });
    b.loans = [{ accountId: "loan", categoryId: "", annualRateBasisPoints: 650, monthlyPayment: 100000, monthlyFees: 0 }];
    return b;
  };
  try {
    render(<App />);
    expect(screen.queryByRole("button", { name: "Loan planner" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Actions for Car loan" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Loan details & payoff…" }));
    expect(screen.getByRole("dialog", { name: "Payoff planner · Car loan" })).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1, name: "Car loan" })).toBeTruthy();
    await waitFor(() => expect(screen.getByRole("region", { name: "Loan" }).textContent).toContain("Oct 2027"));
  } finally {
    state.budget = null;
  }
});

it.each(["card", "loan"])("keeps the %s payment draft and save-only retry mounted through recalculation", async (kind) => {
  const b = demoBudget();
  b.accounts.push({ id: "loan", name: "Car loan", kind: "tracking", accountType: "autoLoan", openingBalance: -1200000, archived: false });
  b.loans = [{ accountId: "loan", categoryId: "groceries", annualRateBasisPoints: 650, monthlyPayment: 100000, monthlyFees: 0 }];
  state.budget = () => b;
  const retry = vi.fn().mockResolvedValue(undefined);
  state.transform.mockReset().mockRejectedValue(new AppliedChangeError(retry));
  try {
    const view = render(<App />);
    fireEvent.click(screen.getByRole("button", { name: kind === "card" ? /^Credit card\d/ : /^Car loan\d/ }));
    fireEvent.click(screen.getByRole("button", { name: kind === "card" ? "Make payment" : "Record payment" }));
    const amount = screen.getByLabelText("Payment amount");
    fireEvent.change(amount, { target: { value: "50" } });

    // The native transform applies the draft before its save completes. The resulting
    // calculation must not recreate the form and lose its payment identity or typed amount.
    state.calculated = false;
    state.display = true;
    view.rerender(<App />);
    expect(screen.getByLabelText("Payment amount")).toBe(amount);
    expect(amount.closest("[inert]")).toBeTruthy();
    state.calculated = true;
    view.rerender(<App />);
    expect(screen.getByLabelText("Payment amount")).toHaveProperty("value", "50");
    fireEvent.click(screen.getByRole("button", { name: /^Pay 50\.00/ }));
    const saving = await screen.findByRole("button", { name: "Retry saving" });

    state.calculated = false;
    view.rerender(<App />);
    expect(screen.getByRole("button", { name: "Retry saving" })).toBe(saving);
    expect(saving.closest("[inert]")).toBeTruthy();
    state.calculated = true;
    view.rerender(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Retry saving" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(state.transform).toHaveBeenCalledTimes(1);
    expect(retry).toHaveBeenCalledTimes(1);
  } finally {
    state.budget = null;
  }
});

it("puts the report groups in the page header, after its title", async () => {
  render(<App />);
  fireEvent.click(screen.getByRole("button", { name: "Reports" }));
  const groups = await screen.findByRole("tablist", { name: "Report group" });
  expect(groups.closest(".app-header")?.querySelector("h1")?.textContent).toBe("Reports");
  expect(within(groups).getByRole("tab", { name: "Spending" }).getAttribute("aria-selected")).toBe("true");
  // The group's reports are tabs over the report, not a second sidebar.
  expect(screen.getByRole("tablist", { name: "Spending reports" }).closest(".app-header")).toBeNull();
  expect(screen.queryByRole("navigation", { name: "Reports" })).toBeNull();
});
