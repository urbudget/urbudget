// Sync with no passphrase to choose: the first computer makes the key, a recovery code is shown
// once, and the other ways in are a code from another device or that recovery code. The key and
// the codes are the engine's; these check what the window does with them.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SyncSettings } from "./CloudSettings";
import type { Cloud, CloudStatus, Overview, SyncKeyInfo } from "./cloud";
import { ToastProvider } from "./ui";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("./bridge", () => ({ desktop: true, api: { library: async () => ({ version: "1", activeId: null, entries: [] }) } }));

let status: CloudStatus;
let info: SyncKeyInfo | Error;
let answers: Record<string, unknown>;
const overview: Overview = {
  user: { id: "user", email: "person@example.test", createdAt: "2026-01-01" },
  subscription: null,
  sync: { active: true, budgets: 1, bytes: 1 },
};
const CODE = "0123-4567-89AB-CDEF-GHJK-MNPQ";
const commands = (command: string) => native.invoke.mock.calls.filter(([name]) => name === command);
const view = (model: Cloud) => (
  <ToastProvider>
    <SyncSettings cloud={model} budgetId="test" isClean={() => true} openBudget={vi.fn()} />
  </ToastProvider>
);
const state = (): Cloud => ({ status, setStatus: vi.fn(), refresh: vi.fn(async () => status), syncNow: vi.fn(async () => null), syncing: false });
const unlocked = (sync: Partial<CloudStatus["sync"]> = {}): CloudStatus => ({
  ...status,
  sync: { protocol: 2, unlocked: true, passphrase: false, recovery: true, last: { at: "2026-11-10T00:00:00Z", state: "ok", message: null, openChanged: false, budgets: [] }, ...sync },
});

beforeEach(() => {
  vi.clearAllMocks();
  status = { email: "person@example.test", credentialStore: "Keychain", sync: { unlocked: false, last: null }, site: "https://urbudget.app" };
  info = { exists: false, passphrase: false, recovery: false, setup: "code", reset: "passphrase" };
  answers = { sync_pair_request: { id: "asked", code: "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ", qr: { size: 3, modules: "101010101" }, expiresAt: new Date(Date.now() + 5 * 60_000).toISOString() }, sync_pair_wait: { ready: false } };
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "cloud_status") return status;
    if (command === "account_overview") return overview;
    if (command === "sync_key_info") {
      if (info instanceof Error) throw info.message;
      return info;
    }
    if (command in answers) return answers[command];
    throw new Error(`Unexpected native command: ${command}`);
  });
});
afterEach(cleanup);

it("turns sync on with nothing to choose, and keeps the recovery code up until it's saved", async () => {
  answers.sync_setup = { created: true, protocol: 2, recoveryCode: CODE };
  const model = state();
  render(view(model));
  fireEvent.click(await screen.findByText("Turn on sync"));
  expect(await screen.findByText(CODE)).toBeTruthy();
  expect(commands("sync_setup")).toEqual([["sync_setup", { passphrase: null }]]);
  expect(screen.queryByLabelText("Sync passphrase")).toBeNull();
  expect(model.syncNow).toHaveBeenCalled();
  expect(await screen.findByText("Sync is on. Your budgets are uploading, encrypted.")).toBeTruthy();
  // The code is shown once, so Escape and a click outside leave it up. One button closes it.
  fireEvent.keyDown(document.body, { key: "Escape" });
  fireEvent.mouseDown(document.querySelector(".modal-shade")!);
  expect(screen.getByText(CODE)).toBeTruthy();
  fireEvent.click(screen.getByText("I've saved it"));
  expect(screen.queryByText(CODE)).toBeNull();
});

it("copies the recovery code", async () => {
  answers.sync_setup = { created: true, protocol: 2, recoveryCode: CODE };
  const writeText = vi.fn(async () => {});
  vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
  render(view(state()));
  fireEvent.click(await screen.findByText("Turn on sync"));
  fireEvent.click(await screen.findByText("Copy"));
  expect(writeText).toHaveBeenCalledWith(CODE);
  expect(await screen.findByText("Recovery code copied.")).toBeTruthy();
  vi.unstubAllGlobals();
});

