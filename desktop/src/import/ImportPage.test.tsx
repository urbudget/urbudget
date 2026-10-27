import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import type { BankFixes, BankRow, BankSession, LineStatus, Statement, StatementLine } from "../bridge";
import type { Budget } from "../types";

vi.mock("../bridge", () => ({
  api: {
    bankSession: vi.fn(),
    bankStatement: vi.fn(),
    bankStatementPreview: vi.fn(),
    bankApplyFixes: vi.fn(),
    importBank: vi.fn(async (b: Budget) => b),
    decodeStatement: vi.fn(async (latin1: string) => ({ text: latin1, encoding: "ISO-8859-1" })),
    writeExport: vi.fn(async (): Promise<string | null> => "C:/Users/pat/Downloads/bank-import-template.csv"),
    reveal: vi.fn(async () => {}),
  },
}));
import { api } from "../bridge";
import { demoBudget } from "../demo";
import { ToastProvider } from "../ui";
import { ImportPage, type ImportState } from "./ImportPage";
import { applyToLatest, roles, withRole } from "./model";

afterEach(cleanup);
beforeEach(() => vi.clearAllMocks());

const scope = (session: string) => ({ session, budget: "b", account: "checking", file: `file-${session}` });
const session = (id = "s1", revision = 0, more: Partial<BankSession["structure"]> = {}): BankSession => ({
  scope: scope(id),
  revision,
  structure: { format: "csv", dateOrder: { order: "ymd", confirmed: false }, flipSigns: false, ...more },
  edits: { cells: {}, fields: {}, include: [] },
  versions: {},
});
const line = (id: string, cells: string[], status: LineStatus = { kind: "ready" }, more: Partial<StatementLine> = {}): StatementLine => ({
  id,
  version: 0,
  cells,
  fields: { date: cells[0], payee: cells[1], memo: "", amount: cells[2] },
  status,
  ...more,
});
const statement = (s: BankSession, lines: StatementLine[], more: Partial<Statement> = {}): Statement => {
  const leftOut: Statement["counts"]["leftOut"] = {};
  for (const l of lines) if (l.status.kind === "leftOut") leftOut[l.status.reason] = (leftOut[l.status.reason] ?? 0) + 1;
  return {
    scope: s.scope,
    revision: s.revision,
    structure: s.structure,
    format: "csv",
    headers: ["Date", "Payee", "Amount"],
    columns: { date: "Date", payee: "Payee", amount: "Amount" },
    headerLine: 0,
    order: "ymd",
    lines,
    counts: {
      ready: lines.filter((l) => l.status.kind === "ready").length,
      attention: lines.filter((l) => l.status.kind === "attention").length,
      leftOut,
    },
    remember: { dateFormat: "ymd", dateConfirmed: false, flipSigns: false },
    ...more,
  };
};
const bakery = line("0", ["2026-10-01", "Bakery", "-3.20"]);
const grocer = line("30", ["2026-10-02", "Grocer", "n/a"], {
  kind: "attention",
  field: "amount",
  column: 2,
  reason: "Invalid amount: n/a",
});
const lunch = line("60", ["2026-10-03", "Lunch", "-8.50"]);
const rowOf = (l: StatementLine): BankRow => ({
  id: `bank:${l.id}`,
  date: l.cells[0],
  payee: l.cells[1],
  memo: "",
  amount: Math.round(Number(l.cells[2]) * 100),
  duplicate: false,
  candidates: [],
});

/**
 * The engine, for one file: a session for it, its statement as `read` makes it of the session,
 * fixes that apply by making the next revision, and the review of the lines that read.
 */
function engine(read: (s: BankSession) => Statement, first = session()) {
  vi.mocked(api.bankSession).mockResolvedValue(first);
  vi.mocked(api.bankStatement).mockImplementation(async (_b, _a, _c, s) => read(s));
  vi.mocked(api.bankApplyFixes).mockImplementation(async (_b, _a, _c, s, fixes) => ({
    session: { ...s, revision: s.revision + 1, structure: fixes.structure ?? s.structure },
    results: [{ fix: "cell", applied: true }],
  }));
  vi.mocked(api.bankStatementPreview).mockImplementation(async (_b, _a, _c, s) => {
    const st = read(s);
    const ready = st.lines.filter((l) => l.status.kind === "ready");
    return { statement: st, rows: ready.map(rowOf), lines: ready.map((l) => l.id) };
  });
}
/** The fixes sent last. */
const sent = (): BankFixes => vi.mocked(api.bankApplyFixes).mock.lastCall![4];

