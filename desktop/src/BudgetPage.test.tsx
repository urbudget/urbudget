import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { produce } from "immer";
import { afterEach, expect, it, vi } from "vitest";

vi.mock("./bridge", () => ({
  api: {
    action: vi.fn(async (b: unknown) => b),
    reports: vi.fn(async () => ({ ageOfMoney: 23 })),
  },
}));
import { BudgetPage } from "./BudgetPage";
import { categoryActivity } from "./ActivityPopover";
import { demoBudget } from "./demo";
import { ToastProvider } from "./ui";
import { today, type Budget, type Summary } from "./types";

afterEach(() => {
  cleanup();
});
const month = today().slice(0, 7);

function setup(initial: Budget = demoBudget(), more: Partial<Summary> = {}) {
  let budget = initial;
  const groceries = budget.categories.find((c) => c.id === "groceries")!;
  const summary: Summary = {
    readyToAssign: 1000,
    assigned: 0,
    activity: 0,
    available: 0,
    income: 0,
    netWorth: 0,
    accounts: [],
    categories: budget.categories.map((c) => ({
      id: c.id,
      assigned: 0,
      activity: c.id === groceries.id ? -47975 : 0,
      available: c.id === "rent" ? -500 : 0,
      carried: 0,
      needed: c.id === groceries.id ? 1200 : 0,
    })),
    ...more,
  };
  const edit = vi.fn((update: (d: Budget) => void) => {
    budget = produce(budget, update);
    view.rerender(page());
  });
  const onOpenRegister = vi.fn();
  const page = () => (
    <ToastProvider>
      <BudgetPage
        budget={budget}
        summary={summary}
        month={month}
        edit={edit}
        transform={vi.fn()}
        onEditorChange={vi.fn()}
        onEditCategory={vi.fn()}
        onOpenRegister={onOpenRegister}
      />
    </ToastProvider>
  );
  const view = render(page());
  return { edit, result: () => budget, groceries, onOpenRegister };
}

it("filters the plan to underfunded or overspent categories", () => {
  setup();
  const grid = screen.getByRole("grid", { name: "Monthly plan" });
  fireEvent.click(screen.getByRole("tab", { name: /Underfunded/ }));
  expect(within(grid).getAllByRole("row").map((r) => r.textContent).join("|")).toContain("Groceries");
  expect(within(grid).queryByText("Rent")).toBeNull();
  fireEvent.click(screen.getByRole("tab", { name: /Overspent/ }));
  expect(within(grid).getByText("Rent")).toBeTruthy();
  expect(within(grid).queryByText("Groceries")).toBeNull();
});