it("asks for a passphrase where the account's sync still makes its key from one", async () => {
  info = { exists: false, passphrase: false, recovery: false, setup: "passphrase", reset: "passphrase" };
  answers.sync_setup = { created: true };
  render(view(state()));
  fireEvent.change(await screen.findByLabelText("Sync passphrase"), { target: { value: "correct horse battery" } });
  fireEvent.change(screen.getByLabelText("Type it again"), { target: { value: "correct horse battery" } });
  fireEvent.click(screen.getByText("Turn on sync"));
  await waitFor(() => expect(commands("sync_setup")).toEqual([["sync_setup", { passphrase: "correct horse battery" }]]));
  expect(screen.queryByText("Save your recovery code")).toBeNull();
});

it("has one box for a code on an account with no passphrase, and takes the recovery code in it", async () => {
  info = { exists: true, passphrase: false, recovery: true, setup: "code", reset: "code" };
  answers.sync_unlock_code = { created: false, protocol: 2 };
  const model = state();
  render(view(model));
  // No passphrase exists, so nothing asks for one or offers to.
  expect(await screen.findByRole("img", { name: "QR code for unlocking sync on this computer" })).toBeTruthy();
  expect(screen.queryByLabelText("Sync passphrase")).toBeNull();
  expect(screen.queryByText("Use the sync passphrase instead")).toBeNull();
  const unlock = screen.getByText("Unlock sync").closest("button")!;
  expect(unlock.disabled).toBe(true);
  // The same box as for a code from another device: the engine works out which it is.
  fireEvent.change(screen.getByLabelText("Or type a code from another device, or your recovery code"), { target: { value: "0123 4567 89ab cdef ghjk mnpq" } });
  fireEvent.click(unlock);
  await waitFor(() => expect(model.syncNow).toHaveBeenCalled());
  expect(commands("sync_unlock_code")).toEqual([["sync_unlock_code", { code: "0123 4567 89ab cdef ghjk mnpq" }]]);
  expect(await screen.findByText("Sync is unlocked on this computer.")).toBeTruthy();
  // Unlocking with a code someone already has shows no new one.
  expect(screen.queryByText("Save your recovery code")).toBeNull();
});

it("asks an account with a passphrase for it, with a code as the other way, and says what a wrong code did", async () => {
  info = { exists: true, passphrase: true, recovery: false, setup: "code", reset: "code" };
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "cloud_status") return status;
    if (command === "account_overview") return overview;
    if (command === "sync_key_info") return info;
    if (command === "sync_pair_request") return answers.sync_pair_request;
    if (command === "sync_pair_wait") return { ready: false };
    if (command === "sync_unlock_code") throw "That code didn't work. Check it, or make a new one on the other device.";
    throw new Error(`Unexpected native command: ${command}`);
  });
  const model = state();
  render(view(model));
  // The passphrase is the one thing asked for. Nothing mentions a recovery code it doesn't have.
  expect(await screen.findByLabelText("Sync passphrase")).toBeTruthy();
  expect(screen.queryByRole("img")).toBeNull();
  fireEvent.click(screen.getByText("Use a code from another device instead"));
  const box = screen.getByLabelText("Or type a code from another device") as HTMLInputElement;
  fireEvent.change(box, { target: { value: CODE } });
  fireEvent.click(screen.getByText("Unlock sync"));
  expect(await screen.findByText("That code didn't work. Check it, or make a new one on the other device.")).toBeTruthy();
  expect(model.syncNow).not.toHaveBeenCalled();
  // Still there to correct, and the passphrase is one click away.
  expect(box.value).toBe(CODE);
  fireEvent.click(screen.getByText("Use the sync passphrase instead"));
  expect(screen.getByLabelText("Sync passphrase")).toBeTruthy();
});

