import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SyncSettings } from "./CloudSettings";
import type { Cloud, CloudStatus, Overview } from "./cloud";
import { ToastProvider } from "./ui";

// Settings › Account & sync for an account that still syncs on protocol 1 (sync.rs `sync_upgrade`).
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("./bridge", () => ({ desktop: true, api: { library: async () => ({ version: "1", activeId: null, entries: [] }) } }));
let status: CloudStatus;
let upgrade: () => Promise<unknown>;
const overview: Overview = { user: { id: "user", email: "person@example.test", createdAt: "2026-01-01" }, subscription: null, sync: { active: true, budgets: 1, bytes: 1 } };
const upgrades = () => native.invoke.mock.calls.filter(([name]) => name === "sync_upgrade");
const model = (): Cloud => ({ status, setStatus: vi.fn(), refresh: vi.fn(async () => status), syncNow: vi.fn(async () => null), syncing: false });
const view = (cloud: Cloud) => render(<ToastProvider><SyncSettings cloud={cloud} budgetId="" isClean={() => true} openBudget={vi.fn()} /></ToastProvider>);
const v1 = (sync: Partial<CloudStatus["sync"]>): CloudStatus => ({
  email: "person@example.test", credentialStore: "Keychain", site: "https://urbudget.app",
  sync: { unlocked: true, protocol: 1, last: null, upgradeAvailable: true, upgradeNeedsPassphrase: false, ...sync },
});

beforeEach(() => {
  vi.clearAllMocks();
  status = v1({});
  upgrade = async () => ({ created: false, protocol: 2 });
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "account_overview") return overview;
    if (command === "sync_upgrade") return upgrade();
    throw new Error(`Unexpected native command: ${command}`);
  });
});
afterEach(cleanup);

it("warns about older versions before moving with this computer's saved key", async () => {
  const cloud = model();
  view(cloud);
  fireEvent.click(await screen.findByRole("button", { name: "Move to the new sync" }));
  expect(screen.getByText(/needs desktop 1\.0\.11 or later, Android 1\.0\.1 or later, or iPhone 1\.0\.1 or later/)).toBeTruthy();
  expect(screen.queryByLabelText("Sync passphrase")).toBeNull();
  expect(upgrades()).toHaveLength(0);
  const dialog = screen.getByRole("dialog");
  fireEvent.click(Array.from(dialog.querySelectorAll("button")).find(b => b.textContent?.includes("Move to the new sync"))!);
  await waitFor(() => expect(cloud.syncNow).toHaveBeenCalledOnce());
  expect(upgrades()).toEqual([["sync_upgrade", { passphrase: null }]]);
  expect(cloud.refresh).toHaveBeenCalled();
  expect(await screen.findByText("This account now uses the new sync.")).toBeTruthy();
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("asks for the passphrase when this computer has no key that opens the account", async () => {
  status = v1({ unlocked: false, upgradeNeedsPassphrase: true });
  view(model());
  fireEvent.click(await screen.findByRole("button", { name: "Move to the new sync" }));
  const input = screen.getByLabelText("Sync passphrase") as HTMLInputElement;
  const move = Array.from(screen.getByRole("dialog").querySelectorAll("button")).find(b => b.textContent?.includes("Move to the new sync"))!;
  fireEvent.change(input, { target: { value: "short" } });
  expect(move.disabled).toBe(true);
  fireEvent.change(input, { target: { value: "correct horse battery" } });
  fireEvent.click(move);
  await waitFor(() => expect(upgrades()).toEqual([["sync_upgrade", { passphrase: "correct horse battery" }]]));
});

it("keeps the dialog open with the engine's answer when the move fails", async () => {
  upgrade = async () => { throw new Error("Enter your sync passphrase to move this account to the new sync."); };
  const cloud = model();
  view(cloud);
  fireEvent.click(await screen.findByRole("button", { name: "Move to the new sync" }));
  fireEvent.click(Array.from(screen.getByRole("dialog").querySelectorAll("button")).find(b => b.textContent?.includes("Move to the new sync"))!);
  expect((await screen.findByRole("alert")).textContent).toBe("Enter your sync passphrase to move this account to the new sync.");
  expect(screen.getByRole("dialog")).toBeTruthy();
  // The refreshed status decides whether the passphrase field now shows.
  await waitFor(() => expect(cloud.refresh).toHaveBeenCalled());
  expect(cloud.syncNow).not.toHaveBeenCalled();
});

it("offers nothing to accounts that can't move or already have", async () => {
  for (const sync of [{ upgradeAvailable: false }, { protocol: 2, upgradeAvailable: false }, { upgradeAvailable: undefined }]) {
    status = v1(sync);
    const { unmount } = view(model());
    await screen.findByText("Sync");
    expect(screen.queryByRole("button", { name: "Move to the new sync" })).toBeNull();
    expect(screen.queryByText(/Automatic budget discovery/)).toBeNull();
    unmount();
  }
});
