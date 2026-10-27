import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { SettingsPage } from "./SettingsPage";
import { ToastProvider } from "./ui";
import { demoBudget } from "./demo";
import { getPref } from "./prefs";
import type { Cloud } from "./cloud";
import type { Updates } from "./updates";

const api = vi.hoisted(() => ({ backup: vi.fn(), reveal: vi.fn(), backupRetention: vi.fn(), setBackupRetention: vi.fn() }));
vi.mock("./bridge", () => ({ desktop: true, api }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: async () => "1.0.3" }));

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  api.backup.mockResolvedValue("/private/backups/budget-20261019-100000-000000000.sqlite3");
  api.reveal.mockResolvedValue(undefined);
  api.backupRetention.mockResolvedValue(5);
  api.setBackupRetention.mockResolvedValue(undefined);
});
afterEach(cleanup);

function setup(flush = vi.fn(async () => true)) {
  const onImportFile = vi.fn();
  const view = render(<ToastProvider><SettingsPage
    budget={demoBudget()} sync="saved" syncLabel="Saved" metadata={null}
    edit={vi.fn()} onBankImport={vi.fn()} flush={flush} onReload={vi.fn()} onExport={vi.fn()}
    onImportFile={onImportFile} importYnab={vi.fn()} resolveYnab={vi.fn()} acceptYnab={vi.fn()} openBudget={vi.fn()}
    onBankSync={vi.fn()} theme="system" setTheme={vi.fn()} onResetLayout={vi.fn()}
    initialTab="backup" cloud={{} as Cloud} updates={{} as Updates} onInstallUpdate={vi.fn()}
    budgetId="backup-test" isClean={() => true}
  /></ToastProvider>);
  return { ...view, flush, onImportFile };
}

it("backs up after saving, and shows the backup in its folder on request", async () => {
  const path = "/private/backups/budget-20261019-100000-000000000.sqlite3";
  api.backup.mockResolvedValue(path);
  const { flush } = setup();
  fireEvent.click(screen.getByRole("button", { name: "Back up now" }));
  await waitFor(() => expect(api.backup).toHaveBeenCalled());
  expect(api.backup.mock.invocationCallOrder[0]).toBeGreaterThan(flush.mock.invocationCallOrder[0]);
  expect(api.reveal).not.toHaveBeenCalled();
  expect(screen.getByText(/A backup is a copy of this budget/)).toBeTruthy();
  expect(screen.getByText("Back up when the app closes")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Show in folder" }));
  await waitFor(() => expect(api.reveal).toHaveBeenCalledWith(path));
});

it("does not export an outdated backup when pending edits cannot be saved", async () => {
  setup(vi.fn(async () => false));
  fireEvent.click(screen.getByRole("button", { name: "Back up now" }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Save or resolve"));
  expect(api.backup).not.toHaveBeenCalled();
  expect(api.reveal).not.toHaveBeenCalled();
});

it("reads native retention after remounting and persists changes before reporting them", async () => {
  localStorage.setItem("lb:prefs", JSON.stringify({ keepBackups: 30 }));
  const { unmount } = setup();
  await waitFor(() => expect(screen.getByRole<HTMLSelectElement>("combobox", { name: "Backups to keep" }).value).toBe("5"));
  fireEvent.change(screen.getByRole("combobox", { name: "Backups to keep" }), { target: { value: "10" } });
  await waitFor(() => expect(getPref("keepBackups")).toBe(10));
  expect(api.setBackupRetention).toHaveBeenCalledWith(10);
  unmount();
  localStorage.clear();
  api.backupRetention.mockResolvedValue(10);
  setup();
  await waitFor(() => expect(screen.getByRole<HTMLSelectElement>("combobox", { name: "Backups to keep" }).value).toBe("10"));
});

it("keeps the old retention choice and reports a failed native save", async () => {
  setup();
  await waitFor(() => expect(getPref("keepBackups")).toBe(5));
  api.setBackupRetention.mockRejectedValue(new Error("Disk unavailable"));
  fireEvent.change(screen.getByRole("combobox", { name: "Backups to keep" }), { target: { value: "30" } });
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Disk unavailable"));
  expect(getPref("keepBackups")).toBe(5);
});

it("accepts a portable JSON file through the existing restore picker", async () => {
  const { container, onImportFile } = setup();
  const file = new File([JSON.stringify(demoBudget())], "budget-backup.json", { type: "application/json" });
  const picker = container.querySelector<HTMLInputElement>('input[type="file"]')!;
  expect(picker.accept).toContain(".json");
  fireEvent.change(picker, { target: { files: [file] } });
  expect(onImportFile).toHaveBeenCalledWith(file);
});
