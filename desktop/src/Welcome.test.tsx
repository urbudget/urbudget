import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("./bridge", async () => {
  const { templatesFixture } = await import("./budgetTemplates.fixture");
  return {
    api: {
      listBudgets: vi.fn(async () => []),
      createBudget: vi.fn(async () => "sample-id"),
      deleteBudget: vi.fn(async () => []),
      budgetTemplates: vi.fn(async () => templatesFixture()),
      importYnab: vi.fn(),
    },
  };
});
vi.mock("./cloud", async (original) => {
  const real = await original<typeof import("./cloud")>();
  return { ...real, cloud: { ...real.cloud, open: vi.fn(async () => {}) } };
});
import { api } from "./bridge";
import { cloud } from "./cloud";
import { FirstRunWelcome, SampleBanner, Welcome, isBlank, isUntouched, openSample } from "./Welcome";
import { localeDefaults } from "./locale";
import { readDraft, writeMark } from "./setup";
import { ToastProvider } from "./ui";
import type { Budget } from "./types";

const blank = (): Budget => ({
  schemaVersion: 2,
  name: "My budget",
  currency: "USD",
  numberFormat: { decimal: ".", group: ",", symbol: "before" },
  dateFormat: "mdy",
  accounts: [],
  categories: [],
  transactions: [],
  assignments: [],
});
/** An empty budget's page. */
const welcome = (props: Partial<Parameters<typeof Welcome>[0]> = {}) => (
  <ToastProvider>
    <Welcome
      budget={blank()}
      budgetId="b1"
      transform={vi.fn()}
      openBudget={vi.fn()}
      onSetup={vi.fn()}
      onAddAccount={vi.fn()}
      importYnab={vi.fn()}
      resolveYnab={vi.fn()}
      acceptYnab={vi.fn()}
      {...props}
    />
  </ToastProvider>
);
const firstRun = (props: Partial<Parameters<typeof FirstRunWelcome>[0]> = {}) => (
  <ToastProvider>
    <FirstRunWelcome openBudget={vi.fn(async () => {})} onSetup={vi.fn()} sync={<p>Account and Sync panels</p>} {...props} />
  </ToastProvider>
);
beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  vi.clearAllMocks();
  vi.mocked(api.listBudgets).mockResolvedValue([]);
});
afterEach(cleanup);

it("knows a first launch's budget from one someone set up", () => {
  const { numberFormat: _n, dateFormat: _d, ...fresh } = { ...blank(), currency: "SAR" };
  expect(isUntouched(fresh)).toBe(true);
  expect(isUntouched(blank())).toBe(false);
  expect(isBlank(blank())).toBe(true);
  expect(isBlank({ ...blank(), categories: [{ id: "c", name: "Rent", group: "Bills", target: 0 }] })).toBe(false);
});

it("gives an empty budget one button into the walkthrough, with the sample, YNAB and the guide as quiet links", () => {
  const onSetup = vi.fn();
  const view = render(welcome({ onSetup }));
  // The one filled button.
  expect(document.querySelectorAll("button.primary")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Set up this budget" }));
  expect(onSetup).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "Look around a sample" }).className).toBe("link-button");
  expect(screen.getByRole("button", { name: "I'm coming from YNAB" }).className).toBe("link-button");
  fireEvent.click(screen.getByRole("button", { name: "How this works" }));
  expect(cloud.open).toHaveBeenCalledWith("/getting-started/");
  // A budget the walkthrough made by skipping is offered the rest of it.
  writeMark("b1", { card: true });
  view.unmount();
  render(welcome());
  expect(screen.getByRole("button", { name: "Continue setup" })).toBeTruthy();
});

it("gives a budget with categories and no accounts the same page, without YNAB's import", () => {
  // New budget's template made it; nothing can be done in it until it has an account.
  const onSetup = vi.fn(), onAddAccount = vi.fn();
  const template = { ...blank(), categories: [{ id: "c", name: "Rent", group: "Bills", target: 0 }] };
  render(welcome({ budget: template, onSetup, onAddAccount }));
  expect(document.querySelectorAll("button.primary")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Set up this budget" }));
  expect(onSetup).toHaveBeenCalledOnce();
  // By hand is a quiet way around it.
  const byHand = screen.getByRole("button", { name: "Add an account myself" });
  expect(byHand.className).toBe("link-button");
  fireEvent.click(byHand);
  expect(onAddAccount).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "Look around a sample" })).toBeTruthy();
  // YNAB's import fills a whole budget: it's offered only to an empty one.
  expect(screen.queryByRole("button", { name: "I'm coming from YNAB" })).toBeNull();
});

