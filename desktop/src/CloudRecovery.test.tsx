// Sync with nothing to choose: turning it on makes a recovery code, which stays on screen until
// the person says it's saved, and a reset or a replacement makes another. The key and the codes
// are the engine's; these check what the window does with them.
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
/** What the engine answers each command with. A function is called; an Error is thrown as its text. */
let answers: Record<string, unknown>;
const overview: Overview = {
  user: { id: "user", email: "person@example.test", createdAt: "2026-01-01" },
  subscription: null,
  sync: { active: true, budgets: 1, bytes: 1 },
};
const CODE = "0123-4567-89AB-CDEF-GHJK-MNPR";
const commands = (command: string) => native.invoke.mock.calls.filter(([name]) => name === command);
const view = (model: Cloud) => (
  <ToastProvider>
    <SyncSettings cloud={model} budgetId="test" isClean={() => true} openBudget={vi.fn()} />
  </ToastProvider>
);
const state = (): Cloud => ({ status, setStatus: vi.fn(), refresh: vi.fn(async () => status), syncNow: vi.fn(async () => null), syncing: false });
const unlocked = (sync: Partial<CloudStatus["sync"]> = {}): CloudStatus => ({
  ...status,
  sync: { protocol: 2, unlocked: true, secret: "code", recovery: false, recoveryUnsaved: false, last: { at: "2026-11-24T00:00:00Z", state: "ok", message: null, openChanged: false, budgets: [] }, ...sync },
});

beforeEach(() => {
  vi.clearAllMocks();
  status = { email: "person@example.test", credentialStore: "Keychain", sync: { unlocked: false, last: null, secret: null }, site: "https://urbudget.app" };
  info = { exists: false, secret: null, recovery: false, protocol: 2 };
  answers = {
    sync_recovery_saved: null,
    sync_pair_request: { id: "asked", code: "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ", qr: { size: 3, modules: "101010101" }, expiresAt: new Date(Date.now() + 5 * 60_000).toISOString() },
    sync_pair_wait: { ready: false },
    sync_pair_cancel: null,
  };
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "cloud_status") return status;
    if (command === "account_overview") return overview;
    if (command === "sync_key_info") {
      if (info instanceof Error) throw info.message;
      return info;
    }
    if (!(command in answers)) throw new Error(`Unexpected native command: ${command}`);
    const answer = answers[command];
    if (answer instanceof Error) throw answer.message;
    return typeof answer === "function" ? answer() : answer;
  });
});
afterEach(cleanup);

it("turns sync on with nothing to choose, and keeps the recovery code up until it's saved", async () => {
  answers.sync_setup = { created: true, protocol: 2, recoveryCode: CODE };
  const model = state();
  render(view(model));
  fireEvent.click(await screen.findByText("Turn on sync"));
  expect(await screen.findByText(CODE)).toBeTruthy();
  expect(screen.getByRole("dialog", { name: "Save your recovery code" })).toBeTruthy();
  expect(commands("sync_setup")).toEqual([["sync_setup", { passphrase: null }]]);
  // Nothing asks for a passphrase, to set sync up or anywhere else.
  expect(document.querySelector("input[type=password]")).toBeNull();
  expect(model.syncNow).toHaveBeenCalled();
  expect(await screen.findByText("Sync is on. Your budgets are uploading, encrypted.")).toBeTruthy();
  // Escape, a click outside and the heading's close button leave it up. One button closes it.
  fireEvent.keyDown(document.body, { key: "Escape" });
  fireEvent.mouseDown(document.querySelector(".modal-shade")!);
  fireEvent.click(screen.getByRole("button", { name: "Close dialog" }));
  expect(screen.getByText(CODE)).toBeTruthy();
  expect(commands("sync_recovery_saved")).toEqual([]);
  fireEvent.click(screen.getByText("I've saved it"));
  await waitFor(() => expect(screen.queryByText(CODE)).toBeNull());
  expect(commands("sync_recovery_saved")).toHaveLength(1);
  // The status is read again, so the code isn't asked for a second time.
  expect(vi.mocked(model.refresh).mock.calls.length).toBeGreaterThanOrEqual(2);
});