function Page({
  budget = demoBudget(),
  initial = { account: "checking" },
  onImported = vi.fn(),
  onAskAssistant,
  onFixWithAssistant,
  away = false,
}: {
  budget?: Budget;
  initial?: ImportState;
  onImported?: () => void;
  onAskAssistant?: (file: File, problem: string, account: string) => void;
  onFixWithAssistant?: () => void;
  /** On another page: the import is kept, as the app keeps it, but not shown. */
  away?: boolean;
}) {
  const [state, setState] = useState<ImportState>(initial);
  if (away) return null;
  return (
    <ToastProvider>
      <ImportPage
        budget={budget}
        budgetId="b"
        state={state}
        onState={(next) => setState((was) => (typeof next === "function" ? next(was) : next))}
        transform={async (work) => void (await work(budget))}
        onImported={onImported}
        onCancel={vi.fn()}
        onAskAssistant={onAskAssistant}
        onFixWithAssistant={onFixWithAssistant}
      />
    </ToastProvider>
  );
}
const choose = (file: File) => fireEvent.change(screen.getByLabelText("Bank file"), { target: { files: [file] } });
const csv = (name = "october.csv") => new File(["Date,Payee,Amount\n2026-10-01,Bakery,-3.20\n"], name, { type: "text/csv" });
/** Opens a line to fix and gives one of its cells a new value, saved with Enter. */
const fixCell = (id: string, column: string, to: string) => {
  fireEvent.click(screen.getByRole("button", { name: `Fix line ${id}` }));
  const input = screen.getByLabelText(`${column} of line ${id}`);
  fireEvent.change(input, { target: { value: to } });
  fireEvent.keyDown(input, { key: "Enter" });
};

