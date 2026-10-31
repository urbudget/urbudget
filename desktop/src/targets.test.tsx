import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TargetFields, updateTarget } from "./TargetFields";
import { categoryState } from "./CategoryRow";
import type { Category, Summary } from "./types";

afterEach(cleanup);

describe("target transitions and monthly progress", () => {
  it.each([200_000, 2_000_000])("does not require replenishing %s moved out of a category with no target", (amount) => {
    const c: Category = { id: "c", name: "Cash Reserve", group: "Assets & Goals", target: 0 };
    const row = { id: "c", assigned: -amount, available: 0, carried: amount, activity: 0 };
    // Older summaries can omit needed; both paths must treat a zero target as absent.
    for (const needed of [undefined, 0]) {
      expect(categoryState(c, { ...row, needed }, "2026-10"))
        .toMatchObject({ status: "zero", needed: 0, progress: null });
      expect(categoryState(c, { ...row, needed, available: 1000 }, "2026-10"))
        .toMatchObject({ status: "positive", needed: 0 });
      expect(categoryState(c, { ...row, needed, upcoming: 5000 }, "2026-10"))
        .toMatchObject({ status: "underfunded", needed: 5000 });
      expect(categoryState(c, { ...row, needed, available: -5000 }, "2026-10"))
        .toMatchObject({ status: "negative", needed: 5000 });
    }
  });

  it("removes a hidden yearly anchor when selecting weekly", () => {
    const onChange = vi.fn();
    render(<TargetFields goal={{ goal_type: "NEED", goal_cadence: 13, goal_target_date: "2026-12-31", goal_day: 31 }} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("Target cadence"), { target: { value: "2" } });
    const next = onChange.mock.calls[0][0];
    expect(next.goal_cadence).toBe(2);
    expect(next).not.toHaveProperty("goal_target_date");
    expect(next).not.toHaveProperty("goal_day");
  });
  it("clears weekly timing before a dated balance goal and when changing weekday", () => {
    const weekly = { goal_type: "NEED", goal_cadence: 2, goal_target_date: "2026-10-01", goal_day: 4, goal_cadence_frequency: 2 };
    const dated = updateTarget(weekly, { goal_type: "TBD" });
    expect(dated.goal_cadence).toBe(0);
    expect(dated).not.toHaveProperty("goal_target_date");
    expect(updateTarget(weekly, { goal_day: 0 })).not.toHaveProperty("goal_target_date");
  });
  it("drops a hidden first due date when a target becomes plain monthly", () => {
    const bimonthly = { goal_type: "NEED", goal_cadence: 3, goal_target_date: "2026-12-15" };
    expect(updateTarget(bimonthly, { goal_cadence: 1 })).not.toHaveProperty("goal_target_date");
    const everyOther = { goal_type: "NEED", goal_cadence: 1, goal_cadence_frequency: 2, goal_target_month: "2026-12-01" };
    expect(updateTarget(everyOther, { goal_cadence_frequency: 1 })).not.toHaveProperty("goal_target_month");
    // An imported monthly target keeps its first due date through other edits.
    const imported = { goal_type: "NEED", goal_cadence: 1, goal_cadence_frequency: 1, goal_target_month: "2025-03-01" };
    expect(updateTarget(imported, { goal_needs_whole_amount: false })).toHaveProperty("goal_target_month", "2025-03-01");
  });
  it("keeps the same period when only switching refill behavior", () => {
    const goal = { goal_type: "NEED", goal_cadence: 13, goal_target_date: "2027-01-31", goal_creation_month: "2026-10-01", source: "retained" };
    expect(updateTarget(goal, { goal_type: "NEED", goal_needs_whole_amount: false })).toEqual({ ...goal, goal_needs_whole_amount: false });
  });
  it("shows monthly funding for weekly and annual goals on both shared row layouts", () => {
    const c: Category = { id: "c", name: "Goal", group: "Goals", target: 10000 };
    const row: Summary["categories"][number] = { id: "c", assigned: 10000, available: 10000, carried: 0, activity: 0, needed: 30000, targetFundingRequired: 40000, targetFundingFunded: 10000 };
    expect(categoryState(c, row, "2026-10").progress).toBe(25);
    expect(categoryState({ ...c, target: 120000 }, { ...row, assigned: 0, needed: 10000, targetFundingRequired: 10000, targetFundingFunded: 0 }, "2026-10").progress).toBe(0);
  });

  it("keeps an undated balance target's progress and needs its shortfall only eventually", () => {
    const c: Category = { id: "c", name: "Savings", group: "Goals", target: 10000, goal: { goal_type: "TB" } };
    const state = categoryState(c, { id: "c", assigned: 1000, available: 2500, carried: 1500, activity: 0, needed: 0, targetFundingRequired: 10000, targetFundingFunded: 2500 }, "2026-10");
    expect(state).toMatchObject({ status: "positive", progress: 25, needed: 0, eventually: 7500 });
  });

  it.each(["NEED", "TB"])("keeps scheduled commitments and overspending visible when a %s target is snoozed", (type) => {
    const c: Category = { id: "c", name: "Goal", group: "Goals", target: 10000, goal: { goal_type: type, goal_snoozed_at: "2026-10-01" } };
    const row = { id: "c", assigned: 2500, available: 2500, carried: 0, activity: 0, needed: 3500, upcoming: 6000 };
    expect(categoryState(c, row, "2026-10")).toMatchObject({ status: "underfunded", needed: 3500, eventually: 0, snoozed: true });
    expect(categoryState(c, { ...row, available: -500, needed: 6500 }, "2026-10")).toMatchObject({ status: "negative", needed: 6500, eventually: 0 });
  });

  it("needs only a balance target's commitments this month and the rest of it eventually", () => {
    const c: Category = { id: "c", name: "Savings", group: "Goals", target: 10000, goal: { goal_type: "TB" } };
    expect(categoryState(c, { id: "c", assigned: 0, available: 2500, carried: 2500, activity: 0, needed: 3500, upcoming: 6000 }, "2026-10"))
      .toMatchObject({ status: "underfunded", needed: 3500, eventually: 7500 });
    // Older summaries without funding fields, or one still counting the balance's shortfall.
    expect(categoryState(c, { id: "c", assigned: 0, available: -500, carried: 0, activity: -500, needed: 10500 }, "2026-10"))
      .toMatchObject({ status: "negative", needed: 500, eventually: 10000 });
  });
});

