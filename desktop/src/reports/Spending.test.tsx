import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { ByPayee, SpendingCalendar } from "./Spending";
import type { Scope } from "./shared";

afterEach(cleanup);

function scope(count = 2500): Scope {
  return {
    data: { payees: Object.fromEntries(Array.from({ length: count }, (_, i) => [`Payee ${i}`, { amount: 300000 - i, count: 4 }])) },
    show: vi.fn(),
  } as unknown as Scope;
}

it("keeps the payee report bounded while making payees beyond the old 500-row limit reachable", () => {
  const s = scope();
  render(<ByPayee scope={s} />);
  const table = screen.getByRole("table", { name: "Payee spending" });
  expect(table.getAttribute("aria-rowcount")).toBe("2501");
  expect(within(table).getAllByRole("row").length).toBeLessThan(50);
  expect(within(table).queryByRole("button", { name: "Payee 2000" })).toBeNull();
  fireEvent.scroll(screen.getByRole("region", { name: "Payee spending rows" }), { target: { scrollTop: 2000 * 36 } });
  fireEvent.click(within(table).getByRole("button", { name: "Payee 2000" }));
  expect(s.show).toHaveBeenCalledWith({ title: "Payee 2000", payee: "Payee 2000" });
  expect(within(table).getAllByRole("row").length).toBeLessThan(50);
});

it("starts a new payee search at its first result after scrolling, and handles no matches", () => {
  render(<ByPayee scope={scope()} />);
  fireEvent.scroll(screen.getByRole("region", { name: "Payee spending rows" }), { target: { scrollTop: 2000 * 36 } });
  const search = screen.getByRole("textbox", { name: "Find a payee" });
  fireEvent.change(search, { target: { value: "Payee 2499" } });
  expect(within(screen.getByRole("table", { name: "Payee spending" })).getByRole("button", { name: "Payee 2499" })).toBeTruthy();
  expect(screen.getByRole("region", { name: "Payee spending rows" }).scrollTop).toBe(0);
  fireEvent.change(search, { target: { value: "Missing merchant" } });
  expect(screen.getByText("No payees match.")).toBeTruthy();
  fireEvent.change(search, { target: { value: "" } });
  expect(within(screen.getByRole("table", { name: "Payee spending" })).getByRole("button", { name: "Payee 0" })).toBeTruthy();
});

it("says what the weekday averages count, and what below zero means only when a weekday is below it", () => {
  const calendar = (days: Record<string, number>) =>
    render(<SpendingCalendar scope={{ data: { days }, from: "2026-09", to: "2026-09", show: vi.fn() } as unknown as Scope} />);
  const below = /Below zero, more came back than was spent/;
  calendar({ "2026-09-08": 5000 });
  expect(screen.getByText(/Each bar is the average spent on that day of the week, after refunds/)).toBeTruthy();
  expect(screen.queryByText(below)).toBeNull();
  cleanup();
  // A Tuesday refund bigger than that Tuesday's spending.
  calendar({ "2026-09-08": -50000, "2026-09-09": 3000 });
  expect(screen.getByText(below)).toBeTruthy();
});