it("opens the file at the lines that need attention, and a cell fixed there is sent as that fix", async () => {
  engine((s) =>
    statement(
      s,
      s.revision
        ? [
            bakery,
            line(
              "30",
              ["2026-10-02", "Grocer", "-12.40"],
              { kind: "ready" },
              { fixed: true, original: ["2026-10-02", "Grocer", "n/a"], version: 1 },
            ),
            lunch,
          ]
        : [bakery, grocer, lunch],
    ),
  );
  render(<Page />);
  choose(csv());
  expect(await screen.findByText("Invalid amount: n/a")).toBeTruthy();
  // Only the line that needs attention is shown to begin with; the rest are a tab away.
  expect(screen.queryByText("Bakery")).toBeNull();
  expect(screen.getByText(/2 lines read · 1 needs attention and won't be imported/)).toBeTruthy();
  expect(api.bankSession).toHaveBeenCalledWith(
    expect.anything(),
    "checking",
    expect.stringContaining("Bakery"),
    expect.any(String),
    "b",
    null,
    false,
  );
  // The line opens to edit at the cell that doesn't read.
  fireEvent.click(screen.getByRole("button", { name: "Fix line 30" }));
  const cell = screen.getByLabelText("Amount of line 30");
  expect(document.activeElement).toBe(cell);
  fireEvent.change(cell, { target: { value: "-12.40" } });
  fireEvent.keyDown(cell, { key: "Enter" });
  await waitFor(() => expect(api.bankApplyFixes).toHaveBeenCalledTimes(1));
  expect(sent()).toMatchObject({
    scope: scope("s1"),
    baseRevision: 0,
    cells: [{ line: "30", lineVersion: 0, column: 2, from: "n/a", to: "-12.40" }],
  });
  // It reads now, marked as changed from the file.
  fireEvent.click(screen.getByRole("tab", { name: /All/ }));
  const fixed = await screen.findByRole("cell", { name: "Amount of line 30: -12.40" });
  expect(fixed.className).toContain("edited");
  expect(fixed.title).toBe("In the file: n/a");
  // Lines still needing attention don't stop the review: they stay out of it.
  fireEvent.click(screen.getByRole("button", { name: "Continue to the review" }));
  expect(await screen.findByRole("button", { name: "Import 3 transactions" })).toBeTruthy();
});

it("edits a line as the register does: a click selects it, another edits it there, Esc leaves it as it was", async () => {
  const pending = line("90", ["2026-10-04", "Cinema", "-12.00"], { kind: "leftOut", reason: "pending", detail: "Pending" });
  engine((s) => statement(s, [bakery, grocer, pending]));
  render(<Page />);
  choose(csv());
  fireEvent.click(await screen.findByRole("tab", { name: /All/ }));
  const payee = screen.getByRole("cell", { name: "Payee of line 0: Bakery" });
  fireEvent.click(payee);
  expect(payee.closest('[role="row"]')!.getAttribute("aria-selected")).toBe("true");
  expect(screen.queryByLabelText("Payee of line 0")).toBeNull();
  // A second click edits it, from the cell clicked.
  fireEvent.click(payee);
  const input = screen.getByLabelText("Payee of line 0");
  expect(document.activeElement).toBe(input);
  fireEvent.change(input, { target: { value: "Bakery & Co" } });
  fireEvent.keyDown(input, { key: "Escape" });
  expect(screen.queryByLabelText("Payee of line 0")).toBeNull();
  expect(screen.getByRole("cell", { name: "Payee of line 0: Bakery" })).toBeTruthy();
  // The keys move along the lines, and Enter edits one.
  const lines = screen.getByRole("grid").parentElement!;
  fireEvent.keyDown(lines, { key: "ArrowDown" });
  fireEvent.keyDown(lines, { key: "Enter" });
  expect(document.activeElement).toBe(screen.getByLabelText("Amount of line 30"));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(api.bankApplyFixes).not.toHaveBeenCalled();
  // A click on another line saves the one being edited, as a spreadsheet does.
  fireEvent.keyDown(lines, { key: "Enter" });
  fireEvent.change(screen.getByLabelText("Amount of line 30"), { target: { value: "-12.40" } });
  fireEvent.click(screen.getByRole("cell", { name: "Payee of line 0: Bakery" }));
  await waitFor(() => expect(sent()).toMatchObject({ cells: [{ line: "30", column: 2, from: "n/a", to: "-12.40" }] }));
  expect(api.bankApplyFixes).toHaveBeenCalledTimes(1);
  expect(screen.queryByLabelText("Amount of line 30")).toBeNull();
});

it("leaves Enter on the editor's Cancel to the button, which cancels", async () => {
  engine((s) => statement(s, [bakery, grocer]));
  render(<Page />);
  choose(csv());
  fireEvent.click(await screen.findByRole("button", { name: "Fix line 30" }));
  fireEvent.change(screen.getByLabelText("Amount of line 30"), { target: { value: "-1.00" } });
  const cancel = screen.getByRole("button", { name: "Cancel" });
  // Not taken as Save: the browser goes on to click the button.
  expect(fireEvent.keyDown(cancel, { key: "Enter" })).toBe(true);
  fireEvent.click(cancel);
  expect(screen.queryByLabelText("Amount of line 30")).toBeNull();
  expect(api.bankApplyFixes).not.toHaveBeenCalled();
});

it("saves only the cells typed over, keeping a change that landed on the line while it was edited", async () => {
  let release: () => void = () => {};
  engine((s) =>
    statement(
      s,
      s.revision
        ? [bakery, line("30", ["2026-10-02", "Grocer", "-12.40"], { kind: "ready" }, { fixed: true, version: 1 })]
        : [bakery, grocer],
    ),
  );
  render(<Page />);
  choose(csv());
  await screen.findByRole("button", { name: "Fix line 30" });
  // The amount is fixed, and the file takes its time to read with it.
  const later = vi.mocked(api.bankStatement).getMockImplementation()!;
  vi.mocked(api.bankStatement).mockImplementationOnce((...args) => new Promise((r) => (release = () => r(later(...args)))));
  fixCell("30", "Amount", "-12.40");
  await waitFor(() => expect(api.bankStatement).toHaveBeenCalledTimes(2));
  // Opened again meanwhile, the line shows the amount given.
  fireEvent.click(screen.getByRole("button", { name: "Fix line 30" }));
  expect((screen.getByLabelText("Amount of line 30") as HTMLInputElement).value).toBe("-12.40");
  // The line reads now, so it's no longer one that needs attention, but it stays while it's edited.
  release();
  expect(await screen.findByText("Editing the line")).toBeTruthy();
  const payee = screen.getByLabelText("Payee of line 30");
  fireEvent.change(payee, { target: { value: "Grocer Ltd" } });
  fireEvent.keyDown(payee, { key: "Enter" });
  await waitFor(() => expect(api.bankApplyFixes).toHaveBeenCalledTimes(2));
  expect(sent().cells).toEqual([{ line: "30", lineVersion: 1, column: 1, from: "Grocer", to: "Grocer Ltd" }]);
});

it("selects lines as the register does, and includes or leaves them out together", async () => {
  const pending = line("90", ["2026-10-04", "Cinema", "-12.00"], { kind: "leftOut", reason: "pending", detail: "Pending" });
  const declined = line("120", ["2026-10-05", "Taxi", "-7.00"], { kind: "leftOut", reason: "declined", detail: "Declined" });
  engine((s) => statement(s, [bakery, grocer, pending, declined]));
  render(<Page />);
  choose(csv());
  fireEvent.click(await screen.findByRole("tab", { name: /All/ }));
  const row = (id: string) => screen.getByRole("cell", { name: new RegExp(`^Payee of line ${id}:`) }).closest('[role="row"]')!;
  const picked = () => screen.getAllByRole("row").filter((r) => r.getAttribute("aria-selected") === "true").map((r) => r.getAttribute("data-line"));
  // A click selects one line; Shift the run to another, Ctrl adds or takes one away.
  fireEvent.click(row("0"));
  fireEvent.click(row("90"), { shiftKey: true });
  expect(picked()).toEqual(["0", "30", "90"]);
  expect(screen.getByRole("toolbar", { name: "Selected lines" }).textContent).toContain("3 selected");
  fireEvent.click(row("30"), { ctrlKey: true });
  expect(picked()).toEqual(["0", "90"]);
  // Esc lets them go, from anywhere on the page.
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(picked()).toEqual([]);
  expect(screen.queryByRole("toolbar", { name: "Selected lines" })).toBeNull();
  // The checkboxes add lines and keep the rest; Space adds the line the keys are on.
  fireEvent.click(screen.getByLabelText("Select line 90"));
  fireEvent.click(screen.getByLabelText("Select line 120"));
  fireEvent.click(screen.getByLabelText("Select line 0"));
  expect(picked()).toEqual(["0", "90", "120"]);
  // What fits some of them is done to those, as one change.
  const bar = screen.getByRole("toolbar", { name: "Selected lines" });
  expect(within(bar).queryByRole("button", { name: /Leave out/ })).toBeNull();
  fireEvent.click(within(bar).getByRole("button", { name: /Include/ }));
  await waitFor(() =>
    expect(sent()).toMatchObject({
      include: [
        { line: "90", lineVersion: 0, include: true },
        { line: "120", lineVersion: 0, include: true },
      ],
    }),
  );
  expect(api.bankApplyFixes).toHaveBeenCalledTimes(1);
  // The header's checkbox selects every line shown, or none.
  fireEvent.click(screen.getByLabelText("Select every line shown"));
  expect(picked()).toHaveLength(4);
  fireEvent.click(within(screen.getByRole("toolbar", { name: "Selected lines" })).getByRole("button", { name: "Clear selection" }));
  expect(picked()).toEqual([]);
});

it("goes straight to the review when every line reads, says what's left out, and imports with what the account keeps", async () => {
  const pending = line("90", ["2026-10-04", "Cinema", "-12.00"], { kind: "leftOut", reason: "pending", detail: "Pending" });
  engine((s) => statement(s, [bakery, pending], { remember: { dateFormat: "dmy", dateConfirmed: true, flipSigns: false, source: "A1" } }));
  const onImported = vi.fn();
  const b = demoBudget();
  render(<Page budget={b} onImported={onImported} />);
  choose(csv());
  const check = await screen.findByRole("button", { name: /1 left out · Check the file/ });
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  await waitFor(() => expect(onImported).toHaveBeenCalledWith(1, { ids: [], accounts: [] }));
  expect(api.importBank).toHaveBeenCalledWith(b, "checking", [expect.objectContaining({ action: "new" })], {
    dateFormat: "dmy",
    dateConfirmed: true,
    flipSigns: false,
    source: "A1",
  });
  // The file is a click away, its left-out lines with why.
  fireEvent.click(check);
  fireEvent.click(await screen.findByRole("tab", { name: /Left out/ }));
  expect(screen.getByText("Not booked yet (Pending)")).toBeTruthy();
});

it("reads a file the assistant prepared the standard way, and leaves what the account keeps alone", async () => {
  engine((s) => statement(s, [bakery], { remember: { dateFormat: "ymd", dateConfirmed: true, flipSigns: false } }));
  const onImported = vi.fn();
  const b = demoBudget();
  render(
    <Page
      budget={b}
      onImported={onImported}
      initial={{ account: "checking", file: csv("assistant.csv"), proposal: { threadId: "t1", id: "p1" } }}
    />,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Import 1 transaction" }));
  await waitFor(() => expect(onImported).toHaveBeenCalled());
  expect(api.bankSession).toHaveBeenCalledWith(expect.anything(), "checking", expect.any(String), expect.any(String), "b", null, true);
  expect(api.importBank).toHaveBeenCalledWith(b, "checking", [expect.objectContaining({ action: "new" })], null);
});

it("never shows a review made before a change to the file", async () => {
  engine((s) => statement(s, s.revision ? [bakery, lunch] : [bakery, grocer, lunch]));
  render(<Page />);
  choose(csv());
  fireEvent.click(await screen.findByRole("button", { name: "Continue to the review" }));
  expect(await screen.findByRole("button", { name: "Import 2 transactions" })).toBeTruthy();
  // Back to the file for a change, then on to the review while it's still being checked.
  fireEvent.click(screen.getByRole("button", { name: /Check the file/ }));
  fireEvent.change(await screen.findByLabelText("Purchases in the file"), { target: { value: "positive" } });
  await waitFor(() => expect(api.bankApplyFixes).toHaveBeenCalledTimes(1));
  vi.mocked(api.bankStatementPreview).mockImplementationOnce(() => new Promise(() => {}));
  const next = await screen.findByRole("button", { name: "Continue to the review" });
  await waitFor(() => expect((next as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(next);
  expect(await screen.findByText("Checking the lines against your budget…")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^Import \d/ })).toBeNull();
});

it("keeps what's chosen in the review while the file is looked at, or another page, until another file", async () => {
  const pending = line("90", ["2026-10-04", "Cinema", "-12.00"], { kind: "leftOut", reason: "pending", detail: "Pending" });
  engine((s) => statement(s, [bakery, lunch, pending]));
  const onImported = vi.fn();
  const view = render(<Page onImported={onImported} />);
  choose(csv());
  const action = async () => ((await screen.findByLabelText("Import action 1")) as HTMLSelectElement).value;
  fireEvent.change(await screen.findByLabelText("Import action 1"), { target: { value: "skip" } });
  // To the file and back, both ways.
  fireEvent.click(screen.getByRole("button", { name: /Check the file/ }));
  fireEvent.click(await screen.findByRole("button", { name: "Continue to the review" }));
  expect(await action()).toBe("skip");
  fireEvent.click(screen.getByRole("button", { name: "Back to the file" }));
  fireEvent.click(await screen.findByRole("button", { name: "Continue to the review" }));
  expect(await action()).toBe("skip");
  // Another page, then back to the import from the sidebar.
  view.rerender(<Page onImported={onImported} away />);
  expect(screen.queryByLabelText("Import action 1")).toBeNull();
  view.rerender(<Page onImported={onImported} />);
  expect(await action()).toBe("skip");
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  await waitFor(() => expect(onImported).toHaveBeenCalledWith(1, { ids: [], accounts: [] }));
  expect(vi.mocked(api.importBank).mock.lastCall![2].map((c) => c.action)).toEqual(["skip", "new"]);
  // Another file starts with nothing chosen.
  fireEvent.click(screen.getByRole("button", { name: "Choose another file" }));
  vi.mocked(api.bankSession).mockResolvedValue(session("s2"));
  choose(csv("november.csv"));
  expect(await screen.findByRole("button", { name: "Import 2 transactions" })).toBeTruthy();
  expect(await action()).toBe("new");
});

it("goes on to the review after a change that didn't apply", async () => {
  engine((s) => statement(s, [bakery, grocer]));
  vi.mocked(api.bankApplyFixes).mockResolvedValueOnce({
    session: session(),
    results: [{ line: "30", fix: "cell", applied: false, problem: "This line changed since this fix was made." }],
  });
  render(<Page />);
  choose(csv());
  await screen.findByRole("button", { name: "Fix line 30" });
  fixCell("30", "Amount", "-1.00");
  expect(await screen.findByText("This line changed since this fix was made.")).toBeTruthy();
  const next = screen.getByRole("button", { name: "Continue to the review" }) as HTMLButtonElement;
  await waitFor(() => expect(next.disabled).toBe(false));
  fireEvent.click(next);
  expect(await screen.findByRole("button", { name: "Import 1 transaction" })).toBeTruthy();
  expect(screen.queryByText("This line changed since this fix was made.")).toBeNull();
});

it("includes a line left out, and asks for its amount in the budget's currency", async () => {
  const london = line("40", ["2026-10-05", "London", "-9.99"], { kind: "leftOut", reason: "otherCurrency", detail: "GBP" });
  engine((s) =>
    statement(s, [
      bakery,
      s.revision
        ? {
            ...london,
            included: true,
            version: 1,
            status: { kind: "attention", field: "amount", column: 2, reason: "This line is in GBP: enter the amount in SAR." },
          }
        : london,
    ]),
  );
  render(<Page />);
  choose(csv());
  fireEvent.click(await screen.findByRole("button", { name: /1 left out · Check the file/ }));
  fireEvent.click(await screen.findByRole("tab", { name: /Left out/ }));
  expect(screen.getByText("In GBP")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Include line 40" }));
  await waitFor(() => expect(sent()).toMatchObject({ include: [{ line: "40", lineVersion: 0, include: true }] }));
  fireEvent.click(screen.getByRole("tab", { name: /Need attention/ }));
  expect(await screen.findByText("This line is in GBP: enter the amount in SAR.")).toBeTruthy();
  // Included, it can be fixed or left out again.
  expect(screen.getByRole("button", { name: "Fix line 40" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Leave line 40 out" })).toBeTruthy();
  // A line that reads has nothing to include.
  fireEvent.click(screen.getByRole("tab", { name: /All/ }));
  expect(screen.queryAllByRole("button", { name: /^Include line/ })).toHaveLength(0);
});

it("keeps an answered date order one choice away, and waits for how the file tells money out", async () => {
  engine((s) =>
    statement(s, [line("0", ["01/02/2026", "Bakery", "3.20"])], {
      dateOrder: { order: "dmy", example: "01/02/2026", unconfirmed: false },
      signs: { byType: false, unconfirmed: s.structure.signsByType == null },
    }),
  );
  render(<Page />);
  choose(csv());
  // Not answered yet: the review waits.
  const signs = await screen.findByRole("group", { name: "Signs" });
  expect(screen.getByText("Say how the file tells money out, above, to continue.")).toBeTruthy();
  expect((screen.getByRole("button", { name: "Continue to the review" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(within(signs).getByRole("button", { name: "No, as written" }));
  await waitFor(() => expect(sent().structure).toMatchObject({ signsByType: false }));
  await waitFor(() => expect((screen.getByRole("button", { name: "Continue to the review" }) as HTMLButtonElement).disabled).toBe(false));
  // Both answers stay changeable.
  expect((screen.getByLabelText("Money out") as HTMLSelectElement).value).toBe("written");
  const dates = screen.getByLabelText("Dates") as HTMLSelectElement;
  expect(dates.value).toBe("dmy");
  expect(screen.getByRole("option", { name: "Month first (01/02/2026 is 2 January 2026)" })).toBeTruthy();
  fireEvent.change(dates, { target: { value: "mdy" } });
  await waitFor(() => expect(sent().structure).toMatchObject({ dateOrder: { order: "mdy", confirmed: true } }));
});

it("doesn't dim the lines for a quick change, and makes one asked for meanwhile after it", async () => {
  engine((s) => statement(s, [bakery, grocer, lunch]));
  render(<Page />);
  choose(csv());
  const purchases = (await screen.findByLabelText("Purchases in the file")) as HTMLSelectElement;
  // The first change takes its time.
  let finish: () => void = () => {};
  const apply = vi.mocked(api.bankApplyFixes).getMockImplementation()!;
  vi.mocked(api.bankApplyFixes).mockImplementationOnce((...args) => new Promise((r) => (finish = () => r(apply(...args)))));
  fireEvent.change(purchases, { target: { value: "positive" } });
  // Nothing dims or locks for a moment's work.
  expect((screen.getByRole("button", { name: "Fix line 30" }) as HTMLButtonElement).disabled).toBe(false);
  expect(purchases.disabled).toBe(false);
  // Another change meanwhile waits for it, then is made on the import as the first left it.
  fireEvent.change(screen.getByLabelText("What column 2 holds"), { target: { value: "memo" } });
  expect(api.bankApplyFixes).toHaveBeenCalledTimes(1);
  finish();
  await waitFor(() => expect(api.bankApplyFixes).toHaveBeenCalledTimes(2));
  expect(sent()).toMatchObject({ baseRevision: 1, structure: { flipSigns: true, columns: { memo: "Payee", payee: null } } });
});

it("makes every change asked for while another applies, in turn, none dropped for the next", async () => {
  engine((s) => statement(s, [bakery, grocer, lunch]));
  render(<Page />);
  choose(csv());
  const purchases = await screen.findByLabelText("Purchases in the file");
  let finish: () => void = () => {};
  const apply = vi.mocked(api.bankApplyFixes).getMockImplementation()!;
  vi.mocked(api.bankApplyFixes).mockImplementationOnce((...args) => new Promise((r) => (finish = () => r(apply(...args)))));
  fireEvent.change(purchases, { target: { value: "positive" } });
  // Two more while the first applies.
  fireEvent.change(screen.getByLabelText("What column 2 holds"), { target: { value: "memo" } });
  fireEvent.change(screen.getByLabelText("What column 3 holds"), { target: { value: "outflow" } });
  expect(api.bankApplyFixes).toHaveBeenCalledTimes(1);
  finish();
  await waitFor(() => expect(api.bankApplyFixes).toHaveBeenCalledTimes(3));
  const made = vi.mocked(api.bankApplyFixes).mock.calls.map(([, , , , fixes]) => fixes);
  expect(made.map((f) => f.baseRevision)).toEqual([0, 1, 2]);
  expect(made[1].structure).toMatchObject({ flipSigns: true, columns: { memo: "Payee", payee: null } });
  expect(made[2].structure).toMatchObject({ flipSigns: true, columns: { amount: null, outflow: "Amount" } });
});

it("makes a change again on the import as another left it, so neither undoes the other", async () => {
  const base = session("s1", 0);
  const mine = { ...base, revision: 1 };
  const theirs = { ...base, revision: 1, edits: { ...base.edits, fields: { "30": { amount: "-12.40" } } } };
  const both = { ...theirs, revision: 2 };
  let now: BankSession = base;
  const apply = vi.fn(async (s: BankSession) => {
    // The assistant's fix lands while the user's change is being made.
    if (s === base) {
      now = theirs;
      return { session: mine, results: [] };
    }
    return { session: both, results: [] };
  });
  const applied = await applyToLatest(() => now, apply);
  expect(apply.mock.calls.map(([s]) => s)).toEqual([base, theirs]);
  expect(applied).toMatchObject({ base: theirs, session: both });
  // A change for a file since replaced is dropped.
  now = base;
  expect(await applyToLatest(() => now, async () => ((now = session("s2")), { session: mine, results: [] }))).toBeNull();
});

it("keeps both of two changes that land at once", async () => {
  const first = session("s9", 0);
  // An engine that makes a change on the session it's given.
  const engine = (id: string) => async (s: BankSession) => ({
    session: { ...s, revision: s.revision + 1, edits: { ...s.edits, fields: { ...s.edits.fields, [id]: { amount: "-1" } } } },
    results: [],
  });
  let shown = first;
  const install = (applied: Awaited<ReturnType<typeof applyToLatest>>) => {
    // As a state update does: kept only if it's for the import as it is.
    if (applied && shown === applied.base) shown = applied.session;
  };
  // Both start on the same import; neither result shows before the other lands.
  const [a, b] = await Promise.all([applyToLatest(() => first, engine("one")), applyToLatest(() => first, engine("two"))]);
  install(a);
  install(b);
  expect(Object.keys(shown.edits.fields).sort()).toEqual(["one", "two"]);
});

it("chooses a column whose name another column has too by its place", () => {
  const headers = ["Date", "Payee", "Amount", "Amount", ""];
  const chosen = withRole(headers, { date: "Date", payee: "Payee", amount: "Amount" }, 3, "amount");
  expect(chosen.amount).toBe("#4");
  expect(roles(headers, chosen)).toEqual(["date", "payee", null, "amount", null]);
  // A column without a name, likewise; and a choice moves off the column it was on.
  const memo = withRole(headers, chosen, 4, "memo");
  expect(roles(headers, memo)[4]).toBe("memo");
  expect(roles(headers, withRole(headers, memo, 4, null))[4]).toBeNull();
  // A column named like a place doesn't take another's choice.
  const odd = ["Date", "#4", "Amount", "Amount"];
  const placed = withRole(odd, { date: "Date", amount: "#3" }, 3, "amount");
  expect(placed.amount).toBe("##4");
  expect(roles(odd, placed)).toEqual(["date", null, null, "amount"]);
});

it("shows a cell's new value at once, while the file reads again", async () => {
  let release: () => void = () => {};
  engine((s) =>
    statement(
      s,
      s.revision
        ? [bakery, line("30", ["2026-10-02", "Grocer", "-12.40"], { kind: "ready" }, { fixed: true, version: 1 })]
        : [bakery, grocer],
    ),
  );
  render(<Page />);
  choose(csv());
  await screen.findByRole("button", { name: "Fix line 30" });
  // The next reading takes its time.
  const later = vi.mocked(api.bankStatement).getMockImplementation()!;
  vi.mocked(api.bankStatement).mockImplementationOnce((...args) => new Promise((r) => (release = () => r(later(...args)))));
  fixCell("30", "Amount", "-12.40");
  const shown = await screen.findByRole("cell", { name: "Amount of line 30: -12.40" });
  expect(shown.className).toContain("waiting");
  await waitFor(() => expect(api.bankStatement).toHaveBeenCalledTimes(2));
  release();
  // Read again, it reads: every line does, and the tab says so.
  expect(await screen.findByText(/Every line reads now/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Show all lines" }));
  expect((await screen.findByRole("cell", { name: "Amount of line 30: -12.40" })).className).not.toContain("waiting");
});

it("asks once whether the day or the month comes first, when the dates don't say", async () => {
  engine((s) =>
    statement(
      s,
      [line("0", ["01/02/2026", "Bakery", "-3.20"])],
      s.structure.dateOrder.confirmed ? {} : { dateOrder: { order: "dmy", example: "01/02/2026", unconfirmed: true } },
    ),
  );
  render(<Page />);
  choose(csv());
  const question = await screen.findByRole("group", { name: "Date order" });
  expect(within(question).getByText("01/02/2026")).toBeTruthy();
  // Nothing goes on to the review on a guessed date.
  expect((screen.getByRole("button", { name: "Continue to the review" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(within(question).getByRole("button", { name: "2 January 2026" }));
  await waitFor(() => expect(sent().structure).toMatchObject({ dateOrder: { order: "mdy", confirmed: true } }));
  await waitFor(() => expect(screen.queryByRole("group", { name: "Date order" })).toBeNull());
  await waitFor(() => expect((screen.getByRole("button", { name: "Continue to the review" }) as HTMLButtonElement).disabled).toBe(false));
});

it("reads another account of the file, a card's purchases as positive, and another sheet", async () => {
  engine((s) =>
    statement(s, [bakery], {
      sources: [
        { label: "A1", lines: 1 },
        { label: "B2", currency: "EUR", lines: 3 },
      ],
      source: 0,
      dateOrder: { order: "ymd", example: "x", unconfirmed: true },
    }),
  );
  vi.mocked(api.decodeStatement).mockImplementation(async (latin1, sheet) => ({
    text: latin1,
    encoding: "spreadsheet",
    sheet: sheet ?? "Movimientos",
    sheets: ["Resumen", "Movimientos"],
  }));
  render(<Page />);
  choose(new File([new Uint8Array([0x50, 0x4b, 0x03, 0x04])], "Movimientos.xlsx"));
  fireEvent.change(await screen.findByLabelText("Account in the file"), { target: { value: "1" } });
  await waitFor(() => expect(sent().structure).toMatchObject({ source: "B2" }));
  fireEvent.change(screen.getByLabelText("Purchases in the file"), { target: { value: "positive" } });
  await waitFor(() => expect(sent().structure).toMatchObject({ flipSigns: true }));
  expect(api.bankSession).toHaveBeenLastCalledWith(
    expect.anything(),
    "checking",
    expect.any(String),
    expect.any(String),
    "b",
    "Movimientos",
    false,
  );
  // Another sheet is another reading of the file: a new session.
  fireEvent.click(screen.getByRole("tab", { name: "Resumen" }));
  await waitFor(() => expect(api.decodeStatement).toHaveBeenLastCalledWith(expect.any(String), "Resumen"));
  await waitFor(() =>
    expect(api.bankSession).toHaveBeenLastCalledWith(
      expect.anything(),
      "checking",
      expect.any(String),
      expect.any(String),
      "b",
      "Resumen",
      false,
    ),
  );
});

it("chooses what a column holds, imports a batch as one entry, and puts a line back as the file has it", async () => {
  const anna = line(
    "e2.1",
    ["2026-10-03", "Anna", "100.00"],
    { kind: "attention", field: "amount", column: null, reason: "These payments add up to 350.00, not the entry's 300.00." },
    { batch: 0 },
  );
  const ben = line(
    "e2.2",
    ["2026-10-03", "Ben", "250.00"],
    { kind: "attention", field: "amount", column: null, reason: "These payments add up to 350.00, not the entry's 300.00." },
    { batch: 0 },
  );
  const edited = line(
    "e3",
    ["2026-10-04", "Shop", "-4.00"],
    { kind: "ready" },
    { fixed: true, version: 2, original: ["2026-10-04", "Shop", "-4.0O"] },
  );
  engine((s) =>
    statement(s, [anna, ben, edited], { batches: [{ entry: "e2", payments: [0, 1], total: 30000, addsUp: false, asOne: false }] }),
  );
  render(<Page />);
  choose(csv());
  fireEvent.click(await screen.findByRole("button", { name: "Import as one entry" }));
  await waitFor(() => expect(sent().structure).toMatchObject({ wholeBatches: ["e2"] }));
  // Each change waits for the file to read again, so it's made against the lines as they are.
  const ready = () => waitFor(() => expect((screen.getByLabelText("What column 3 holds") as HTMLSelectElement).disabled).toBe(false));
  await ready();
  fireEvent.click(screen.getByRole("tab", { name: /Edited/ }));
  fireEvent.click(await screen.findByRole("button", { name: "Put line e3 back as the file has it" }));
  await waitFor(() => expect(sent()).toMatchObject({ revert: [{ line: "e3", lineVersion: 2 }] }));
  await ready();
  fireEvent.change(screen.getByLabelText("What column 3 holds"), { target: { value: "outflow" } });
  await waitFor(() => expect(sent().structure?.columns).toEqual({ date: "Date", payee: "Payee", amount: null, outflow: "Amount" }));
});

it("says why a fix didn't apply, and drops a fix made for a file since replaced", async () => {
  engine((s) => statement(s, [bakery, grocer]));
  vi.mocked(api.bankApplyFixes).mockResolvedValueOnce({
    session: session(),
    results: [{ line: "30", fix: "cell", applied: false, problem: "This line changed since this fix was made." }],
  });
  render(<Page />);
  choose(csv());
  await screen.findByRole("button", { name: "Fix line 30" });
  fixCell("30", "Amount", "-1.00");
  expect(await screen.findByText("This line changed since this fix was made.")).toBeTruthy();
  // A fix still on its way when another file is chosen is for an import that's gone.
  let finish!: (v: Awaited<ReturnType<typeof api.bankApplyFixes>>) => void;
  vi.mocked(api.bankApplyFixes).mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
  await screen.findByRole("cell", { name: "Amount of line 30: n/a" });
  fixCell("30", "Amount", "-2.00");
  fireEvent.click(screen.getByRole("button", { name: "Choose another file" }));
  vi.mocked(api.bankSession).mockResolvedValue(session("s2"));
  choose(csv("november.csv"));
  await waitFor(() =>
    expect(api.bankStatement).toHaveBeenLastCalledWith(expect.anything(), "checking", expect.any(String), session("s2"), expect.anything()),
  );
  finish({ session: session("s1", 5), results: [{ fix: "cell", applied: true }] });
  await new Promise((r) => setTimeout(r, 250));
  expect(vi.mocked(api.bankStatement).mock.calls.every(([, , , s]) => s.scope.session === "s2" || s.revision === 0)).toBe(true);
});

it("checks the review again when the budget changes, instead of failing", async () => {
  engine((s) => statement(s, [bakery]));
  const b = demoBudget();
  const view = render(<Page budget={b} />);
  choose(csv());
  await screen.findByRole("button", { name: "Import 1 transaction" });
  expect(api.bankStatementPreview).toHaveBeenCalledTimes(1);
  const changed = { ...b, name: "Changed" };
  view.rerender(<Page budget={changed} />);
  await waitFor(() =>
    expect(api.bankStatementPreview).toHaveBeenLastCalledWith(changed, "checking", expect.any(String), expect.anything()),
  );
});

it("sends a PDF statement to the assistant, and saves a template", async () => {
  const ask = vi.fn();
  render(<Page onAskAssistant={ask} />);
  const pdf = new File(["%PDF-1.7"], "Kontoauszug_2026_10.pdf");
  choose(pdf);
  expect(await screen.findByText(/PDF statements need the assistant/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Read it with the assistant" }));
  expect(ask).toHaveBeenCalledWith(pdf, "", "Everyday account");
  expect(api.bankSession).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Choose another file" }));
  fireEvent.click(screen.getByRole("button", { name: /Download a template/ }));
  await waitFor(() =>
    expect(api.writeExport).toHaveBeenCalledWith("bank-import-template.csv", expect.stringContaining("Date,Payee,Memo,Outflow,Inflow")),
  );
  expect(await screen.findByText("Template saved.")).toBeTruthy();
  // A cancelled save panel says nothing.
  vi.mocked(api.writeExport).mockResolvedValueOnce(null);
  fireEvent.click(screen.getByRole("button", { name: /Download a template/ }));
  await waitFor(() => expect(api.writeExport).toHaveBeenCalledTimes(2));
  expect(screen.getAllByText("Template saved.")).toHaveLength(1);
});

it("offers the assistant first when no line reads, with why", async () => {
  engine((s) => statement(s, [grocer]));
  const ask = vi.fn();
  render(<Page onAskAssistant={ask} />);
  const file = csv();
  choose(file);
  await screen.findByText("Invalid amount: n/a");
  fireEvent.click(screen.getByRole("button", { name: /Fix with the assistant/ }));
  expect(ask).toHaveBeenCalledWith(file, "1 lines don't read.", "Everyday account");
  // Nothing reads, so there's no review to continue to.
  expect((screen.getByRole("button", { name: "Continue to the review" }) as HTMLButtonElement).disabled).toBe(true);
});

it("asks the assistant to fix the lines that need attention, and marks the lines it fixed", async () => {
  const fixed = line(
    "30",
    ["2026-10-02", "Grocer", "-12.40"],
    { kind: "ready" },
    { fixed: true, version: 1, original: ["2026-10-02", "Grocer", "n/a"] },
  );
  engine((s) => statement(s, s.revision ? [bakery, fixed] : [bakery, grocer]));
  const ask = vi.fn();
  const view = render(<Page onFixWithAssistant={ask} />);
  choose(csv());
  fireEvent.click(await screen.findByRole("button", { name: /Fix with the assistant/ }));
  expect(ask).toHaveBeenCalledTimes(1);
  cleanup();
  view.unmount();
  // The assistant's fixes came back: its lines say so.
  render(
    <Page initial={{ account: "checking", file: csv(), content: "x", session: session("s1", 1), view: "editor", assisted: ["30"] }} />,
  );
  fireEvent.click(await screen.findByRole("tab", { name: /Edited/ }));
  expect((await screen.findByRole("button", { name: "Put line 30 back as the file has it" })).title).toBe(
    "Fixed by the assistant · put back as the file has it",
  );
});