it("renames a category in place with F2 and Enter", () => {
  const { result, groceries } = setup();
  const name = screen.getAllByRole("button").find((b) => b.textContent?.startsWith("Groceries"))!;
  fireEvent.keyDown(name, { key: "F2" });
  const input = screen.getByRole("textbox", { name: "Rename Groceries" });
  fireEvent.change(input, { target: { value: "Food shop" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(result().categories.find((c) => c.id === groceries.id)!.name).toBe("Food shop");
});

it("selects categories with their checkboxes, Space on the name, and the header's box", () => {
  setup();
  const grid = screen.getByRole("grid", { name: "Monthly plan" });
  const box = (name: string) => within(grid).getByRole("checkbox", { name: `Select ${name}` }) as HTMLInputElement;
  const row = (name: string) => box(name).closest<HTMLElement>("[role=row]")!;
  // A checkbox adds to the selection, as Ctrl-click does.
  fireEvent.click(box("Groceries"));
  fireEvent.click(box("Coffee & snacks"));
  expect(row("Groceries").getAttribute("aria-selected")).toBe("true");
  expect(row("Coffee & snacks").getAttribute("aria-selected")).toBe("true");
  const all = box("all categories");
  expect(all.indeterminate).toBe(true);
  const coffee = within(row("Coffee & snacks")).getAllByRole("button").find((b) => b.textContent?.startsWith("Coffee"))!;
  fireEvent.keyDown(coffee, { key: " " });
  expect(row("Coffee & snacks").getAttribute("aria-selected")).toBe("false");
  expect(row("Groceries").getAttribute("aria-selected")).toBe("true");
  fireEvent.click(all);
  expect(all.checked).toBe(true);
  expect(within(grid).queryAllByRole("row", { selected: false })).toHaveLength(0);
  fireEvent.click(all);
  expect(within(grid).queryAllByRole("row", { selected: true })).toHaveLength(0);
});

it("says beside Assigned what a category is overspent by", () => {
  setup();
  const grid = screen.getByRole("grid", { name: "Monthly plan" });
  const rent = within(grid).getByRole("checkbox", { name: "Select Rent" }).closest<HTMLElement>("[role=row]")!;
  expect(rent.querySelector(".row-status")!.textContent).toBe("5.00 SAR overspent");
});

it("counts category activity like the budget: split lines and transfers out of the budget", () => {
  const b = demoBudget();
  b.accounts.push({ id: "asset", name: "Investment", kind: "tracking", onBudget: false, openingBalance: 0, archived: false });
  const date = `${month}-20`;
  b.transactions.push(
    {
      id: "split",
      date,
      accountId: "checking",
      payee: "Market",
      memo: "",
      amount: -9000,
      status: "cleared",
      splits: [
        { categoryId: "groceries", amount: -6000, memo: "Food" },
        { categoryId: "gifts", amount: -3000 },
      ],
      transferAccountId: null,
      transferStatus: null,
    },
    {
      // Recorded from the tracking side: the budget side received 50.00 into Groceries.
      id: "from-asset",
      date,
      accountId: "asset",
      payee: "",
      memo: "",
      amount: -5000,
      status: "cleared",
      splits: [{ categoryId: "groceries", amount: -5000 }],
      transferAccountId: "checking",
      transferStatus: "cleared",
    },
    {
      id: "between-budget-accounts",
      date,
      accountId: "checking",
      payee: "",
      memo: "",
      amount: -1000,
      status: "cleared",
      splits: [{ categoryId: "groceries", amount: -1000 }],
      transferAccountId: "savings",
      transferStatus: "cleared",
    },
  );
  const lines = categoryActivity(b, "groceries", month);
  const byKey = new Map(lines.map((l) => [l.key, l]));
  expect(byKey.get("split:0")).toMatchObject({ amount: -6000, memo: "Food", account: "Everyday account" });
  expect(byKey.get("from-asset:0")).toMatchObject({ amount: 5000, payee: "Transfer: Investment", account: "Everyday account" });
  expect(byKey.has("between-budget-accounts:0")).toBe(false);
  expect(byKey.has("split:1")).toBe(false);
  // The two demo grocery purchases are on the credit card, which is a budget account.
  expect(lines.filter((l) => l.key.startsWith("t")).map((l) => l.amount)).toEqual([-19225, -28750]);
});

it("shows a category's transactions from its Activity and opens them in the register", () => {
  const { onOpenRegister } = setup();
  fireEvent.click(screen.getByRole("button", { name: /activity in Groceries/ }));
  const popover = screen.getByRole("dialog", { name: "Groceries activity" });
  expect(within(popover).getAllByText("The neighborhood market")).toHaveLength(2);
  expect(within(popover).getByText("-479.75 SAR")).toBeTruthy();
  fireEvent.click(within(popover).getByRole("button", { name: /View in register/ }));
  expect(onOpenRegister).toHaveBeenCalledWith({
    accountId: "all",
    category: "groceries",
    from: `${month}-01`,
    to: expect.stringMatching(new RegExp(`^${month}-(28|29|30|31)$`)),
  });
});

it("shows the cash age", async () => {
  setup();
  expect(await screen.findByText("23 days")).toBeTruthy();
});

it("creates, changes and removes a target from the inspector", () => {
  const { result } = setup();
  const coffee = () => result().categories.find((c) => c.id === "coffee")!;
  fireEvent.click(screen.getAllByRole("button").find((b) => b.textContent?.startsWith("Coffee"))!);
  const inspector = screen.getByRole("complementary", { name: "Details" });
  const before = coffee().target;
  fireEvent.click(within(inspector).getByRole("button", { name: before ? "Edit target" : "Create target" }));
  fireEvent.change(within(inspector).getByLabelText("Amount"), { target: { value: "50*3" } });
  fireEvent.change(within(inspector).getByLabelText("Target behavior"), { target: { value: "refill" } });
  fireEvent.click(within(inspector).getByRole("button", { name: "Save target" }));
  expect(coffee().target).toBe(15000);
  expect(coffee().goal).toMatchObject({ goal_type: "NEED", goal_needs_whole_amount: false, goal_target: 150000 });
  expect(screen.getByText(/the target for Coffee/)).toBeTruthy();

  fireEvent.click(within(inspector).getByRole("button", { name: "Edit target" }));
  fireEvent.click(within(inspector).getByRole("button", { name: "Remove target" }));
  expect(coffee().target).toBe(0);
  expect(coffee().goal).toBeNull();
});

it("shows this month's spending with no category as YNAB's Uncategorized row, overspent", () => {
  const { onOpenRegister } = setup(demoBudget(), { uncategorized: -2500, uncategorizedCredit: 0 });
  const row = screen.getByText("Uncategorized").closest('[role="row"]') as HTMLElement;
  expect(within(row).getByText("Needs a category")).toBeTruthy();
  const available = within(row).getByRole("button", { name: /uncategorized this month\. Show transactions/ });
  expect(available.className).toBe("available negative");
  fireEvent.click(available);
  expect(onOpenRegister).toHaveBeenCalledWith({
    accountId: "all",
    uncategorized: true,
    from: `${month}-01`,
    to: expect.stringMatching(new RegExp(`^${month}-(28|29|30|31)$`)),
  });
  cleanup();
  // Spent on a card beyond its cash, it's card debt, as a category's credit overspending.
  setup(demoBudget(), { uncategorized: -2500, uncategorizedCredit: 2500 });
  expect(screen.getByRole("button", { name: /overspent on a credit card/ }).className).toBe("available credit");
  cleanup();
  setup();
  expect(screen.queryByText("Uncategorized")).toBeNull();
});

it.each([
  { credit: 0 },
  { credit: 2500 },
])("includes uncategorized spending once in monthly totals, credit=$credit", ({ credit }) => {
  const budget = demoBudget();
  setup(budget, {
    assigned: 10000, activity: -1000, available: 11000,
    uncategorized: -2500, uncategorizedCredit: credit,
    categories: budget.categories.map((c) => ({
      id: c.id,
      assigned: c.id === "groceries" ? 10000 : 0,
      activity: c.id === "groceries" ? -1000 : 0,
      available: c.id === "groceries" ? 11000 : 0,
      carried: c.id === "groceries" ? 2000 : 0,
      needed: 0,
    })),
  });
  const values = (element: Element) => [...element.querySelectorAll("dd")].map((e) => e.textContent);
  expect(values(document.querySelector(".summary-strip .stats")!).slice(0, 3))
    .toEqual(["100.00 SAR", "-35.00 SAR", "85.00 SAR"]);
  expect(document.querySelector(".rta strong")?.textContent).toBe("10.00 SAR");
  expect(values(screen.getByLabelText("How Available adds up")))
    .toEqual(["20.00 SAR", "+100.00 SAR", "−35.00 SAR", "85.00 SAR"]);
  const total = () => [...document.querySelectorAll(".budget-total [role=gridcell]")].map((e) => e.textContent);
  expect(total()).toEqual(["100.00 SAR", "-35.00 SAR", "85.00 SAR"]);
  // Totals describe the whole month even when the category list is filtered.
  fireEvent.click(screen.getByRole("tab", { name: /Money available/ }));
  expect(screen.queryByText("Uncategorized")).toBeNull();
  expect(total()).toEqual(["100.00 SAR", "-35.00 SAR", "85.00 SAR"]);
  fireEvent.click(screen.getByRole("checkbox", { name: "Select Groceries" }));
  const inspector = screen.getByRole("complementary", { name: "Details" });
  expect(within(inspector).getByText("-10.00 SAR")).toBeTruthy();
  expect(inspector.querySelector(".inspector-amount")?.textContent).toContain("110.00 SAR");
});

it("says how much spending needs a category, and opens it", () => {
  const b = demoBudget();
  const t = b.transactions[0];
  b.transactions.push(
    { ...t, id: "u1", date: t.date, splits: [{ categoryId: null, amount: t.amount }] },
    { ...t, id: "u2", date: t.date, splits: [{ categoryId: null, amount: t.amount }] },
  );
  const { onOpenRegister } = setup(b);
  const notice = screen.getByRole("status");
  expect(notice.querySelector("strong")!.textContent).toBe("2 transactions need a category");
  expect(notice.textContent).toContain("counts as overspending until they have one.");
  fireEvent.click(within(notice).getByRole("button", { name: "Review" }));
  expect(onOpenRegister).toHaveBeenCalledWith({ accountId: "all", uncategorized: true });
});
