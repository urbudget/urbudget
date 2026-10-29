import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import type { Budget, Transaction } from "./types";

vi.mock("./bridge", () => ({
  api: {
    previewBank: vi.fn(async () => [
      { id: "r1", date: "2026-10-03", payee: "Grocery store", memo: "", amount: -8540, duplicate: false, candidates: [] },
    ]),
    decodeStatement: vi.fn(async (latin1: string) => ({ text: latin1, encoding: "ISO-8859-1" })),
  },
}));
import { api } from "./bridge";
import {
  ImportReview,
  bankTemplate,
  categorizeText,
  importHelpText,
  statementFile,
  statementText,
  uncategorizedImports,
  type ReviewChoices,
} from "./BankImport";
import { demoBudget } from "./demo";

afterEach(cleanup);
beforeEach(() => vi.clearAllMocks());

it("shows an explicit transfer destination and keeps its account ID when importing a new row", () => {
  const onImport = vi.fn();
  const row = { id: "bank:transfer", date: "2026-10-03", payee: "Bank wording", memo: "", amount: -2500, duplicate: false, candidates: [], transferAccountId: "savings" };
  render(<ImportReview budget={demoBudget()} rows={[row]} busy={false} icon={null} onCancel={() => {}} onImport={onImport} />);
  expect(screen.getByText("Transfer to Savings")).toBeTruthy();
  expect(screen.getByText("Bank wording")).toBeTruthy();
  expect(screen.getByRole("option", { name: "Add as a transfer" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  expect(onImport.mock.lastCall![0]).toEqual([{ row, action: "new", matchId: null }]);
});

it("lets an identical ID-less receipt be imported separately with a stable retry identity", () => {
  const onImport = vi.fn();
  const row = { id: "bank:known", date: "2026-10-03", payee: "Shop", memo: "Second receipt", amount: -500, duplicate: true, idless: true, candidates: ["manual"] };
  render(<ImportReview budget={demoBudget()} rows={[row]} busy={false} icon={null} title="File" onCancel={() => {}} onImport={onImport} />);
  const action = screen.getByLabelText("Import action 1");
  expect(screen.queryByRole("option", { name: /^Match:/ })).toBeNull();
  expect(screen.getByRole("button", { name: "Import 0 transactions" }).hasAttribute("disabled")).toBe(true);
  fireEvent.change(action, { target: { value: "separate" } });
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  expect(onImport).toHaveBeenCalledTimes(2);
  const first = onImport.mock.calls[0][0][0];
  expect(first.action).toBe("new");
  expect(first.row.id).toBe(row.id);
  expect(first.row.importId).toMatch(/^bank:manual:/);
  expect(onImport.mock.calls[1][0][0].row.importId).toBe(first.row.importId);
  expect(screen.getByText("Second receipt")).toBeTruthy();
});

it("keeps its choices with whoever shows it, checked against the rows as they are, with a separate row's ID", () => {
  const known = { id: "bank:known", date: "2026-10-03", payee: "Shop", memo: "", amount: -500, duplicate: true, idless: true, candidates: [] };
  // Chosen to match a transaction since gone.
  const fee = { id: "bank:fee", date: "2026-10-04", payee: "Fee", memo: "", amount: -100, duplicate: false, candidates: [] };
  const onImport = vi.fn();
  function Kept({ shown }: { shown: boolean }) {
    const [choices, setChoices] = useState<ReviewChoices>({ picked: { "bank:fee": "match:gone" } });
    if (!shown) return null;
    const props = { budget: demoBudget(), busy: false, icon: null, onCancel: () => {}, onImport };
    return <ImportReview {...props} rows={[known, fee]} choices={choices} onChoices={setChoices} />;
  }
  const view = render(<Kept shown />);
  const value = (n: number) => (screen.getByLabelText(`Import action ${n}`) as HTMLSelectElement).value;
  expect(value(2)).toBe("review");
  fireEvent.change(screen.getByLabelText("Import action 1"), { target: { value: "separate" } });
  fireEvent.change(screen.getByLabelText("Import action 2"), { target: { value: "skip" } });
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  // Shown again: the same choices, and the same ID for the row added separately.
  view.rerender(<Kept shown={false} />);
  view.rerender(<Kept shown />);
  expect([value(1), value(2)]).toEqual(["separate", "skip"]);
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  const [first, again] = onImport.mock.calls.map(([choices]) => choices[0].row.importId);
  expect(first).toMatch(/^bank:manual:/);
  expect(again).toBe(first);
});

it("skips a chosen row when a refreshed preview confirms it was imported", () => {
  const row = { id: "bank:pending", date: "2026-10-03", payee: "Shop", memo: "", amount: -500, duplicate: false, idless: true, candidates: [] };
  const props = { budget: demoBudget(), busy: false, icon: null, title: "File", onCancel: () => {}, onImport: vi.fn() };
  const view = render(<ImportReview {...props} rows={[row]} />);
  fireEvent.change(screen.getByLabelText("Import action 1"), { target: { value: "new" } });
  view.rerender(<ImportReview {...props} rows={[{ ...row, duplicate: true }]} />);
  expect((screen.getByLabelText("Import action 1") as HTMLSelectElement).value).toBe("skip");
  expect(screen.getByRole("button", { name: "Import 0 transactions" }).hasAttribute("disabled")).toBe(true);
});

it("requires another choice when a refreshed preview removes a selected match", () => {
  const row = { id: "bank:pending", date: "2026-10-03", payee: "Shop", memo: "", amount: -500, duplicate: false, candidates: ["original", "other"] };
  const props = { budget: demoBudget(), busy: false, icon: null, title: "Bank", onCancel: () => {}, onImport: vi.fn() };
  const view = render(<ImportReview {...props} rows={[row]} />);
  fireEvent.change(screen.getByLabelText("Import action 1"), { target: { value: "match:original" } });
  view.rerender(<ImportReview {...props} rows={[{ ...row, candidates: [] }]} />);
  expect((screen.getByLabelText("Import action 1") as HTMLSelectElement).value).toBe("review");
  const importButton = screen.getByRole("button", { name: "Import 1 transaction" });
  expect(importButton.hasAttribute("disabled")).toBe(true);
  fireEvent.click(importButton);
  expect(props.onImport).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText("Import action 1"), { target: { value: "new" } });
  fireEvent.click(importButton);
  expect(props.onImport.mock.calls[0][0][0]).toMatchObject({ action: "new", matchId: null });
});

it("keeps bank-provided IDs deduplicated and requires a choice for legacy ambiguous imports", () => {
  const onImport = vi.fn();
  const row = { id: "bank:legacy-new-identity", date: "2026-10-03", payee: "Shop", memo: "Receipt", amount: -500, duplicate: false, idless: true, possibleDuplicate: true, candidates: ["manual"] };
  render(<ImportReview budget={demoBudget()} rows={[row, { ...row, id: "bank:fit", idless: false, duplicate: true, possibleDuplicate: false }]} busy={false} icon={null} title="File" onCancel={() => {}} onImport={onImport} />);
  expect((screen.getByLabelText("Import action 1") as HTMLSelectElement).value).toBe("review");
  expect(screen.queryByLabelText("Import action 2")).toBeNull();
  expect(screen.getByRole("button", { name: "Import 1 transaction" }).hasAttribute("disabled")).toBe(true);
  fireEvent.change(screen.getByLabelText("Import action 1"), { target: { value: "new" } });
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  expect(onImport.mock.calls[0][0].map((c: { action: string }) => c.action)).toEqual(["new", "skip"]);
});

it.each([false, true])("requires an explicit match for a previously imported account side (split: %s)", (split) => {
  const b = demoBudget();
  const t = { ...b.transactions[0], id: "payment", payee: "Shop", memo: "Receipt", splits: [{ amount: -500, categoryId: null }] };
  const id = split ? "receiving-leg" : t.id;
  const row = { id: "bank:other-source", date: "2026-10-03", payee: "Shop", memo: "", amount: split ? 500 : -500, duplicate: false, candidates: [id], previouslyMatchedCandidates: [id],
    candidateDetails: split ? [{ id, transactionId: t.id, splitIndex: 0 }] : [] };
  const onImport = vi.fn();
  render(<ImportReview budget={{ ...b, transactions: [t] }} rows={[row]} busy={false} icon={null} title="Bank" onCancel={() => {}} onImport={onImport} />);
  const action = screen.getByLabelText("Import action 1") as HTMLSelectElement;
  expect(action.value).toBe("review");
  expect(screen.getByRole("option", { name: /Match:.*Previously imported/ })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Import 1 transaction" }).hasAttribute("disabled")).toBe(true);
  fireEvent.change(action, { target: { value: `match:${id}` } });
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  expect(onImport).toHaveBeenCalledWith([{ row, action: "match", matchId: id }], 1);
});

it("lets a distinct receipt be added instead of automatically merging into an imported payment", () => {
  const row = { id: "bank:receipt-b", date: "2026-10-03", payee: "Shop", memo: "Receipt B", amount: -500, duplicate: false, idless: true, candidates: ["payment"], previouslyMatchedCandidates: ["payment"] };
  const onImport = vi.fn();
  render(<ImportReview budget={demoBudget()} rows={[row]} busy={false} icon={null} title="File" onCancel={() => {}} onImport={onImport} />);
  expect((screen.getByLabelText("Import action 1") as HTMLSelectElement).value).toBe("review");
  fireEvent.change(screen.getByLabelText("Import action 1"), { target: { value: "new" } });
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  expect(onImport).toHaveBeenCalledWith([{ row, action: "new", matchId: null }], 1);
});

it("still defaults to matching one account side that has not been imported", () => {
  const row = { id: "bank:new", date: "2026-10-03", payee: "Shop", memo: "", amount: -500, duplicate: false, candidates: ["payment"] };
  const onImport = vi.fn();
  render(<ImportReview budget={demoBudget()} rows={[row]} busy={false} icon={null} title="File" onCancel={() => {}} onImport={onImport} />);
  expect((screen.getByLabelText("Import action 1") as HTMLSelectElement).value).toBe("match:payment");
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  expect(onImport).toHaveBeenCalledWith([{ row, action: "match", matchId: "payment" }], 1);
});

it("labels and submits receiving split legs independently", () => {
  const b = demoBudget();
  const t = { ...b.transactions[0], id: "parent", payee: "Transfer", splits: [{ amount: -500, categoryId: null, memo: "First leg" }, { amount: -500, categoryId: null, memo: "Second leg" }] };
  const row = { id: "bank:one", date: "2026-10-03", payee: "Transfer", memo: "", amount: 500, duplicate: false, candidates: ["leg-a", "leg-b"], candidateDetails: [{ id: "leg-a", transactionId: "parent", splitIndex: 0 }, { id: "leg-b", transactionId: "parent", splitIndex: 1 }] };
  const onImport = vi.fn();
  render(<ImportReview budget={{ ...b, transactions: [t] }} rows={[row, { ...row, id: "bank:two" }]} busy={false} icon={null} title="Bank" onCancel={() => {}} onImport={onImport} />);
  expect(screen.getAllByRole("option", { name: /Split 1 · First leg/ })).toHaveLength(2);
  expect(screen.getAllByRole("option", { name: /Split 2 · Second leg/ })).toHaveLength(2);
  fireEvent.change(screen.getByLabelText("Import action 1"), { target: { value: "match:leg-a" } });
  fireEvent.change(screen.getByLabelText("Import action 2"), { target: { value: "match:leg-b" } });
  fireEvent.click(screen.getByRole("button", { name: "Import 2 transactions" }));
  expect(onImport.mock.calls[0][0].map((c: { matchId: string }) => c.matchId)).toEqual(["leg-a", "leg-b"]);
});

it("makes a template in the chosen date order", () => {
  expect(bankTemplate("dmy", "2026-10").split("\r\n")).toEqual([
    "Date,Payee,Memo,Outflow,Inflow",
    "01/10/2026,Employer,Salary,,5000.00",
    "03/10/2026,Grocery store,Weekly shop,85.40,",
    "05/10/2026,Electric company,Monthly bill,120.00,",
    "",
  ]);
  expect(bankTemplate("ymd", "2026-10")).toContain("2026-10-03,Grocery store");
  expect(bankTemplate("mdy", "2026-10")).toContain("10/05/2026,Electric company");
});

it("offers the other side of money moved from another account as a transfer", () => {
  const b = demoBudget();
  const move: Transaction = { ...imported("move", "savings", null, -20000), payee: "TO EVERYDAY", date: "2026-10-02" };
  const row = { id: "bank:in", date: "2026-10-03", payee: "From savings", memo: "", amount: 20000, duplicate: false, candidates: ["move"], transferCandidates: ["move"], transferSure: true };
  const onImport = vi.fn();
  render(<ImportReview budget={{ ...b, transactions: [move] }} rows={[row]} busy={false} icon={null} title="File" onCancel={() => {}} onImport={onImport} />);
  expect((screen.getByLabelText("Import action 1") as HTMLSelectElement).value).toBe("match:move");
  expect(screen.getByRole("option", { name: /^Transfer from Savings: .* · TO EVERYDAY$/ })).toBeTruthy();
  expect(screen.getByText(/1 transfer between your accounts/)).toBeTruthy();
  expect(screen.getByText(/isn't counted as spending or income/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  expect(onImport).toHaveBeenCalledWith([{ row, action: "match", matchId: "move" }], 1);
});

it("only suggests a transfer the bank text doesn't back up, and pairs the suggestions in one go", () => {
  const b = demoBudget();
  const move = (id: string): Transaction => ({ ...imported(id, "savings", null, -20000), payee: "A. Example", date: "2026-10-02" });
  const row = (id: string, other: string) => ({ id, date: "2026-10-03", payee: "Incoming", memo: "", amount: 20000, duplicate: false, candidates: [other], transferCandidates: [other] });
  const rows = [row("bank:a", "m1"), row("bank:b", "m2"), { ...row("bank:c", "m3"), transferSure: true }];
  const onImport = vi.fn();
  render(<ImportReview budget={{ ...b, transactions: [move("m1"), move("m2"), move("m3")] }} rows={rows} busy={false} icon={null} title="File" onCancel={() => {}} onImport={onImport} />);
  const first = screen.getByLabelText("Import action 1") as HTMLSelectElement;
  expect([first.value, first.selectedOptions[0].textContent]).toEqual(["review", "Transfer suggested — check it and choose"]);
  expect((screen.getByLabelText("Import action 3") as HTMLSelectElement).value).toBe("match:m3");
  expect(screen.getByText("2 rows need a choice")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Pair 2 suggested transfers" }));
  expect(screen.queryByRole("button", { name: /suggested transfer|as new/ })).toBeNull();
  expect(screen.getByText(/3 transfers between your accounts/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Import 3 transactions" }));
  expect(onImport.mock.calls[0][0].map((c: { matchId: string }) => c.matchId)).toEqual(["m1", "m2", "m3"]);
});

it("decides every row still undecided at once", () => {
  const row = (id: string) => ({ id, date: "2026-10-03", payee: "Fee", memo: "", amount: -100, duplicate: false, candidates: ["a", "b"] });
  const onImport = vi.fn();
  render(<ImportReview budget={demoBudget()} rows={[row("one"), row("two"), { ...row("three"), candidates: [] }]} busy={false} icon={null} title="File" onCancel={() => {}} onImport={onImport} />);
  expect(screen.getByText("2 rows need a choice")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Skip them" }));
  fireEvent.click(screen.getByRole("button", { name: "Import 1 transaction" }));
  expect(onImport.mock.calls[0][0].map((c: { action: string }) => c.action)).toEqual(["skip", "skip", "new"]);
});

it("lists the nearest fifty of thousands of possible matches", () => {
  const b = demoBudget();
  // 120 earlier charges of the same amount, one a day back from the row's date.
  const fees: Transaction[] = Array.from({ length: 120 }, (_, i) => ({
    ...imported(`fee${i}`, "checking", "groceries", -100),
    date: new Date(Date.UTC(2026, 9, 20 - i)).toISOString().slice(0, 10),
  }));
  const ids = fees.map((t) => t.id);
  const row = { id: "bank:fee", date: "2026-10-20", payee: "Fee", memo: "", amount: -100, duplicate: false, candidates: [...ids].reverse(), previouslyMatchedCandidates: ids };
  render(<ImportReview budget={{ ...b, transactions: fees }} rows={[row]} busy={false} icon={null} title="File" onCancel={() => {}} onImport={() => {}} />);
  const matches = screen.getAllByRole("option").filter((o) => (o as HTMLOptionElement).value.startsWith("match:"));
  expect(matches.map((o) => (o as HTMLOptionElement).value)).toEqual(ids.slice(0, 50).map((id) => `match:${id}`));
  expect(screen.getByRole("option", { name: "70 more of the same amount within ten days aren't listed" })).toBeTruthy();
});

it("reads statements in the encodings banks write", async () => {
  const bytes = (...values: number[]) => new Blob([new Uint8Array(values)]);
  // Not UTF-8 (Windows-1250, as Polish banks export): the engine tells which encoding, given a
  // character for each byte.
  vi.mocked(api.decodeStatement).mockResolvedValueOnce({ text: "Tytuł;ŻABKA", encoding: "windows-1250" });
  expect(await statementText(bytes(0x54, 0x79, 0x74, 0x75, 0xb3, 0x3b, 0xaf, 0x41, 0x42, 0x4b, 0x41))).toBe("Tytuł;ŻABKA");
  expect(api.decodeStatement).toHaveBeenCalledWith("Tytu\u00b3;\u00afABKA", null);
  // An Excel workbook (a ZIP inside) goes to the engine too, which gives back its sheet as CSV.
  vi.mocked(api.decodeStatement).mockResolvedValueOnce({ text: "Date,Payee,Amount", encoding: "spreadsheet" });
  expect(await statementText(bytes(0x50, 0x4b, 0x03, 0x04, 0x14, 0x00, 0x06, 0x00, 0x08, 0x00))).toBe("Date,Payee,Amount");
  // UTF-8, with and without a byte-order mark.
  expect(await statementText(bytes(0xef, 0xbb, 0xbf, 0x42, 0xc3, 0xa4))).toBe("Bä");
  expect(await statementText(bytes(0x42, 0xc3, 0xa4))).toBe("Bä");
  // UTF-16, as Excel saves "Unicode text", with a mark or without one.
  expect(await statementText(bytes(0xff, 0xfe, 0x44, 0x00, 0xe4, 0x00))).toBe("Dä");
  expect(await statementText(bytes(0xfe, 0xff, 0x00, 0x44, 0x00, 0xe4))).toBe("Dä");
  expect(await statementText(bytes(0x44, 0x00, 0x61, 0x00, 0x74, 0x00, 0x65, 0x00))).toBe("Date");
  expect(api.decodeStatement).toHaveBeenCalledTimes(2);
  // A workbook's other sheet, by its name, and the sheets it has.
  vi.mocked(api.decodeStatement).mockResolvedValueOnce({ text: "Fecha,Importe", encoding: "spreadsheet", sheet: "Resumen", sheets: ["Resumen", "Movimientos"] });
  expect(await statementFile(bytes(0x50, 0x4b, 0x03, 0x04), "Resumen")).toEqual({ text: "Fecha,Importe", sheet: "Resumen", sheets: ["Resumen", "Movimientos"] });
  expect(api.decodeStatement).toHaveBeenLastCalledWith("PK\u0003\u0004", "Resumen");
});

it("shows the name a payee rule gives a row beside the bank's, and what a refund may be of", () => {
  const b = demoBudget();
  const row = { date: "2026-10-12", memo: "", duplicate: false, candidates: [] };
  const rows = [
    { ...row, id: "a", payee: "SQ *BLUE BOTTLE 0042", rename: "Blue Bottle", categoryId: "x", amount: -650 },
    { ...row, id: "b", payee: "AMAZON EU", amount: 2399, refundOf: { id: "p1", date: "2026-10-07", payee: "Amazon EU", amount: -2399, categoryId: "groceries", purchases: 1 } },
    { ...row, id: "c", payee: "ZALANDO", amount: 1000, refundOf: { id: "p2", date: "2026-10-01", payee: "Zalando", amount: -5000, purchases: 2 } },
  ];
  render(<ImportReview budget={b} rows={rows} busy={false} icon={null} title="File" onCancel={() => {}} onImport={() => {}} />);
  expect(screen.getByText(/1 renamed by your payee rules · 2 refunds/)).toBeTruthy();
  const cell = screen.getByTitle("Blue Bottle, from the bank's “SQ *BLUE BOTTLE 0042”");
  expect(cell.textContent).toBe("Blue BottleSQ *BLUE BOTTLE 0042");
  const groceries = b.categories.find((c) => c.id === "groceries")!.name;
  expect(screen.getByText(new RegExp(`Refund of Amazon EU · .*23[.,]99.* on .* · ${groceries}`))).toBeTruthy();
  expect(screen.getByText("Possible refund · 2 purchases from Zalando match")).toBeTruthy();
});

it("words the request to the assistant, with the problem when there is one", () => {
  expect(importHelpText("Everyday")).toBe(
    "Help me import this file into Everyday. Find the transactions, ask me about anything unclear, and prepare them for the import review.",
  );
  expect(importHelpText("Card", "The file has no transactions to import.")).toContain(
    "into Card. The import preview said: The file has no transactions to import. Find",
  );
});

const imported = (id: string, accountId: string, categoryId: string | null, amount = -1000): Transaction => ({
  id,
  date: "2026-10-16",
  accountId,
  payee: "Shop",
  memo: "",
  amount,
  status: "cleared",
  splits: [{ categoryId, amount }],
  transferAccountId: null,
  transferStatus: null,
  approved: false,
});
/** The demo budget after an import added these transactions, with the Uncategorized placeholder it creates. */
const after = (b: Budget, ...added: Transaction[]): Budget => ({
  ...b,
  categories: [...b.categories, { id: "local-uncategorized", name: "Uncategorized", group: "Needs review", target: 0 }],
  transactions: [...b.transactions, ...added],
});

it("finds the new transactions an import left without a category", () => {
  const b = { ...demoBudget(), accounts: [...demoBudget().accounts, { id: "car", name: "Car", kind: "tracking" as const, openingBalance: 0, archived: false }] };
  const next = after(
    b,
    imported("bank:a", "checking", "local-uncategorized"),
    imported("bank:b", "checking", null, 250000),
    // A payee rule categorized this one.
    imported("bank:c", "checking", "groceries"),
    imported("bank:d", "credit", "local-uncategorized"),
    // Tracking accounts don't use categories.
    imported("bank:e", "car", null),
  );
  expect(uncategorizedImports(b, next)).toEqual({ ids: ["bank:a", "bank:b", "bank:d"], accounts: ["Everyday account", "Credit card"] });
  // Transactions that were already there (a match) don't count, even without a category.
  expect(uncategorizedImports(next, next)).toEqual({ ids: [], accounts: [] });
});

it("words the request with the count and the accounts", () => {
  expect(categorizeText({ ids: ["a"], accounts: ["Checking"] })).toBe("Categorize the new transaction in Checking.");
  expect(categorizeText({ ids: Array.from({ length: 12 }, (_, i) => `t${i}`), accounts: ["Checking"] })).toBe(
    "Categorize the 12 new transactions in Checking.",
  );
  expect(categorizeText({ ids: ["a", "b"], accounts: ["Checking", "Savings"] })).toBe(
    "Categorize the 2 new transactions in Checking and Savings.",
  );
  expect(categorizeText({ ids: ["a", "b", "c"], accounts: ["A", "B", "C"] })).toBe("Categorize the 3 new transactions in A, B and C.");
  expect(categorizeText({ ids: Array.from({ length: 230 }, (_, i) => `t${i}`), accounts: ["Checking"] })).toBe(
    "Categorize these 200 of the 230 new transactions in Checking.",
  );
});