describe("target snooze controls", () => {
  it.each(["NEED", "TB", "TBD", "MF", "DEBT"])("snoozes a nonpayment %s target only for the current month", (type) => {
    const onChange = vi.fn();
    const goal = { goal_type: type, goal_snoozed_at: "2026-09-01", imported: "retained" };
    const view = render(<TargetFields goal={goal} onChange={onChange} month="2026-10" currentMonth="2026-10" />);
    const control = screen.getByRole("checkbox", { name: "Snooze target this month" });
    expect((control as HTMLInputElement).checked).toBe(false);
    fireEvent.click(control);
    expect(onChange).toHaveBeenLastCalledWith({ ...goal, goal_snoozed_at: "2026-10-01" });
    view.rerender(<TargetFields goal={{ ...goal, goal_snoozed_at: "2026-10-01" }} onChange={onChange} month="2026-10" currentMonth="2026-10" />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Snooze target this month" }));
    expect(onChange).toHaveBeenLastCalledWith({ ...goal, goal_snoozed_at: null });
  });

  it.each(["2026-09", "2026-11"])("preserves imported snooze metadata while editing a target in %s", (month) => {
    const onChange = vi.fn();
    const goal = { goal_type: "NEED", goal_snoozed_at: "2026-09-01", imported: "retained" };
    render(<TargetFields goal={goal} onChange={onChange} month={month} currentMonth="2026-10" />);
    expect(screen.queryByLabelText(/Snooze target/)).toBeNull();
    fireEvent.change(screen.getByLabelText("Target behavior"), { target: { value: "refill" } });
    expect(onChange.mock.calls[0][0]).toMatchObject({ goal_snoozed_at: "2026-09-01", imported: "retained" });
  });

  it("does not offer snooze for a credit-card payment category", () => {
    render(<TargetFields goal={{ goal_type: "MF", goal_snoozed_at: "2026-09-01" }} onChange={vi.fn()} month="2026-10" currentMonth="2026-10" paymentCategory />);
    expect(screen.queryByLabelText(/Snooze target/)).toBeNull();
  });
});
