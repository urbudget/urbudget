// The language setting and what it does to the window: Arabic text, right to left, in place.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";

vi.mock("../bridge", () => ({
  desktop: false,
  api: { action: vi.fn(async (b: unknown) => b), reports: vi.fn(async () => ({ ageOfMoney: 23 })) },
}));
import { getLanguage, list, resolveLanguage, t, tn, useLanguage } from "./index";
import { getPref, setPref } from "../prefs";
import { dayName, deadline, formatDate, money, monthName, parseDate, evaluateAmount } from "../format";
import { SettingsPage } from "../SettingsPage";
import { BudgetPage } from "../BudgetPage";
import { ToastProvider } from "../ui";
import { demoBudget } from "../demo";
import { starterCategories } from "../newBudget";
import { today, type Summary } from "../types";
import type { Cloud } from "../cloud";
import type { Updates } from "../updates";

const LRI = "⁦", FSI = "⁨", PDI = "⁩";
const language = (value: "system" | "en" | "ar") => act(() => setPref("language", value));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  setPref("language", "system");
  localStorage.clear();
});

describe("System", () => {
  it("follows the system's first preferred language", () => {
    expect(resolveLanguage("system", ["ar"])).toBe("ar");
    expect(resolveLanguage("system", ["ar-SA", "en-US"])).toBe("ar");
    expect(resolveLanguage("system", ["AR-eg"])).toBe("ar");
    expect(resolveLanguage("system", ["en-US", "ar-SA"])).toBe("en");
    expect(resolveLanguage("system", ["fr-FR"])).toBe("en");
    expect(resolveLanguage("system", [])).toBe("en");
    // Arabic chosen, or English, whatever the system says.
    expect(resolveLanguage("ar", ["en-US"])).toBe("ar");
    expect(resolveLanguage("en", ["ar-SA"])).toBe("en");
  });

  it("reads navigator.languages, and follows it when it changes", () => {
    expect(getPref("language")).toBe("system");
    const languages = vi.spyOn(navigator, "languages", "get").mockReturnValue(["ar-SA", "en"]);
    act(() => void window.dispatchEvent(new Event("languagechange")));
    expect(getLanguage()).toBe("ar");
    expect(document.documentElement.dir).toBe("rtl");
    expect(document.documentElement.lang).toBe("ar");
    languages.mockReturnValue(["en-GB", "ar"]);
    act(() => void window.dispatchEvent(new Event("languagechange")));
    expect(getLanguage()).toBe("en");
    expect(document.documentElement.dir).toBe("ltr");
  });
});

describe("translation", () => {
  it("keeps English as written and fills placeholders", () => {
    expect(t("Save")).toBe("Save");
    expect(t("Not in the catalog {x}", { x: 1 })).toBe("Not in the catalog 1");
    expect(tn(1, "{count} transaction", "{count} transactions")).toBe("1 transaction");
    expect(tn(3, "{count} transaction", "{count} transactions")).toBe("3 transactions");
    expect(list(["a", "b"])).toBe("a, b");
    expect(money(-123456)).toBe("-1,234.56 SAR");
    expect(monthName("2026-10")).toBe("October 2026");
  });

  it("picks each of Arabic's plural forms", () => {
    language("ar");
    const count = (n: number) => tn(n, "{count} transaction", "{count} transactions");
    expect(count(0)).toBe("لا معاملات");
    expect(count(1)).toBe("معاملة واحدة");
    expect(count(2)).toBe("معاملتان");
    expect(count(3)).toBe(`${FSI}3${PDI} معاملات`);
    expect(count(11)).toBe(`${FSI}11${PDI} معاملة`);
    expect(count(100)).toBe(`${FSI}100${PDI} معاملة`);
  });

  it("keeps amounts, dates and names in their own direction inside Arabic", () => {
    language("ar");
    expect(money(-123456)).toBe(`${LRI}-1,234.56 SAR${PDI}`);
    expect(formatDate("2026-10-15")).toBe(`${LRI}2026-10-15${PDI}`);
    expect(t("Delete {budget}?", { budget: "Home" })).toContain(`${FSI}Home${PDI}`);
    // What the marks wrap still reads back as typed.
    expect(evaluateAmount(money(-123456, { symbol: false }))).toBe(-123456);
    expect(parseDate(formatDate("2026-10-15"), "2026-01-01")).toBe("2026-10-15");
    expect(list(["أ", "ب"])).toBe("أ، ب");
  });

  it("writes the month and day words the window shows in Arabic", () => {
    language("ar");
    expect(monthName("2026-10")).toBe("أكتوبر 2026");
    expect(monthName("2026-01", "short")).toBe("يناير 2026");
    expect(deadline("2026-12-01")).toBe("1 ديسمبر");
    expect(dayName(today())).toBe("اليوم");
    expect(dayName(`${Number(today().slice(0, 4)) - 1}-02-05`)).toMatch(/^[؀-ۿ]+، 5 فبراير \d{4}$/);
  });

  it("makes a new budget's starter categories in the language in use", () => {
    expect(starterCategories().groupOrder).toEqual(["Bills", "Needs", "Wants", "Savings"]);
    language("ar");
    const { categories, groupOrder } = starterCategories();
    expect(groupOrder).toEqual(["الفواتير", "الضروريات", "الكماليات", "المدخرات"]);
    expect(categories.map((c) => c.name)).toEqual(expect.arrayContaining(["البقالة", "المواصلات", "صندوق الطوارئ"]));
  });
});