it("shows a code nobody confirmed saving when the page opens", async () => {
  status = unlocked({ recoveryUnsaved: true });
  answers.sync_recovery_unsaved = { recoveryCode: CODE };
  const model = state();
  render(view(model));
  expect(await screen.findByText(CODE)).toBeTruthy();
  expect(screen.getByText(/UrBudget can't show it again or recover it/)).toBeTruthy();
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(screen.getByText(CODE)).toBeTruthy();
  fireEvent.click(screen.getByText("I've saved it"));
  await waitFor(() => expect(screen.queryByText(CODE)).toBeNull());
  expect(commands("sync_recovery_saved")).toHaveLength(1);
  expect(model.refresh).toHaveBeenCalled();
});

it("keeps the code up when the engine couldn't let go of it", async () => {
  status = unlocked({ recoveryUnsaved: true });
  answers.sync_recovery_unsaved = { recoveryCode: CODE };
  answers.sync_recovery_saved = new Error("Couldn't reach the keychain.");
  render(view(state()));
  fireEvent.click(await screen.findByText("I've saved it"));
  expect(await screen.findByText("Couldn't reach the keychain.")).toBeTruthy();
  expect(screen.getByText(CODE)).toBeTruthy();
});

it("asks for no code where this computer holds none", async () => {
  status = unlocked();
  render(view(state()));
  await screen.findByText("Last synced", { exact: false });
  expect(commands("sync_recovery_unsaved")).toEqual([]);
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("copies the recovery code", async () => {
  answers.sync_setup = { created: true, protocol: 2, recoveryCode: CODE };
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  render(view(state()));
  fireEvent.click(await screen.findByText("Turn on sync"));
  fireEvent.click(await screen.findByText("Copy"));
  expect(writeText).toHaveBeenCalledWith(CODE);
  expect(await screen.findByText("Recovery code copied.")).toBeTruthy();
});

it("unlocks instead when another device turned sync on first", async () => {
  answers.sync_setup = new Error("Sync is already set up for this account. Unlock it with a code from a device that syncs, or with your recovery code.");
  const model = state();
  render(view(model));
  const turnOn = await screen.findByText("Turn on sync");
  info = { exists: true, secret: "code", recovery: false, protocol: 2 };
  fireEvent.click(turnOn);
  expect(await screen.findByText(/Sync is already set up for this account/)).toBeTruthy();
  expect(await screen.findByLabelText("Code from another device, or your recovery code")).toBeTruthy();
  expect(screen.queryByText("Turn on sync")).toBeNull();
  expect(screen.queryByText("Save your recovery code")).toBeNull();
  expect(model.syncNow).not.toHaveBeenCalled();
});

it("says so when it can't find out how the account syncs, and tries again", async () => {
  info = new Error("Couldn't reach urbudget.app. Check your internet connection and try again.");
  render(view(state()));
  expect(await screen.findByText("Couldn't reach urbudget.app. Check your internet connection and try again.")).toBeTruthy();
  info = { exists: false, secret: null, recovery: false, protocol: 2 };
  fireEvent.click(screen.getByText("Try again"));
  expect(await screen.findByText("Turn on sync")).toBeTruthy();
});

it("makes another recovery code where sync is unlocked, after saying what keeps working", async () => {
  status = unlocked();
  answers.sync_recovery_create = { recoveryCode: CODE };
  render(view(state()));
  fireEvent.click(await screen.findByText("New recovery code…"));
  expect(screen.getByText("The code or passphrase you already have keeps working. To retire it, reset sync.")).toBeTruthy();
  expect(commands("sync_recovery_create")).toEqual([]);
  fireEvent.click(screen.getByText("Make a new code"));
  expect(await screen.findByText(CODE)).toBeTruthy();
  expect(commands("sync_recovery_create")).toHaveLength(1);
  // One dialog at a time: the question has gone, and the code is the only thing open.
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  expect(screen.getByRole("dialog", { name: "Save your recovery code" })).toBeTruthy();
});

it("says a replacement code takes the place of the one before it", async () => {
  status = unlocked({ secret: "passphrase", recovery: true });
  render(view(state()));
  fireEvent.click(await screen.findByText("New recovery code…"));
  expect(screen.getByText(/The new code replaces the last one made this way/)).toBeTruthy();
  fireEvent.click(screen.getByText("Cancel"));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(commands("sync_recovery_create")).toEqual([]);
});

it("offers no recovery code to an account still on the older sync, which can't keep one", async () => {
  status = unlocked({ protocol: 1, secret: "passphrase" });
  render(view(state()));
  expect(await screen.findByText("Reset sync…")).toBeTruthy();
  expect(screen.getByText("Lock on this computer")).toBeTruthy();
  expect(screen.queryByText("New recovery code…")).toBeNull();
});

it("resets sync with nothing to type and shows the new recovery code", async () => {
  status = unlocked({ secret: "passphrase" });
  answers.sync_reset = { created: false, protocol: 2, recoveryCode: CODE };
  const model = state();
  render(view(model));
  fireEvent.click(await screen.findByText("Reset sync…"));
  // On the new sync the computer that resets uploads the budgets it was syncing by itself, and
  // every other device keeps its own copy to recover.
  expect(screen.getByText(/the cloud copies made with the old key are deleted\./)).toBeTruthy();
  expect(screen.getByText("This computer uploads the budgets it was syncing again, and they keep syncing.")).toBeTruthy();
  expect(screen.getByText(/Your other devices are unlocked with the new recovery code/)).toBeTruthy();
  expect(screen.getByText(/keeps its own copy in the budget library\. Recover as a copy brings back anything only that device had\./)).toBeTruthy();
  // Protocol 1's wording, where every budget on this computer uploads, is not shown here.
  expect(screen.queryByText(/this computer's budgets upload again/)).toBeNull();
  // Said only to someone who can't unlock: this computer already syncs.
  expect(screen.queryByText(/add this computer from it instead/)).toBeNull();
  expect(screen.getByRole("dialog").querySelector("input")).toBeNull();
  fireEvent.click(screen.getByText("Reset sync"));
  expect(await screen.findByText(CODE)).toBeTruthy();
  expect(commands("sync_reset")).toEqual([["sync_reset", { passphrase: null }]]);
  expect(model.syncNow).toHaveBeenCalled();
  expect(await screen.findByText("Sync was reset.")).toBeTruthy();
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
});

it("lets someone who can't unlock start over, and says what that costs", async () => {
  info = { exists: true, secret: "code", recovery: true, protocol: 2 };
  answers.sync_reset = { created: false, protocol: 2, recoveryCode: CODE };
  render(view(state()));
  fireEvent.click(await screen.findByText("Can't unlock?"));
  expect(screen.getByText(/add this computer from it instead/)).toBeTruthy();
  expect(commands("sync_reset")).toEqual([]);
  fireEvent.click(screen.getByText("Reset sync"));
  expect(await screen.findByText(CODE)).toBeTruthy();
  expect(commands("sync_reset")).toEqual([["sync_reset", { passphrase: null }]]);
});

it("leaves sync as it is when a reset fails", async () => {
  status = unlocked();
  answers.sync_reset = new Error("Couldn't reach urbudget.app. Check your internet connection and try again.");
  const model = state();
  render(view(model));
  fireEvent.click(await screen.findByText("Reset sync…"));
  fireEvent.click(screen.getByText("Reset sync"));
  expect(await screen.findByText("Couldn't reach urbudget.app. Check your internet connection and try again.")).toBeTruthy();
  expect(screen.getByRole("dialog", { name: "Reset sync?" })).toBeTruthy();
  expect(screen.queryByText("Save your recovery code")).toBeNull();
  expect(model.syncNow).not.toHaveBeenCalled();
});

it("says which secret opens the budgets, for either kind of account", async () => {
  status = unlocked();
  const first = render(view(state()));
  expect(await screen.findByText(/only your devices and your recovery code can open them/)).toBeTruthy();
  first.unmount();
  status = unlocked({ secret: "passphrase" });
  render(view(state()));
  expect(await screen.findByText(/only your devices and your sync passphrase can open them/)).toBeTruthy();
});
