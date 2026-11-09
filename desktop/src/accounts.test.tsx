import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
const api = vi.hoisted(() => ({ sourceLoanTerms: vi.fn() }));
vi.mock("./bridge", () => ({ api }));
import { accountType, creditLeft, inNetWorth, isLiability, isLoanType, kindForType, sectionOf } from "./accounts";
import { AccountForm } from "./forms";
import { diff } from "./patch";
import { TransactionsPage } from "./Register";
import { ToastProvider } from "./ui";
import type { Account, Budget } from "./types";

afterEach(cleanup);
const account = (over: Partial<Account>): Account => ({
  id: "a",
  name: "A",
  kind: "tracking",
  openingBalance: 0,
  archived: false,
  ...over,
});

it("works out an account's type from what's set, YNAB's import, or its kind", () => {
  expect(accountType(account({ accountType: "receivable" }))).toBe("receivable");
  expect(accountType(account({ source: { type: "mortgage" } }))).toBe("mortgage");
  expect(accountType(account({ source: { type: "otherLiability" } }))).toBe("otherDebt");
  expect(accountType(account({ kind: "credit" }))).toBe("creditCard");
  expect(accountType(account({ kind: "cash" }))).toBe("checking");
  expect(accountType(account({}), -5000)).toBe("otherDebt");
  expect(accountType(account({}), 5000)).toBe("otherAsset");
  expect(kindForType("savings")).toBe("cash");
  expect(kindForType("lineOfCredit")).toBe("credit");
  expect(kindForType("vehicle")).toBe("tracking");
  expect(isLiability("autoLoan")).toBe(true);
  expect(isLoanType("mortgage")).toBe(true);
  expect(isLoanType("lineOfCredit")).toBe(false);
  expect(isLoanType("receivable")).toBe(false);
  expect(sectionOf("studentLoan")).toBe("Loans");
  expect(inNetWorth(account({ netWorth: false }))).toBe(false);
  expect(inNetWorth(account({}))).toBe(true);
});

it("sets a tracking account's type and can leave it out of net worth", () => {
  const save = vi.fn();
  render(<AccountForm close={() => {}} save={save} />);
  fireEvent.change(screen.getByLabelText("Account name"), { target: { value: "Loan to Sam" } });
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: "receivable" } });
  fireEvent.click(screen.getByLabelText("Count this account in net worth"));
  fireEvent.change(screen.getByLabelText("Opening balance"), { target: { value: "500" } });
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  expect(save).toHaveBeenCalledWith(
    expect.objectContaining({
      name: "Loan to Sam",
      kind: "tracking",
      accountType: "receivable",
      netWorth: false,
      onBudget: false,
      openingBalance: 50000,
    }),
  );
});

it("budget accounts always count in net worth", () => {
  const save = vi.fn();
  render(<AccountForm close={() => {}} save={save} />);
  fireEvent.change(screen.getByLabelText("Account name"), { target: { value: "Savings" } });
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: "savings" } });
  expect(screen.queryByLabelText("Count this account in net worth")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  expect(save).toHaveBeenCalledWith(expect.objectContaining({ kind: "cash", accountType: "savings", netWorth: true }));
});

it.each([
  ["credit", "creditCard", "checking", "Checking"],
  ["credit", "creditCard", "otherDebt", "Other debt"],
  ["cash", "checking", "creditCard", "Credit card"],
  ["cash", "checking", "otherAsset", "Other asset"],
  ["tracking", "otherDebt", "checking", "Checking"],
  ["tracking", "otherDebt", "creditCard", "Credit card"],
] as const)("blocks an existing %s account's subtype %s from changing to %s", (kind, accountType, nextType, label) => {
  const save = vi.fn();
  render(<AccountForm initial={account({ kind, accountType })} close={() => {}} save={save} />);
  expect((screen.getByRole("option", { name: label }) as HTMLOptionElement).disabled).toBe(true);
  // A synthetic change can bypass a disabled option; submission still must not emit an invalid draft.
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: nextType } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save).not.toHaveBeenCalled();
  expect(screen.getByText("Create a new account to change how this account affects your budget.")).toBeTruthy();
});

it.each([
  ["cash", "checking", "savings"],
  ["credit", "creditCard", "lineOfCredit"],
  ["tracking", "autoLoan", "mortgage"],
] as const)("allows an existing %s account to change subtype from %s to %s", (kind, accountType, nextType) => {
  const save = vi.fn();
  render(<AccountForm initial={account({ kind, accountType })} close={() => {}} save={save} />);
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: nextType } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save).toHaveBeenCalledWith(expect.objectContaining({ kind, accountType: nextType }));
});

