import { afterEach, describe, expect, it } from "vitest";
import { activeFormat, configureFormat, editableAmount, errorText, evaluateAmount, formatDate, money } from "./format";
import { orderedGroups } from "./categories";
import { rangeFor } from "./ui/DatePickers";
import { today, type Category } from "./types";

afterEach(() => configureFormat({ currency: "SAR" }));

describe("amounts people type", () => {
  it("accepts plain amounts with grouping", () => {
    expect(evaluateAmount("1,234.56")).toBe(123456);
    expect(evaluateAmount("-20")).toBe(-2000);
    expect(evaluateAmount(".5")).toBe(50);
    expect(evaluateAmount("")).toBe(0);
  });
  it("does calculator math, rounding only for * and /", () => {
    expect(evaluateAmount("6500.00+20")).toBe(652000);
    expect(evaluateAmount("(10+5)*2")).toBe(3000);
    expect(evaluateAmount("100/3")).toBe(3333);
    expect(evaluateAmount("0.1+0.2")).toBe(30);
    expect(evaluateAmount("10 - -2.5")).toBe(1250);
  });
  it("uses the budget's decimal separator", () => {
    expect(evaluateAmount("1.234,5", ",")).toBe(123450);
    expect(evaluateAmount("12,5*2", ",")).toBe(2500);
  });
  it("writes an amount for editing so it reads back the same with either decimal mark", () => {
    expect(editableAmount(-123450)).toBe("-1234.50");
    configureFormat({ currency: "EUR", numberFormat: { decimal: ",", group: ".", symbol: "after" } });
    expect(editableAmount(-123450)).toBe("-1234,50");
    expect(evaluateAmount(editableAmount(100000))).toBe(100000);
  });
  it("rejects anything that is not an amount", () => {
    for (const bad of ["abc", "1e5", "0.001", "5/0", "(1+2", "1++", "*3", "1.2.3"])
      expect(() => evaluateAmount(bad), bad).toThrow();
  });
});

describe("display formats", () => {
  it("formats money for each number format and never shows -0.00", () => {
    expect(money(-123456)).toBe("-1,234.56 SAR");
    expect(money(-0.4)).toBe("0.00 SAR");
    configureFormat({ currency: "EUR", numberFormat: { decimal: ",", group: ".", symbol: "after" } });
    expect(money(123456789)).toBe("1.234.567,89 €");
    expect(money(123456789, { symbol: true })).toBe("1.234.567,89 €");
    configureFormat({ currency: "USD", numberFormat: { decimal: ".", group: " ", symbol: "before" } });
    expect(money(-150000)).toBe("-$1 500.00");
    configureFormat({ currency: "SAR", numberFormat: { symbol: "none" } });
    expect(money(100)).toBe("1.00");
    expect(money(100, { symbol: true })).toBe("1.00");
  });
  it("puts a currency sign before the amount and a currency code after it, unless the budget chooses", () => {
    configureFormat({ currency: "USD" });
    expect(money(-161095)).toBe("-$1,610.95");
    configureFormat({ currency: "SAR" });
    expect(money(161095)).toBe("1,610.95 SAR");
    configureFormat({ currency: "USD", numberFormat: { symbol: "after" } });
    expect(money(161095)).toBe("1,610.95 $");
    configureFormat({ currency: "SAR", numberFormat: { symbol: "before" } });
    expect(money(161095)).toBe("SAR 1,610.95");
    expect(money(-161095)).toBe("-SAR 1,610.95");
  });
  it.each([
    ["before", "$0.00", "-$1,234.56", "$1,234,567.89"],
    ["after", "0.00 $", "-1,234.56 $", "1,234,567.89 $"],
    ["none", "0.00", "-1,234.56", "1,234,567.89"],
  ] as const)("applies %s placement to zero, negative and large amounts", (symbol, zero, negative, large) => {
    configureFormat({ currency: "USD", numberFormat: { symbol } });
    expect(money(0)).toBe(zero);
    expect(money(-0.4)).toBe(zero);
    expect(money(-123456)).toBe(negative);
    expect(money(123456789)).toBe(large);
  });
  it("can omit currency explicitly while retaining the budget's separators", () => {
    configureFormat({ currency: "EUR", numberFormat: { decimal: ",", group: ".", symbol: "before" } });
    expect(money(-123456, { symbol: false })).toBe("-1.234,56");
    expect(money(123456, { currency: "USD", symbol: false })).toBe("1.234,56");
  });
  it("labels another source currency with the active separators and placement, and omits unknown currency", () => {
    configureFormat({ currency: "USD", numberFormat: { decimal: ",", group: ".", symbol: "after" } });
    expect(money(-123456, { currency: "EUR" })).toBe("-1.234,56 €");
    expect(money(123456, { currency: "SAR" })).toBe("1.234,56 SAR");
    expect(money(123456, { currency: null })).toBe("1.234,56");
    expect(money(123456, { currency: undefined })).toBe("1.234,56 $");
    configureFormat({ currency: "USD", numberFormat: { symbol: "none" } });
    expect(money(123456, { currency: "EUR" })).toBe("1,234.56");
  });
  it("previews independent currency and number settings without changing the active format", () => {
    configureFormat({ currency: "USD", dateFormat: "mdy" });
    const original = activeFormat();
    const preview = { currency: "EUR", numberFormat: { decimal: "," as const, group: "." as const, symbol: "after" as const } };
    expect(money(-123456, preview)).toBe("-1.234,56 €");
    expect(money(123456, { numberFormat: { group: "'" } })).toBe("$1'234.56");
    expect(activeFormat()).toBe(original);
    expect(activeFormat()).toEqual({
      currency: "USD", number: { decimal: ".", group: ",", symbol: "before" }, date: "mdy",
    });
    expect(money(123456)).toBe("$1,234.56");
    expect(preview).toEqual({ currency: "EUR", numberFormat: { decimal: ",", group: ".", symbol: "after" } });
  });
  it("formats dates for each date format", () => {
    expect(formatDate("2026-10-05")).toBe("2026-10-05");
    configureFormat({ currency: "SAR", dateFormat: "dmy" });
    expect(formatDate("2026-10-05")).toBe("05/10/2026");
    configureFormat({ currency: "SAR", dateFormat: "mdy" });
    expect(formatDate("2026-10-05")).toBe("10/05/2026");
    configureFormat({ currency: "SAR", dateFormat: "dmmmy" });
    expect(formatDate("2026-10-05")).toBe("5 Oct 2026");
  });
  it("strips internal prefixes from errors", () => {
    expect(errorText(new Error("CONFLICT: The database changed."))).toBe("The database changed.");
    expect(errorText("Error: Nope")).toBe("Nope");
  });
});