it("says so when it can't find out how the account syncs, and tries again", async () => {
  info = new Error("Couldn't reach urbudget.app. Check your internet connection and try again.");
  render(view(state()));
  expect(await screen.findByText("Couldn't reach urbudget.app. Check your internet connection and try again.")).toBeTruthy();
  info = { exists: false, passphrase: false, recovery: false, setup: "code", reset: "passphrase" };
  fireEvent.click(screen.getByText("Try again"));
  expect(await screen.findByText("Turn on sync")).toBeTruthy();
});

it("makes a new recovery code where sync is unlocked, and says the old one stops working", async () => {
  status = unlocked();
  answers.sync_recovery_create = { recoveryCode: CODE };
  render(view(state()));
  fireEvent.click(await screen.findByText("Recovery code…"));
  expect(screen.getByText(/The old code stops working and nothing else changes/)).toBeTruthy();
  expect(commands("sync_recovery_create")).toEqual([]);
  fireEvent.click(screen.getByText("Make a new recovery code"));
  expect(await screen.findByText(CODE)).toBeTruthy();
  expect(commands("sync_recovery_create")).toHaveLength(1);
  expect(screen.getByText("Save your recovery code")).toBeTruthy();
});

it("gives an account with a passphrase its first recovery code, and keeps the passphrase", async () => {
  status = unlocked({ passphrase: true, recovery: false });
  answers.sync_recovery_create = { recoveryCode: CODE };
  render(view(state()));
  fireEvent.click(await screen.findByText("Recovery code…"));
  expect(screen.getByText(/Your sync passphrase keeps working/)).toBeTruthy();
  fireEvent.click(screen.getByText("Make a recovery code"));
  expect(await screen.findByText(CODE)).toBeTruthy();
});

it("resets sync with nothing to type and shows the new recovery code", async () => {
  status = unlocked();
  answers.sync_reset = { created: false, protocol: 2, recoveryCode: CODE };
  const model = state();
  render(view(model));
  fireEvent.click(await screen.findByText("Reset sync…"));
  expect(screen.getByText(/deletes the cloud copies made with the old key/)).toBeTruthy();
  expect(screen.queryByLabelText("New passphrase")).toBeNull();
  fireEvent.click(screen.getByText("Reset sync"));
  expect(await screen.findByText(CODE)).toBeTruthy();
  expect(commands("sync_reset")).toEqual([["sync_reset", { passphrase: null }]]);
  expect(await screen.findByText("Sync was reset.")).toBeTruthy();
});

it("keeps the passphrase reset, and no recovery code, for an account still on the older sync", async () => {
  status = unlocked({ protocol: 1, passphrase: true, recovery: false });
  answers.sync_reset = { created: false };
  render(view(state()));
  expect(await screen.findByText("Reset passphrase…")).toBeTruthy();
  expect(screen.queryByText("Recovery code…")).toBeNull();
  fireEvent.click(screen.getByText("Reset passphrase…"));
  fireEvent.change(screen.getByLabelText("New passphrase"), { target: { value: "a brand new passphrase" } });
  fireEvent.change(screen.getByLabelText("Type it again"), { target: { value: "a brand new passphrase" } });
  fireEvent.click(screen.getByText("Reset sync"));
  await waitFor(() => expect(commands("sync_reset")).toEqual([["sync_reset", { passphrase: "a brand new passphrase" }]]));
  expect(screen.queryByText("Save your recovery code")).toBeNull();
});

it("lets someone who can't unlock start over, and says what that costs", async () => {
  info = { exists: true, passphrase: false, recovery: true, setup: "code", reset: "code" };
  answers.sync_reset = { created: false, protocol: 2, recoveryCode: CODE };
  render(view(state()));
  // Folded under "Can't unlock?", beside nothing else on an account with no passphrase.
  expect(await screen.findByText("Can't unlock?")).toBeTruthy();
  fireEvent.click(screen.getByText("Reset sync…"));
  expect(screen.getByText(/add this computer from it instead/)).toBeTruthy();
  fireEvent.click(screen.getByText("Reset sync"));
  expect(await screen.findByText(CODE)).toBeTruthy();
  expect(commands("sync_reset")).toEqual([["sync_reset", { passphrase: null }]]);
});