it("keeps an imported tracking account's accounting kind when its source describes a credit card", () => {
  const save = vi.fn();
  const initial = account({ source: { type: "creditCard" }, openingBalance: -5000 });
  render(<AccountForm initial={initial} close={() => {}} save={save} />);
  expect((screen.getByLabelText("Account type") as HTMLSelectElement).value).toBe("otherDebt");
  fireEvent.change(screen.getByLabelText("Account name"), { target: { value: "Tracked card" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save).toHaveBeenCalledWith(expect.objectContaining({ name: "Tracked card", kind: "tracking", onBudget: false }));
});

it.each([
  ["cash", "checking", "savings"],
  ["credit", "creditCard", "lineOfCredit"],
] as const)("preserves an existing %s account's explicit off-budget setting when renamed or changing subtype", (kind, accountType, nextType) => {
  const save = vi.fn();
  render(<AccountForm initial={account({ kind, accountType, onBudget: false })} close={() => {}} save={save} />);
  fireEvent.change(screen.getByLabelText("Account name"), { target: { value: "Renamed account" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ name: "Renamed account", kind, accountType, onBudget: false }));
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: nextType } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ kind, accountType: nextType, onBudget: false }));
});

const loanBudget = (over: Partial<Budget> = {}): Budget => ({
  schemaVersion: 2,
  name: "Test",
  currency: "SAR",
  accounts: [account({ id: "loan", name: "Car loan", accountType: "autoLoan", openingBalance: -1200000 })],
  categories: [{ id: "car", name: "Car payment", group: "Bills", target: 0 }],
  transactions: [],
  assignments: [],
  ...over,
});

it("adds a debt's loan details with the account, and only a debt's", () => {
  const save = vi.fn();
  render(<AccountForm budget={loanBudget()} close={() => {}} save={save} />);
  fireEvent.change(screen.getByLabelText("Account name"), { target: { value: "Mortgage" } });
  // Not for a budget account or an asset.
  expect(screen.queryByLabelText("Interest rate")).toBeNull();
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: "vehicle" } });
  expect(screen.queryByLabelText("Interest rate")).toBeNull();
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: "mortgage" } });
  fireEvent.change(screen.getByLabelText("Opening balance"), { target: { value: "-250000" } });
  fireEvent.change(screen.getByLabelText("Interest rate"), { target: { value: "5.25" } });
  fireEvent.change(screen.getByLabelText("Monthly payment"), { target: { value: "1,850" } });
  // A mortgage's fees are its escrow.
  fireEvent.change(screen.getByLabelText("Monthly escrow"), { target: { value: "320" } });
  fireEvent.change(screen.getByLabelText("New annual rate"), { target: { value: "6.125" } });
  fireEvent.change(screen.getByLabelText("Rate change month"), { target: { value: "2028-01" } });
  fireEvent.click(screen.getByRole("button", { name: "Add rate change" }));
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  const [saved, loan] = save.mock.calls[0];
  expect(saved).toMatchObject({ kind: "tracking", accountType: "mortgage", openingBalance: -25000000 });
  expect(loan).toEqual({
    accountId: saved.id,
    categoryId: "",
    annualRateBasisPoints: 525,
    monthlyPayment: 185000,
    monthlyFees: 32000,
    rateHistory: [{ month: "2028-01", annualRateMilliPercent: 6125 }],
  });
});