describe("category order", () => {
  const cat = (id: string, group: string, hidden = false): Category => ({ id, name: id, group, target: 0, hidden });
  it("puts categories and groups missing from the saved order last, and hidden ones in their own group", () => {
    const groups = orderedGroups({
      categories: [cat("a", "Bills"), cat("b", "Fun"), cat("c", "Bills"), cat("h", "Bills", true), cat("n", "New")],
      categoryOrder: ["c", "a"],
      groupOrder: ["Fun", "Bills"],
    });
    expect(groups.map((g) => g.name)).toEqual(["Fun", "Bills", "New", "Hidden categories"]);
    expect(groups[1].categories.map((c) => c.id)).toEqual(["c", "a"]);
    expect(groups[3].categories.map((c) => c.id)).toEqual(["h"]);
  });
});

describe("date range presets", () => {
  const today = new Date(2026, 9, 15);
  it("covers whole months relative to today", () => {
    expect(rangeFor("thisMonth", "day", today)).toMatchObject({ from: "2026-10-01", to: "2026-10-31" });
    expect(rangeFor("last3", "day", today)).toMatchObject({ from: "2026-08-01", to: "2026-10-31" });
    expect(rangeFor("lastYear", "month", today)).toMatchObject({ from: "2025-01", to: "2025-12" });
    expect(rangeFor("ytd", "month", today)).toMatchObject({ from: "2026-01", to: "2026-10" });
    expect(rangeFor("all", "day", today)).toMatchObject({ from: "", to: "" });
    expect(rangeFor("all", "month", today, "2023-11")).toMatchObject({ from: "2023-11", to: "2026-10" });
  });
});

import { parseDate, shiftDate } from "./format";
it("reads typed dates in the budget's format", () => {
  const base = "2026-10-15";
  expect(parseDate("5/10", base, "dmy")).toBe("2026-10-05");
  expect(parseDate("5/10", base, "mdy")).toBe("2026-05-10");
  expect(parseDate("05.10.27", base, "dmy")).toBe("2027-10-05");
  expect(parseDate("2026-1-9", base, "mdy")).toBe("2026-01-09");
  expect(parseDate("5 oct", base, "iso")).toBe("2026-10-05");
  expect(parseDate("Oct 5, 2025", base, "dmy")).toBe("2025-10-05");
  expect(parseDate("today", base)).toBe(today());
  expect(parseDate("31/2", base, "dmy")).toBeNull();
  expect(parseDate("hello", base)).toBeNull();
  expect(shiftDate("2026-01-31", 0, 1)).toBe("2026-02-28");
  expect(shiftDate("2026-12-31", 1)).toBe("2027-01-01");
});