it("says Continue setup only where setup is still on offer", () => {
  writeMark("b1", { card: false });
  render(welcome());
  expect(screen.getByRole("button", { name: "Set up this budget" })).toBeTruthy();
});

it("opens the sample as its own budget in this budget's currency", async () => {
  const openBudget = vi.fn(async () => {});
  render(welcome({ budget: { ...blank(), currency: "CAD" }, openBudget }));
  fireEvent.click(screen.getByRole("button", { name: "Look around a sample" }));
  await vi.waitFor(() => expect(openBudget).toHaveBeenCalledWith("sample-id"));
  const sample = vi.mocked(api.createBudget).mock.calls[0][0];
  expect(sample).toMatchObject({ name: "Sample budget", currency: "CAD", sample: { from: "b1" } });
  expect(sample.accounts.length).toBeGreaterThan(0);
});

it("leaves the sample for the budget it came from, deleting it if asked", async () => {
  vi.mocked(api.listBudgets).mockResolvedValue([
    { id: "other", name: "Other", currency: "USD", current: false, missing: false, path: "", lastOpened: null },
    { id: "b1", name: "My budget", currency: "USD", current: false, missing: false, path: "", lastOpened: null },
    { id: "s", name: "Sample budget", currency: "USD", current: true, missing: false, path: "", lastOpened: null },
  ] as never);
  const openBudget = vi.fn(async () => {});
  render(
    <ToastProvider>
      <SampleBanner budget={{ ...blank(), sample: { from: "b1" } }} budgetId="s" openBudget={openBudget} onNewBudget={vi.fn()} />
    </ToastProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete sample" }));
  await vi.waitFor(() => expect(api.deleteBudget).toHaveBeenCalledWith("s"));
  expect(openBudget).toHaveBeenCalledWith("b1");
});

it("leads the YNAB choice with the token import, with the export file a click away", () => {
  render(welcome());
  fireEvent.click(screen.getByRole("button", { name: /I'm coming from YNAB/ }));
  expect(screen.getByLabelText("YNAB personal access token")).toBeTruthy();
  // With the import open, the walkthrough steps back to a link: its button isn't a second filled one.
  expect(screen.queryByRole("button", { name: "Set up this budget" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "No token? Use YNAB's export file" }));
  expect(screen.getByRole("button", { name: /Choose export file/ })).toBeTruthy();
  expect(screen.queryByLabelText("YNAB personal access token")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Set it up step by step instead" }));
  expect(screen.getByRole("button", { name: "Set up this budget" })).toBeTruthy();
});

it("creates a new sample when the previous sample is in Trash or recovery", async () => {
  vi.mocked(api.listBudgets).mockResolvedValue([
    { id: "trashed-sample", name: "Sample budget", missing: false, lifecycle: "trashed" },
    { id: "recovered-sample", name: "Sample budget", missing: false, lifecycle: "recovery" },
  ] as never);
  const open = vi.fn(async () => {});
  await openSample(blank(), "mine", open);
  expect(api.createBudget).toHaveBeenCalledOnce();
  expect(open).toHaveBeenCalledWith("sample-id");
});

it("leaves a sample for an active budget when its original budget is in Trash", async () => {
  vi.mocked(api.listBudgets).mockResolvedValue([
    { id: "old", name: "Old budget", missing: false, lifecycle: "trashed" },
    { id: "mine", name: "My budget", missing: false, lifecycle: "active" },
    { id: "sample", name: "Sample budget", missing: false, lifecycle: "active" },
  ] as never);
  const open = vi.fn(async () => {});
  render(<ToastProvider><SampleBanner budget={{...blank(), sample: {from: "old"}}} budgetId="sample" openBudget={open} onNewBudget={vi.fn()} /></ToastProvider>);
  fireEvent.click(screen.getByRole("button", {name: "Go to my budget"}));
  await vi.waitFor(() => expect(open).toHaveBeenCalledWith("mine"));
});

it("opens a first launch on one button, the device's currency and date format, and three quiet links", () => {
  const onSetup = vi.fn();
  vi.spyOn(navigator, "languages", "get").mockReturnValue(["en-US"]);
  render(firstRun({ onSetup }));
  expect(document.querySelectorAll("button.primary")).toHaveLength(1);
  // Under the app's mark, one line says what the button starts; the walkthrough teaches the method.
  expect(document.querySelector(".welcome-first > svg.welcome-mark")).not.toBeNull();
  expect(screen.getByText("Five short steps to a budget where all your money has a job. Skip any time.")).toBeTruthy();
  expect(screen.queryByText(/works like envelopes/)).toBeNull();
  // No name, currency or format questions: the guess is one line, with a way to change it.
  expect(screen.getByText("US Dollar")).toBeTruthy();
  expect(screen.getByText("10/15/2026")).toBeTruthy();
  expect(screen.queryByText("Currency")).toBeNull();
  expect(screen.queryByText("Account and Sync panels")).toBeNull();
  expect(screen.getByRole("button", { name: "Look around a sample" }).className).toBe("link-button");
  expect(screen.getByRole("button", { name: "I already have a budget" }).className).toBe("link-button");
  // The method is a link away, on urbudget.app, for anyone who wants it before starting.
  fireEvent.click(screen.getByRole("button", { name: "How this works" }));
  expect(cloud.open).toHaveBeenCalledWith("/getting-started/");

  fireEvent.click(screen.getByRole("button", { name: "Create my budget" }));
  // The walkthrough opens on a draft; nothing is in the library.
  expect(onSetup).toHaveBeenCalledOnce();
  expect(api.createBudget).not.toHaveBeenCalled();
  expect(readDraft()).toMatchObject({ name: "My budget", step: 1, format: localeDefaults(["en-US"]) });
});

it("starts the walkthrough in the currency and formats changed on the first screen", () => {
  vi.spyOn(navigator, "languages", "get").mockReturnValue(["en-US"]);
  render(firstRun());
  fireEvent.click(screen.getByRole("button", { name: "Change" }));
  fireEvent.change(screen.getByLabelText("Date format"), { target: { value: "dmy" } });
  expect(screen.getByText("15/10/2026", { selector: "p span" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Create my budget" }));
  expect(readDraft()?.format).toMatchObject({ currency: "USD", dateFormat: "dmy" });
});

it("opens the sample from a first launch without making a budget of the person's own", async () => {
  vi.spyOn(navigator, "languages", "get").mockReturnValue(["en-GB"]);
  const openBudget = vi.fn(async () => {});
  render(firstRun({ openBudget }));
  fireEvent.click(screen.getByRole("button", { name: "Look around a sample" }));
  await vi.waitFor(() => expect(openBudget).toHaveBeenCalledWith("sample-id"));
  expect(api.createBudget).toHaveBeenCalledOnce();
  expect(vi.mocked(api.createBudget).mock.calls[0][0]).toMatchObject({ name: "Sample budget", currency: "GBP", dateFormat: "dmy", sample: { from: "" } });
  expect(readDraft()).toBeNull();
});

it("keeps YNAB, a backup file and signing in behind I already have a budget", async () => {
  const openBudget = vi.fn(async () => {});
  render(firstRun({ openBudget }));
  fireEvent.click(screen.getByRole("button", { name: "I already have a budget" }));
  expect(screen.getByRole("heading", { name: "I already have a budget" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Create my budget" })).toBeNull();
  for (const name of [/Bring it over from YNAB/, /Use YNAB's export file/, /Restore a backup file/, /Sign in and sync/])
    expect(screen.getByRole("button", { name })).toBeTruthy();
  // Account and Sync left the first screen: they're here, behind their row.
  expect(screen.queryByText("Account and Sync panels")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /Sign in and sync/ }));
  expect(screen.getByText("Account and Sync panels")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /Use YNAB's export file/ }));
  expect(screen.getByRole("button", { name: /Choose export file/ })).toBeTruthy();
  expect(screen.queryByText("Account and Sync panels")).toBeNull();
  expect(api.createBudget).not.toHaveBeenCalled();

  // A backup file becomes a budget and opens.
  const backup = { ...blank(), name: "Restored" };
  const file = new File([JSON.stringify(backup)], "backup.json", { type: "application/json" });
  fireEvent.change(screen.getByLabelText("Restore a backup file"), { target: { files: [file] } });
  await vi.waitFor(() => expect(openBudget).toHaveBeenCalledWith("sample-id"));
  expect(vi.mocked(api.createBudget).mock.calls[0][0]).toMatchObject({ name: "Restored" });

  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(screen.getByRole("button", { name: "Create my budget" })).toBeTruthy();
});

it("creates an explicit YNAB destination before showing its token form so import review survives", async () => {
  const openBudget = vi.fn(async () => {});
  const { unmount } = render(firstRun({ openBudget }));
  fireEvent.click(screen.getByRole("button", { name: "I already have a budget" }));
  fireEvent.click(screen.getByRole("button", { name: /Bring it over from YNAB/ }));
  await vi.waitFor(() => expect(openBudget).toHaveBeenCalledWith("sample-id"));
  expect(vi.mocked(api.createBudget).mock.calls[0][0]).toMatchObject({ name: "My budget", accounts: [], categories: [] });
  unmount();
  render(welcome({ budgetId: "sample-id", openBudget }));
  expect(screen.getByLabelText("YNAB personal access token")).toBeTruthy();
  expect(sessionStorage.getItem("lb:welcome-ynab:sample-id")).toBeNull();
});