it("edits saved loan details, leaves them alone when untouched, and removes them when cleared", () => {
  const terms = { accountId: "loan", categoryId: "car", annualRateBasisPoints: 650, monthlyPayment: 100000, monthlyFees: 0 };
  const b = loanBudget({ loans: [terms] });
  const save = vi.fn();
  render(<AccountForm budget={b} initial={b.accounts[0]} focusLoan close={() => {}} save={save} />);
  // Opened from "Edit loan details", it starts at them.
  expect(document.activeElement).toBe(screen.getByLabelText("Interest rate"));
  expect((screen.getByLabelText("Monthly payment") as HTMLInputElement).value).toBe("1000.00");
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save.mock.calls[0]).toHaveLength(1);
  fireEvent.change(screen.getByLabelText("Monthly payment"), { target: { value: "1200" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save.mock.calls[1][1]).toMatchObject({ categoryId: "car", monthlyPayment: 120000 });
  fireEvent.change(screen.getByLabelText("Monthly payment"), { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(screen.getByText("Enter the loan's monthly payment, or clear its other details.")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Interest rate"), { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save.mock.calls[2][1]).toBeNull();
});

it("starts an imported loan from its YNAB terms", async () => {
  api.sourceLoanTerms.mockResolvedValue({
    accountId: "loan",
    categoryId: "car",
    annualRateBasisPoints: 499,
    monthlyPayment: 45000,
    monthlyFees: 0,
    rateHistory: [],
  });
  const b = loanBudget();
  b.accounts[0].source = { type: "autoLoan" };
  const save = vi.fn();
  render(<AccountForm budget={b} initial={b.accounts[0]} close={() => {}} save={save} />);
  // Offered, not filled in: only asking for them saves them.
  fireEvent.click(await screen.findByRole("button", { name: "Use them" }));
  expect((screen.getByLabelText("Interest rate") as HTMLInputElement).value).toBe("4.99");
  expect(screen.getByText("Filled in from your YNAB import. Check them, then save.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save.mock.calls[0][1]).toMatchObject({ annualRateBasisPoints: 499, monthlyPayment: 45000, categoryId: "car" });
});

it("reads an untyped tracking account as a debt by its balance now, as the register does", () => {
  const b = loanBudget({ accounts: [account({ id: "loan", name: "Loan", openingBalance: 0 })] });
  render(<AccountForm budget={b} initial={b.accounts[0]} balance={-500000} close={() => {}} save={vi.fn()} />);
  expect(screen.getByLabelText("Interest rate")).toBeTruthy();
});

it("never saves imported terms unasked, so cleared ones stay cleared", async () => {
  api.sourceLoanTerms.mockResolvedValue({ accountId: "loan", categoryId: "car", annualRateBasisPoints: 499, monthlyPayment: 45000, monthlyFees: 0, rateHistory: [] });
  // An imported loan whose details were cleared here: renaming it keeps them cleared.
  const b = loanBudget();
  b.accounts[0].source = { type: "autoLoan" };
  const save = vi.fn();
  render(<AccountForm budget={b} initial={b.accounts[0]} close={() => {}} save={save} />);
  await screen.findByRole("button", { name: "Use them" });
  expect((screen.getByLabelText("Monthly payment") as HTMLInputElement).value).toBe("");
  fireEvent.change(screen.getByLabelText("Account name"), { target: { value: "Car loan (Toyota)" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save.mock.calls[0][1]).toBeUndefined();
});

const card = (over: Partial<Account> = {}) =>
  account({ id: "visa", name: "Visa", kind: "credit", accountType: "creditCard", ...over });

it("works out a card's credit left from its limit", () => {
  // A card's balance is negative while money is owed.
  expect(creditLeft(card({ creditLimit: 1000000 }), -154500)).toBe(845500);
  expect(creditLeft(card({ creditLimit: 1000000 }), 2500)).toBe(1002500);
  // Past the limit, it's negative.
  expect(creditLeft(card({ creditLimit: 1000000 }), -1020000)).toBe(-20000);
  expect(creditLeft(card(), -154500)).toBeUndefined();
  expect(creditLeft(card({ creditLimit: 0 }), -154500)).toBeUndefined();
  // Only credit accounts have one.
  expect(creditLeft(account({ kind: "cash", accountType: "checking", creditLimit: 1000000 }), 0)).toBeUndefined();
});

it.each([
  ["creditCard", true],
  ["lineOfCredit", true],
  ["checking", false],
  ["cash", false],
  ["otherDebt", false],
  ["mortgage", false],
] as const)("shows the credit limit field for type %s: %s", (type, shown) => {
  render(<AccountForm close={() => {}} save={vi.fn()} />);
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: type } });
  expect(screen.queryByLabelText("Credit limit") !== null).toBe(shown);
});

it("saves a card's credit limit in cents, and leaves the key out when there's none", () => {
  const save = vi.fn();
  render(<AccountForm close={() => {}} save={save} />);
  fireEvent.change(screen.getByLabelText("Account name"), { target: { value: "Visa" } });
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: "creditCard" } });
  expect((screen.getByLabelText("Credit limit") as HTMLInputElement).value).toBe("");
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  expect(save.mock.calls[0][0]).not.toHaveProperty("creditLimit");
  fireEvent.change(screen.getByLabelText("Credit limit"), { target: { value: "10,000" } });
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  expect(save.mock.calls[1][0]).toMatchObject({ kind: "credit", accountType: "creditCard", creditLimit: 1000000 });
  // Typed in, then the type changed to a bank account: no limit.
  fireEvent.change(screen.getByLabelText("Account type"), { target: { value: "checking" } });
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  expect(save.mock.calls[2][0]).not.toHaveProperty("creditLimit");
});

it("edits a card's credit limit, and clearing it removes the key", () => {
  const save = vi.fn();
  render(<AccountForm initial={card({ creditLimit: 1000000 })} close={() => {}} save={save} />);
  expect((screen.getByLabelText("Credit limit") as HTMLInputElement).value).toBe("10000.00");
  fireEvent.change(screen.getByLabelText("Credit limit"), { target: { value: "12500.50" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save.mock.calls[0][0].creditLimit).toBe(1250050);
  fireEvent.change(screen.getByLabelText("Credit limit"), { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save.mock.calls[1][0]).not.toHaveProperty("creditLimit");
  fireEvent.change(screen.getByLabelText("Credit limit"), { target: { value: "0" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save.mock.calls[2][0]).not.toHaveProperty("creditLimit");
});

it("won't save a negative credit limit", () => {
  const save = vi.fn();
  render(<AccountForm initial={card({ accountType: "lineOfCredit" })} close={() => {}} save={save} />);
  fireEvent.change(screen.getByLabelText("Credit limit"), { target: { value: "-5000" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(save).not.toHaveBeenCalled();
  expect(screen.getByText("Enter the credit limit as a positive amount.")).toBeTruthy();
});

it("keeps a limit set on another device, and fields it doesn't know, when an account is edited", () => {
  const save = vi.fn();
  // Fields another app (or a later version) added to the record.
  const initial = { ...card({ creditLimit: 1000000 }), statementDay: 25, bank: { last4: "1234" } } as Account;
  render(<AccountForm initial={initial} close={() => {}} save={save} />);
  fireEvent.change(screen.getByLabelText("Account name"), { target: { value: "Visa Gold" } });
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  const saved = save.mock.calls[0][0];
  expect(saved).toMatchObject({ name: "Visa Gold", creditLimit: 1000000, statementDay: 25, bank: { last4: "1234" } });
  // The whole record goes to the engine, so they reach the saved budget.
  const before = loanBudget({ accounts: [initial] });
  const patch = diff(before, { ...before, accounts: [saved] });
  expect(patch?.accounts?.upsert[0]).toMatchObject({ creditLimit: 1000000, statementDay: 25, bank: { last4: "1234" } });
  cleanup();

  const plain = vi.fn();
  render(<AccountForm initial={{ ...account({ kind: "cash", accountType: "checking" }), statementDay: 1 } as Account} close={() => {}} save={plain} />);
  fireEvent.click(screen.getByRole("button", { name: "Update account" }));
  expect(plain.mock.calls[0][0]).toMatchObject({ statementDay: 1 });
});

function register(a: Account, balance: number) {
  render(
    <ToastProvider>
      <TransactionsPage
        budget={loanBudget({ accounts: [a] })}
        accountId={a.id}
        account={a}
        summary={{ id: a.id, balance, cleared: balance, uncleared: 0 }}
        netWorth={0}
        edit={vi.fn()}
        transform={vi.fn()}
        onEditAccount={vi.fn()}
        onReconcile={vi.fn()}
        onImport={vi.fn()}
        onSync={vi.fn()}
        onEdit={vi.fn()}
        onAddDialog={vi.fn()}
      />
    </ToastProvider>,
  );
}

it("shows a card's credit left beside its balance", () => {
  register(card({ creditLimit: 1000000 }), -154500);
  const left = screen.getByText("Credit left").nextElementSibling!;
  expect(left.textContent).toBe("8,455.00 SAR");
  expect(left.classList.contains("negative")).toBe(false);
  cleanup();

  // Past the limit: shown as it is, as a negative amount.
  register(card({ creditLimit: 1000000 }), -1020000);
  const over = screen.getByText("Credit left").nextElementSibling!;
  expect(over.textContent).toBe("-200.00 SAR");
  expect(over.classList.contains("negative")).toBe(true);
  cleanup();

  // Not without a limit.
  register(card(), -154500);
  expect(screen.queryByText("Credit left")).toBeNull();
});