/** Settings as the window shows it: App uses the language, so everything under it follows. */
function Window() {
  useLanguage();
  return (
    <ToastProvider>
      <SettingsPage
        budget={demoBudget()}
        sync="saved"
        syncLabel="Saved"
        metadata={null}
        edit={vi.fn()}
        onBankImport={vi.fn()}
        flush={vi.fn(async () => true)}
        onReload={vi.fn()}
        onExport={vi.fn()}
        onImportFile={vi.fn()}
        importYnab={vi.fn()}
        resolveYnab={vi.fn()}
        acceptYnab={vi.fn()}
        openBudget={vi.fn()}
        onBankSync={vi.fn()}
        theme="system"
        setTheme={vi.fn()}
        onResetLayout={vi.fn()}
        cloud={{} as Cloud}
        updates={{} as Updates}
        onInstallUpdate={vi.fn()}
        budgetId="language-test"
        isClean={() => true}
      />
    </ToastProvider>
  );
}
const settings = () => render(<Window />);

describe("Settings › General › Language", () => {
  it("offers System, English and العربية, names each language in itself, and remembers the choice", () => {
    settings();
    const select = screen.getByRole("combobox", { name: "Language" }) as HTMLSelectElement;
    expect([...select.options].map((o) => [o.value, o.textContent])).toEqual([
      ["system", "System"],
      ["en", "English"],
      ["ar", "العربية"],
    ]);
    expect(select.value).toBe("system");
    fireEvent.change(select, { target: { value: "ar" } });
    expect(getPref("language")).toBe("ar");
    expect(JSON.parse(localStorage.getItem("lb:prefs")!).language).toBe("ar");
    // Shown in Arabic, the language names stay as they are.
    const arabic = screen.getByRole("combobox", { name: "اللغة" }) as HTMLSelectElement;
    expect([...arabic.options].map((o) => o.textContent)).toEqual(["لغة النظام", "English", "العربية"]);
  });

  it("switches the open page to Arabic, right to left, without losing what's being typed", () => {
    settings();
    expect(document.documentElement.dir).toBe("ltr");
    const name = screen.getByRole("textbox", { name: "Budget name" }) as HTMLInputElement;
    fireEvent.change(name, { target: { value: "Half-typed name" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Language" }), { target: { value: "ar" } });

    expect(document.documentElement.dir).toBe("rtl");
    expect(document.documentElement.lang).toBe("ar");
    expect(screen.getByRole("tablist", { name: "أقسام الإعدادات" })).toBeTruthy();
    expect(screen.getByRole("tab", { name: "عام" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("heading", { name: "التطبيق" })).toBeTruthy();
    expect(screen.getByText("السمة")).toBeTruthy();
    // The same field, still holding the unsaved text: nothing was reloaded or remounted.
    expect(screen.getByRole("textbox", { name: "اسم الميزانية" })).toBe(name);
    expect(name.value).toBe("Half-typed name");

    fireEvent.change(screen.getByRole("combobox", { name: "اللغة" }), { target: { value: "en" } });
    expect(document.documentElement.dir).toBe("ltr");
    expect(screen.getByRole("heading", { name: "App" })).toBeTruthy();
    expect(name.value).toBe("Half-typed name");
  });
});

describe("the budget page in Arabic", () => {
  it("shows its chrome in Arabic and leaves the budget's own names alone", () => {
    language("ar");
    const budget = demoBudget();
    const summary: Summary = {
      readyToAssign: 100000,
      assigned: 0,
      activity: 0,
      available: 0,
      income: 0,
      netWorth: 0,
      accounts: [],
      categories: budget.categories.map((c) => ({ id: c.id, assigned: 0, activity: 0, available: 0, carried: 0, needed: 0 })),
    };
    render(
      <ToastProvider>
        <BudgetPage
          budget={budget}
          summary={summary}
          month={today().slice(0, 7)}
          edit={vi.fn()}
          transform={vi.fn()}
          onEditorChange={vi.fn()}
          onEditCategory={vi.fn()}
          onAddAccount={vi.fn()}
          onOpenSample={vi.fn()}
          onOpenRegister={vi.fn()}
        />
      </ToastProvider>,
    );
    expect(document.documentElement.dir).toBe("rtl");
    const grid = screen.getByRole("grid", { name: "الخطة الشهرية" });
    for (const heading of ["الفئة", "المخصَّص", "النشاط", "المتاح"]) expect(within(grid).getAllByText(heading).length).toBeGreaterThan(0);
    expect(screen.getByText("غير المخصَّص")).toBeTruthy();
    // People's data stays as they wrote it.
    expect(within(grid).getByText("Groceries")).toBeTruthy();
    expect(within(grid).getByText("Rent")).toBeTruthy();
    // No English chrome is left among the page's labels.
    const labels = [...document.querySelectorAll("[aria-label], [title], [placeholder]")].flatMap((el) =>
      ["aria-label", "title", "placeholder"].map((a) => el.getAttribute(a)).filter((v): v is string => !!v),
    );
    const names = new Set(budget.categories.flatMap((c) => [c.name, c.group]));
    const english = labels.filter((v) => /[A-Za-z]{3,}/.test(v.replace(/[⁦-⁩]/g, "")) && ![...names].some((n) => v.includes(n)));
    expect(english).toEqual([]);
  });
});
