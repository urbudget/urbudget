import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { BankFixes, BankSession, Statement } from "../bridge";
import type { BankColumns } from "../types";
import { demoBudget } from "../demo";
import { configureFormat } from "../format";
import { StatementGrid } from "./StatementGrid";

afterEach(() => { cleanup(); configureFormat(demoBudget()); });

const budget = () => ({ ...demoBudget(), payees: [{ id: "shop", name: "Known Grocer" }] });
function fixture(headers = ["When", "Counterparty", "Value"], columns: BankColumns = { date: "When", payee: "Counterparty", amount: "Value" }, cells = ["2026-10-01", "Bank wording", "-10.00"]) {
  const session: BankSession = {
    scope: { session: "s", budget: "b", account: "checking", file: "f" }, revision: 0,
    structure: { format: "csv", dateOrder: { order: "ymd", confirmed: true }, flipSigns: false },
    edits: { cells: {}, fields: {}, include: [] }, versions: {},
  };
  const statement: Statement = {
    scope: session.scope, revision: 0, structure: session.structure, format: "csv", headers, columns,
    order: "ymd", counts: { ready: 1, attention: 0, leftOut: {} }, remember: {},
    lines: [{ id: "1", version: 0, cells, fields: { date: "2026-10-01", payee: "Bank wording", amount: "-10.00", memo: "" }, status: { kind: "ready" } }],
  };
  const onFix = vi.fn<(fixes: BankFixes) => void>();
  const props = { budget: budget(), session, statement, onFix, busy: false, assisted: new Set<string>(), onContinue: vi.fn() };
  return { props, onFix };
}
function edit(column: string, value: string) {
  fireEvent.doubleClick(screen.getByRole("cell", { name: `${column} of line 1: ${value}` }));
  return screen.getByLabelText(`${column} of line 1`);
}

it.each(["mouse", "keyboard"])("selects a known payee using %s in the mapped payee column", (method) => {
  const { props, onFix } = fixture();
  render(<StatementGrid {...props} />);
  const input = edit("Counterparty", "Bank wording");
  fireEvent.change(input, { target: { value: "Known" } });
  if (method === "mouse") fireEvent.click(screen.getByRole("option", { name: "Known Grocer" }));
  else fireEvent.keyDown(input, { key: "Enter" });
  expect((input as HTMLInputElement).value).toBe("Known Grocer");
  expect(onFix).not.toHaveBeenCalled();
  fireEvent.keyDown(input, { key: "Enter" });
  expect(onFix).toHaveBeenCalledWith(expect.objectContaining({ cells: [{ line: "1", lineVersion: 0, column: 1, from: "Bank wording", to: "Known Grocer" }] }));
});

it.each([
  ["amount", "10+2.50", "12.50"],
  ["outflow", "10+2.50", "-12.50"],
  ["inflow", "10+2.50", "12.50"],
] as const)("calculates an explicitly edited %s and sends the signed final amount", (role, input, expected) => {
  const { props, onFix } = fixture(undefined, { date: "When", payee: "Counterparty", [role]: "Value" });
  render(<StatementGrid {...props} />);
  const field = edit("Value", "-10.00");
  fireEvent.change(field, { target: { value: input } });
  fireEvent.keyDown(field, { key: "Enter" });
  expect(onFix.mock.lastCall![0].fields).toEqual([{ line: "1", lineVersion: 0, field: "amount", from: "-10.00", to: expected }]);
  expect(onFix.mock.lastCall![0].cells?.[0].to).toBe("12.50");
});

it.each([
  ["Out", "10", "+25", "In", "25.00"],
  ["In", "empty", "-25", "Out", "-25.00"],
] as const)("moves signed input from %s to its other flow as the register does", (column, old, text, other, expected) => {
  const { props, onFix } = fixture(["When", "Counterparty", "Out", "In"], { date: "When", payee: "Counterparty", outflow: "Out", inflow: "In" }, ["2026-10-01", "Bank wording", "10", ""]);
  render(<StatementGrid {...props} />);
  const input = edit(column, old);
  fireEvent.change(input, { target: { value: text } });
  expect(document.activeElement).toBe(screen.getByLabelText(`${other} of line 1`));
  fireEvent.keyDown(document.activeElement!, { key: "Enter" });
  expect(onFix.mock.lastCall![0].fields?.[0].to).toBe(expected);
});

