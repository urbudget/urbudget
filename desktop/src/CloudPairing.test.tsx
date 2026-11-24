// Getting the sync key onto another device with nothing typed from memory: a computer that
// doesn't sync shows a code for a phone to scan, a computer that does shows one for the new
// device, and one box takes whatever was typed instead. The codes and the key are the engine's;
// these check what the window does with them.
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SyncSettings } from "./CloudSettings";
import type { Cloud, CloudStatus, Overview, PairingCode, SyncKeyInfo } from "./cloud";
import { ToastProvider } from "./ui";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("./bridge", () => ({ desktop: true, api: { library: async () => ({ version: "1", activeId: null, entries: [] }) } }));

let status: CloudStatus;
let info: SyncKeyInfo;
let offers: (PairingCode | Error)[];
let requests: PairingCode[];
let unlock: (secret: string) => Promise<unknown>;
/** What the engine answers each time this computer asks whether its own code was approved. */
let waits: (object | Error)[];
/** And each time it asks whether the code it offered is still waiting. */
let states: ({ waiting: boolean; collected: boolean } | Error)[];
const overview: Overview = {
  user: { id: "user", email: "person@example.test", createdAt: "2026-01-01" },
  subscription: null,
  sync: { active: true, budgets: 1, bytes: 1 },
};
const OFFERED = "0123-4567-89AB-CDEF-GHJK-MNPQ";
const ASKED = "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ";
const UNLOCK_QR = "QR code for unlocking sync on this computer";
const commands = (command: string) => native.invoke.mock.calls.filter(([name]) => name === command);
const inMinutes = (minutes: number) => new Date(Date.now() + minutes * 60_000).toISOString();
const shown = (id: string, code: string, expiresAt = inMinutes(5)): PairingCode => ({
  id,
  code,
  // A 3×3 grid stands in for the real one: dark, light, dark on the first row.
  qr: { size: 3, modules: "101010101" },
  expiresAt,
});
const next = <T,>(queue: (T | Error)[], otherwise: T) => {
  const answer = queue.shift() ?? otherwise;
  if (answer instanceof Error) throw answer.message;
  return answer;
};
const view = (model: Cloud) => (
  <ToastProvider>
    <SyncSettings cloud={model} budgetId="test" isClean={() => true} openBudget={vi.fn()} />
  </ToastProvider>
);
const state = (): Cloud => ({ status, setStatus: vi.fn(), refresh: vi.fn(async () => status), syncNow: vi.fn(async () => null), syncing: false });
const syncing = (): CloudStatus => ({
  ...status,
  sync: { protocol: 2, unlocked: true, secret: "code", last: { at: "2026-11-24T00:00:00Z", state: "ok", message: null, openChanged: false, budgets: [] } },
});
const tick = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