it("retains invalid calculations for correction without saving the row", () => {
  const { props, onFix } = fixture();
  render(<StatementGrid {...props} />);
  const field = edit("Value", "-10.00");
  fireEvent.change(field, { target: { value: "10/0" } });
  fireEvent.keyDown(field, { key: "Enter" });
  expect(screen.getByRole("alert").textContent).toContain("Enter an amount");
  expect((field as HTMLInputElement).value).toBe("10/0");
  expect(onFix).not.toHaveBeenCalled();
  fireEvent.change(field, { target: { value: "10/2" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(onFix.mock.lastCall![0].fields?.[0].to).toBe("5.00");
});

it("uses the budget decimal mark for entered calculations", () => {
  configureFormat({ currency: "EUR", numberFormat: { decimal: ",", group: "." } });
  const { props, onFix } = fixture();
  render(<StatementGrid {...props} />);
  const field = edit("Value", "-10.00");
  fireEvent.change(field, { target: { value: "10,50+2" } });
  fireEvent.keyDown(field, { key: "Enter" });
  expect(onFix.mock.lastCall![0].fields?.[0].to).toBe("12.50");
  expect(onFix.mock.lastCall![0].cells?.[0].to).toBe("12,50");
});

it("opens bank accounting amounts as register values without editing untouched source cells", () => {
  const { props, onFix } = fixture(undefined, undefined, ["2026-10-01", "Bank wording", "(10)"]);
  render(<StatementGrid {...props} />);
  const amount = edit("Value", "(10)");
  expect((amount as HTMLInputElement).value).toBe("-10.00");
  const payee = screen.getByLabelText("Counterparty of line 1");
  fireEvent.change(payee, { target: { value: "New merchant" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(onFix.mock.lastCall![0].cells).toEqual([{ line: "1", lineVersion: 0, column: 1, from: "Bank wording", to: "New merchant" }]);
  expect(onFix.mock.lastCall![0].fields).toBeUndefined();
});

it("switches from a mapped Amount to flows without counting fallback columns twice", () => {
  const { props, onFix } = fixture(["When", "Counterparty", "Value", "Out", "In"],
    { date: "When", payee: "Counterparty", amount: "Value", outflow: "Out", inflow: "In" },
    ["2026-10-01", "Bank wording", "(10)", "(999)", "50 CR"]);
  render(<StatementGrid {...props} />);
  const out = edit("Out", "(999)");
  expect((out as HTMLInputElement).value).toBe("");
  expect((screen.getByLabelText("In of line 1") as HTMLInputElement).value).toBe("");
  fireEvent.change(out, { target: { value: "10+5" } });
  fireEvent.keyDown(out, { key: "Enter" });
  expect(onFix.mock.lastCall![0].fields?.[0].to).toBe("-15.00");
  expect(onFix.mock.lastCall![0].cells).toContainEqual({ line: "1", lineVersion: 0, column: 2, from: "(10)", to: "" });
});

it("switches from fallback flows to Amount using the signed calculated result", () => {
  const { props, onFix } = fixture(["When", "Counterparty", "Value", "Out", "In"],
    { date: "When", payee: "Counterparty", amount: "Value", outflow: "Out", inflow: "In" },
    ["2026-10-01", "Bank wording", "", "(10)", ""]);
  render(<StatementGrid {...props} />);
  const amount = edit("Value", "empty");
  expect((screen.getByLabelText("Out of line 1") as HTMLInputElement).value).toBe("10.00");
  fireEvent.change(amount, { target: { value: "-20+5" } });
  fireEvent.keyDown(amount, { key: "Enter" });
  expect(onFix.mock.lastCall![0].fields?.[0].to).toBe("-15.00");
});

it("opens resolved money in the budget locale before an expression is appended", () => {
  configureFormat({ currency: "EUR", numberFormat: { decimal: ",", group: "." } });
  const { props, onFix } = fixture();
  render(<StatementGrid {...props} />);
  const amount = edit("Value", "-10.00") as HTMLInputElement;
  expect(amount.value).toBe("-10,00");
  fireEvent.change(amount, { target: { value: `${amount.value}+2,50` } });
  fireEvent.keyDown(amount, { key: "Enter" });
  expect(onFix.mock.lastCall![0].fields?.[0].to).toBe("-7.50");
});

it("keeps raw bank values when a calculation equals the effective amount", () => {
  const { props, onFix } = fixture(undefined, undefined, ["2026-10-01", "Bank wording", "10"]);
  props.statement.lines[0].fields.amount = "9.00"; // The file deducted a fee.
  render(<StatementGrid {...props} />);
  const amount = edit("Value", "10") as HTMLInputElement;
  expect(amount.value).toBe("9.00");
  fireEvent.change(amount, { target: { value: "4+5" } });
  fireEvent.keyDown(amount, { key: "Enter" });
  expect(onFix).not.toHaveBeenCalled();
  expect(screen.getByRole("cell", { name: "Value of line 1: 10" })).toBeTruthy();
});

it("preserves the opened canonical amount precondition when an assistant edit arrives during a human draft", () => {
  const { props, onFix } = fixture(undefined, undefined, ["2026-10-01", "Bank wording", "10"]);
  props.session.structure.flipSigns = true;
  const view = render(<StatementGrid {...props} />);
  const amount = edit("Value", "10");
  fireEvent.change(amount, { target: { value: "-8-4" } });
  const changed = { ...props.statement, revision: 1, lines: [{ ...props.statement.lines[0], version: 1, fields: { ...props.statement.lines[0].fields, amount: "-20.00" } }] };
  view.rerender(<StatementGrid {...props} statement={changed} session={{ ...props.session, revision: 1 }} />);
  fireEvent.keyDown(amount, { key: "Enter" });
  // The engine atomically checks this bundle; the original value must not be rebased onto -20.
  expect(onFix.mock.lastCall![0]).toMatchObject({
    baseRevision: 1,
    cells: [{ line: "1", lineVersion: 1, column: 2, from: "10", to: "-12.00" }],
    fields: [{ line: "1", lineVersion: 1, field: "amount", from: "-10.00", to: "-12.00" }],
  });
});

it("uses the parsed flow amount even when a CSV date still needs correction", () => {
  const { props, onFix } = fixture(["When", "Counterparty", "Out"], { date: "When", payee: "Counterparty", outflow: "Out" }, ["bad date", "Bank wording", "(10)"]);
  props.statement.lines[0].status = { kind: "attention", field: "date", column: 0, reason: "Invalid date" };
  props.statement.counts = { ready: 0, attention: 1, leftOut: {} };
  render(<StatementGrid {...props} />);
  const amount = edit("Out", "(10)") as HTMLInputElement;
  expect(amount.value).toBe("10.00");
  fireEvent.change(amount, { target: { value: `${amount.value}+2` } });
  fireEvent.keyDown(amount, { key: "Enter" });
  expect(onFix.mock.lastCall![0].fields?.[0].to).toBe("-12.00");
});

it("selects, reopens and clears a structured transfer while retaining the bank payee", () => {
  const { props, onFix } = fixture();
  const view = render(<StatementGrid {...props} />);
  const payee = edit("Counterparty", "Bank wording");
  fireEvent.change(payee, { target: { value: "Savings" } });
  fireEvent.click(screen.getByRole("option", { name: "Transfer: Savings" }));
  fireEvent.keyDown(payee, { key: "Enter" });
  expect(onFix.mock.lastCall![0]).toMatchObject({ cells: [], fields: [{ field: "transferAccountId", from: "", to: "savings" }] });
  const changed = { ...props.statement, revision: 1, lines: [{ ...props.statement.lines[0], version: 1, fixed: true, fields: { ...props.statement.lines[0].fields, transferAccountId: "savings" } }] };
  view.rerender(<StatementGrid {...props} statement={changed} session={{ ...props.session, revision: 1 }} />);
  const reopened = edit("Counterparty", "Transfer: Savings");
  expect((reopened as HTMLInputElement).value).toBe("Transfer: Savings");
  fireEvent.change(reopened, { target: { value: "Known" } });
  fireEvent.click(screen.getByRole("option", { name: "Known Grocer" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(onFix.mock.lastCall![0].fields).toEqual([{ line: "1", lineVersion: 1, field: "transferAccountId", from: "savings", to: "" }]);
  expect(onFix.mock.lastCall![0].cells?.[0]).toMatchObject({ from: "Bank wording", to: "Known Grocer" });
});

it("does not interpret an unmapped column as money just because its header says Amount", () => {
  const { props, onFix } = fixture(["When", "Counterparty", "Amount"], { date: "When", payee: "Counterparty", memo: "Amount" });
  render(<StatementGrid {...props} />);
  const field = edit("Amount", "-10.00");
  fireEvent.change(field, { target: { value: "10+2.50" } });
  fireEvent.keyDown(field, { key: "Enter" });
  expect(onFix.mock.lastCall![0].fields).toBeUndefined();
  expect(onFix.mock.lastCall![0].cells?.[0].to).toBe("10+2.50");
});