beforeEach(() => {
  vi.clearAllMocks();
  status = { email: "person@example.test", credentialStore: "Keychain", sync: { unlocked: false, last: null }, site: "https://urbudget.app" };
  info = { exists: true, secret: "code", recovery: false, protocol: 2 };
  offers = [shown("first", OFFERED), shown("second", OFFERED)];
  requests = [shown("asked", ASKED), shown("asked-again", ASKED)];
  unlock = async () => ({ created: false, protocol: 2 });
  waits = [];
  states = [];
  native.invoke.mockImplementation(async (command: string, args?: { secret?: string }) => {
    if (command === "cloud_status") return status;
    if (command === "account_overview") return overview;
    if (command === "sync_key_info") return info;
    if (command === "sync_unlock") return unlock(args!.secret!);
    if (command === "sync_pair_request") return requests.shift();
    if (command === "sync_pair_wait") return next(waits, { ready: false });
    if (command === "sync_pair_offer") return next(offers, shown("spare", OFFERED));
    if (command === "sync_pair_state") return next(states, { waiting: true, collected: false });
    if (command === "sync_pair_cancel") return null;
    if (command === "sync_devices") return { devices: [] };
    throw new Error(`Unexpected native command: ${command}`);
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it("shows its own code for a phone to scan, and unlocks once the phone approves", async () => {
  // Faked before the screen opens, so its first question is on the faked clock too.
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const model = state();
  const page = render(view(model));
  // A computer has no camera: the phone that syncs does the scanning.
  const qr = await screen.findByRole("img", { name: UNLOCK_QR });
  expect(screen.getByText("On a phone that syncs, open Account & sync › Add a device and scan this code.")).toBeTruthy();
  // The characters are there for whoever can't scan, and are what the picture is described by.
  expect(document.getElementById(qr.getAttribute("aria-describedby")!)!.textContent).toBe(ASKED);
  waits = [{ ready: false }, { ready: true, created: false, protocol: 2 }];
  await tick(3000);
  expect(commands("sync_pair_wait")).toEqual([["sync_pair_wait", { code: ASKED }]]);
  expect(model.syncNow).not.toHaveBeenCalled();
  await tick(3000);
  expect(commands("sync_pair_wait")).toHaveLength(2);
  expect(model.refresh).toHaveBeenCalled();
  expect(model.syncNow).toHaveBeenCalled();
  expect(screen.getByText("Sync is unlocked on this computer.")).toBeTruthy();
  // Nothing was typed, and it stops asking once it has the answer.
  expect(commands("sync_unlock")).toEqual([]);
  await tick(10_000);
  expect(commands("sync_pair_wait")).toHaveLength(2);
  // The phone's sealed key was collected: there is nothing left to take back.
  page.unmount();
  expect(commands("sync_pair_cancel")).toEqual([]);
});

it("stops asking and drops its code when the screen is left", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const page = render(view(state()));
  await screen.findByRole("img", { name: UNLOCK_QR });
  await tick(3000);
  expect(commands("sync_pair_wait")).toHaveLength(1);
  page.unmount();
  await tick(10_000);
  expect(commands("sync_pair_wait")).toHaveLength(1);
  expect(commands("sync_pair_cancel")).toEqual([["sync_pair_cancel", { id: "asked" }]]);
});

it("offers a new code once its own has run out", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  // The second is made after the first ran out.
  requests = [shown("asked", ASKED), shown("asked-again", ASKED, inMinutes(30))];
  render(view(state()));
  await screen.findByRole("img", { name: UNLOCK_QR });
  await tick(6 * 60_000);
  expect(screen.getByText("This code has expired.")).toBeTruthy();
  expect(screen.queryByRole("img")).toBeNull();
  // It says so instead of asking for ever.
  const asked = commands("sync_pair_wait").length;
  await tick(60_000);
  expect(commands("sync_pair_wait")).toHaveLength(asked);
  // The box for a typed code is still there.
  expect(screen.getByLabelText("Code from another device, or your recovery code")).toBeTruthy();
  fireEvent.click(screen.getByText("Show a new code"));
  expect(await screen.findByRole("img", { name: UNLOCK_QR })).toBeTruthy();
  expect(commands("sync_pair_request")).toHaveLength(2);
  await tick(3000);
  expect(commands("sync_pair_wait")).toHaveLength(asked + 1);
});

it("says why when the phone's answer couldn't be used, and offers a new code", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const model = state();
  render(view(model));
  await screen.findByRole("img", { name: UNLOCK_QR });
  waits = [new Error("That code doesn't open what the other device sent. Make a new one and try again.")];
  await tick(3000);
  expect(screen.getByText(/That code doesn't open what the other device sent/)).toBeTruthy();
  expect(screen.getByText("Show a new code")).toBeTruthy();
  expect(screen.queryByRole("img")).toBeNull();
  expect(model.syncNow).not.toHaveBeenCalled();
  await tick(10_000);
  expect(commands("sync_pair_wait")).toHaveLength(1);
});

it("unlocks with a typed code, in the one box there is", async () => {
  const model = state();
  render(view(model));
  const box = (await screen.findByLabelText("Code from another device, or your recovery code")) as HTMLInputElement;
  // The account has no passphrase, so nothing mentions one or hides what's typed.
  expect(box.type).toBe("text");
  expect(screen.queryByText(/passphrase/)).toBeNull();
  const button = screen.getByText("Unlock sync").closest("button")!;
  expect(button.disabled).toBe(true);
  fireEvent.change(box, { target: { value: "0123 4567 89ab" } });
  expect(button.disabled).toBe(true);
  // Typed the way people type it: lower case, with spaces. The engine reads it.
  fireEvent.change(box, { target: { value: "0123 4567 89ab cdef ghjk mnpq" } });
  expect(button.disabled).toBe(false);
  fireEvent.click(button);
  await waitFor(() => expect(model.syncNow).toHaveBeenCalled());
  expect(commands("sync_unlock")).toEqual([["sync_unlock", { secret: "0123 4567 89ab cdef ghjk mnpq" }]]);
  expect(await screen.findByText("Sync is unlocked on this computer.")).toBeTruthy();
  // Unlocking with what someone already has makes no new code to save.
  expect(screen.queryByText("Save your recovery code")).toBeNull();
});

it("takes the passphrase of an account that has one in the same box", async () => {
  info = { exists: true, secret: "passphrase", recovery: false, protocol: 2 };
  const model = state();
  render(view(model));
  const box = (await screen.findByLabelText("Code from another device, your recovery code, or your sync passphrase")) as HTMLInputElement;
  expect(box.type).toBe("password");
  const button = screen.getByText("Unlock sync").closest("button")!;
  fireEvent.change(box, { target: { value: "short" } });
  expect(button.disabled).toBe(true);
  fireEvent.change(box, { target: { value: "correct horse battery" } });
  // A code typed there can be read back.
  fireEvent.click(screen.getByLabelText("Show what I type"));
  expect(box.type).toBe("text");
  fireEvent.click(button);
  await waitFor(() => expect(commands("sync_unlock")).toEqual([["sync_unlock", { secret: "correct horse battery" }]]));
  await waitFor(() => expect(model.syncNow).toHaveBeenCalled());
});

it("says what was wrong with what was typed and keeps it to correct", async () => {
  unlock = async () => {
    throw "That code has a typo. Check each character and try again.";
  };
  const model = state();
  render(view(model));
  const box = (await screen.findByLabelText("Code from another device, or your recovery code")) as HTMLInputElement;
  fireEvent.change(box, { target: { value: OFFERED } });
  fireEvent.click(screen.getByText("Unlock sync"));
  expect(await screen.findByText("That code has a typo. Check each character and try again.")).toBeTruthy();
  expect(model.syncNow).not.toHaveBeenCalled();
  expect(box.value).toBe(OFFERED);
  await waitFor(() => expect(screen.getByText("Unlock sync").closest("button")!.disabled).toBe(false));
});

it("shows a code for another device and closes by itself once that device has the key", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  status = syncing();
  render(view(state()));
  fireEvent.click(await screen.findByText("Add a device"));
  expect(await screen.findByText(OFFERED)).toBeTruthy();
  const qr = screen.getByRole("img", { name: "QR code for adding a device" });
  // The margin scanners need, and one square per dark module.
  expect(qr.getAttribute("viewBox")).toBe("0 0 11 11");
  expect(qr.querySelector("path")!.getAttribute("d")!.match(/M/g)).toHaveLength(5);
  expect(screen.getByText(/Scan this code with the new phone, or type its characters on the new computer/)).toBeTruthy();
  expect(screen.getByText(/It works once, until .+\. Use it only on a device you're holding/)).toBeTruthy();
  states = [{ waiting: true, collected: false }, { waiting: false, collected: true }];
  await tick(3000);
  expect(commands("sync_pair_state")).toEqual([["sync_pair_state", { id: "first" }]]);
  expect(screen.getByRole("dialog", { name: "Add a device" })).toBeTruthy();
  await tick(3000);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByText("Device added.")).toBeTruthy();
  // Used, so there is nothing to take back, and nothing more to ask.
  expect(commands("sync_pair_cancel")).toEqual([]);
  await tick(10_000);
  expect(commands("sync_pair_state")).toHaveLength(2);
});

it("takes its code back when Add a device is closed", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  status = syncing();
  render(view(state()));
  fireEvent.click(await screen.findByText("Add a device"));
  await screen.findByText(OFFERED);
  expect(commands("sync_pair_cancel")).toEqual([]);
  fireEvent.click(screen.getByText("Cancel"));
  expect(commands("sync_pair_cancel")).toEqual([["sync_pair_cancel", { id: "first" }]]);
  expect(screen.queryByText(OFFERED)).toBeNull();
  expect(screen.queryByText("Device added.")).toBeNull();
  await tick(10_000);
  expect(commands("sync_pair_state")).toEqual([]);
});

it("keeps asking through an answer that didn't arrive", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  status = syncing();
  render(view(state()));
  fireEvent.click(await screen.findByText("Add a device"));
  await screen.findByText(OFFERED);
  states = [new Error("Couldn't reach urbudget.app. Check your internet connection and try again."), { waiting: false, collected: true }];
  await tick(3000);
  expect(screen.getByText(OFFERED)).toBeTruthy();
  await tick(3000);
  expect(screen.getByText("Device added.")).toBeTruthy();
});

it("replaces a code that has run out with a new one", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  status = syncing();
  offers = [shown("first", OFFERED, inMinutes(1)), shown("second", OFFERED)];
  // The server's clock may be seconds ahead: it says the code is neither waiting nor
  // collected, which is a code that ran out and not a device added.
  states = [...Array.from({ length: 17 }, () => ({ waiting: true, collected: false })), { waiting: false, collected: false }];
  render(view(state()));
  fireEvent.click(await screen.findByText("Add a device"));
  await screen.findByText(OFFERED);
  await tick(55_000);
  expect(commands("sync_pair_state")).toHaveLength(18);
  expect(screen.getByText("This code has expired.")).toBeTruthy();
  expect(screen.queryByText(OFFERED)).toBeNull();
  expect(screen.queryByText("Device added.")).toBeNull();
  fireEvent.click(screen.getByText("Show a new code"));
  expect(await screen.findByText(OFFERED)).toBeTruthy();
  expect(commands("sync_pair_offer")).toHaveLength(2);
  // The one that ran out is withdrawn too, so a late answer to it finds nothing.
  expect(commands("sync_pair_cancel")).toEqual([["sync_pair_cancel", { id: "first" }]]);
});

it("says so when this computer can't make a code", async () => {
  status = syncing();
  offers = [new Error("Too many devices are waiting to be added. Finish or cancel one first.")];
  render(view(state()));
  fireEvent.click(await screen.findByText("Add a device"));
  expect((await screen.findByRole("alert")).textContent).toBe("Too many devices are waiting to be added. Finish or cancel one first.");
  expect(screen.queryByRole("img")).toBeNull();
  fireEvent.click(screen.getByText("Show a new code"));
  expect(await screen.findByText(OFFERED)).toBeTruthy();
});
