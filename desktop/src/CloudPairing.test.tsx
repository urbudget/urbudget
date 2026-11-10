// Adding a device without typing the passphrase: one computer shows a one-time code, the other
// takes it. The code and the key are the engine's; these check what the window does with them.
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SyncSettings } from "./CloudSettings";
import type { Cloud, CloudStatus, Overview, PairingOffer } from "./cloud";
import { ToastProvider } from "./ui";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("./bridge", () => ({ desktop: true, api: { library: async () => ({ version: "1", activeId: null, entries: [] }) } }));

let status: CloudStatus;
let offers: PairingOffer[];
let accept: (code: string) => Promise<{ created: boolean }>;
/** What the engine answers each time this computer asks whether its own code was approved. */
let waits: { ready: boolean }[];
const overview: Overview = {
  user: { id: "user", email: "person@example.test", createdAt: "2026-01-01" },
  subscription: null,
  sync: { active: true, budgets: 1, bytes: 1 },
};
const commands = (command: string) => native.invoke.mock.calls.filter(([name]) => name === command);
const offer = (id: string, expiresAt = new Date(Date.now() + 5 * 60_000).toISOString()): PairingOffer => ({
  id,
  code: "0123-4567-89AB-CDEF-GHJK-MNPQ",
  // A 3×3 grid stands in for the real one: dark, light, dark on the first row.
  qr: { size: 3, modules: "101010101" },
  expiresAt,
});
const view = (model: Cloud) => (
  <ToastProvider>
    <SyncSettings cloud={model} budgetId="test" isClean={() => true} openBudget={vi.fn()} />
  </ToastProvider>
);
const state = (): Cloud => ({ status, setStatus: vi.fn(), refresh: vi.fn(async () => status), syncNow: vi.fn(async () => null), syncing: false });

beforeEach(() => {
  vi.clearAllMocks();
  status = { email: "person@example.test", credentialStore: "Keychain", sync: { unlocked: false, last: null }, site: "https://urbudget.app" };
  offers = [offer("first"), offer("second")];
  accept = async () => ({ created: false });
  waits = [];
  native.invoke.mockImplementation(async (command: string, args?: { code?: string }) => {
    if (command === "cloud_status") return status;
    if (command === "account_overview") return overview;
    // An account whose key is made from a passphrase: the code is the other way in.
    if (command === "sync_key_info") return { exists: true, passphrase: true, recovery: false, setup: "code", reset: "code" };
    if (command === "sync_pair_offer") return offers.shift();
    if (command === "sync_pair_cancel") return null;
    if (command === "sync_unlock_code") return accept(args!.code!);
    if (command === "sync_pair_request") return { ...offer("asked"), code: "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ" };
    if (command === "sync_pair_wait") return waits.shift() ?? { ready: false };
    throw new Error(`Unexpected native command: ${command}`);
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it("unlocks sync with a code from another device, and never asks for the passphrase", async () => {
  const model = state();
  render(view(model));
  fireEvent.click(await screen.findByText("Use a code from another device instead"));
  const unlock = screen.getByText("Unlock sync").closest("button")!;
  expect(unlock.disabled).toBe(true);
  // Typed the way people type it: lower case, with spaces.
  fireEvent.change(screen.getByLabelText("Or type a code from another device"), { target: { value: "0123 4567 89ab cdef ghjk mnpq" } });
  expect(unlock.disabled).toBe(false);
  fireEvent.click(unlock);
  await waitFor(() => expect(model.syncNow).toHaveBeenCalled());
  expect(commands("sync_unlock_code")).toEqual([["sync_unlock_code", { code: "0123 4567 89ab cdef ghjk mnpq" }]]);
  expect(commands("sync_setup")).toEqual([]);
  expect(await screen.findByText("Sync is unlocked on this computer.")).toBeTruthy();
});

it("shows its own code for a phone to scan, and unlocks once the phone approves", async () => {
  // Faked before the screen opens, so its first question is on the faked clock too.
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const model = state();
  render(view(model));
  fireEvent.click(await screen.findByText("Use a code from another device instead"));
  // A computer has no camera: the phone that syncs does the scanning.
  expect(await screen.findByRole("img", { name: "QR code for unlocking sync on this computer" })).toBeTruthy();
  waits = [{ ready: false }, { ready: true }];
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(commands("sync_pair_wait")).toEqual([["sync_pair_wait", { code: "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ" }]]);
  expect(model.syncNow).not.toHaveBeenCalled();
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(commands("sync_pair_wait")).toHaveLength(2);
  expect(model.refresh).toHaveBeenCalled();
  expect(model.syncNow).toHaveBeenCalled();
  // Nothing typed, and it stops asking once it has the answer.
  expect(commands("sync_unlock_code")).toEqual([]);
  expect(commands("sync_setup")).toEqual([]);
  await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
  expect(commands("sync_pair_wait")).toHaveLength(2);
});

it("stops asking when its code runs out or the screen is left", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  render(view(state()));
  fireEvent.click(await screen.findByText("Use a code from another device instead"));
  await screen.findByRole("img", { name: "QR code for unlocking sync on this computer" });
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(commands("sync_pair_wait")).toHaveLength(1);
  fireEvent.click(screen.getByText("Use the sync passphrase instead"));
  await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
  expect(commands("sync_pair_wait")).toHaveLength(1);
  // Back again: a new code, and after five minutes it says so instead of asking for ever.
  fireEvent.click(screen.getByText("Use a code from another device instead"));
  await act(async () => { await vi.advanceTimersByTimeAsync(6 * 60_000); });
  expect(screen.getByText("This code has expired.")).toBeTruthy();
  expect(screen.queryByRole("img")).toBeNull();
  const asked = commands("sync_pair_wait").length;
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  expect(commands("sync_pair_wait")).toHaveLength(asked);
  expect(commands("sync_pair_request")).toHaveLength(2);
});

it("says what was wrong with a code and lets the person go back to the passphrase", async () => {
  accept = async () => {
    throw "That code has expired or was already used. Make a new one on the other device.";
  };
  const model = state();
  render(view(model));
  fireEvent.click(await screen.findByText("Use a code from another device instead"));
  fireEvent.change(screen.getByLabelText("Or type a code from another device"), { target: { value: "0123-4567-89AB-CDEF-GHJK-MNPQ" } });
  fireEvent.click(screen.getByText("Unlock sync"));
  expect(await screen.findByText("That code has expired or was already used. Make a new one on the other device.")).toBeTruthy();
  expect(model.syncNow).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Use the sync passphrase instead"));
  expect(screen.getByLabelText("Sync passphrase")).toBeTruthy();
});

it("shows a code for another device, and takes it back when the screen closes", async () => {
  status = { ...status, sync: { protocol: 2, unlocked: true, last: { at: "2026-11-10T00:00:00Z", state: "ok", message: null, openChanged: false, budgets: [] } } };
  render(view(state()));
  fireEvent.click(await screen.findByText("Add a device…"));
  expect(await screen.findByText("0123-4567-89AB-CDEF-GHJK-MNPQ")).toBeTruthy();
  const qr = screen.getByRole("img", { name: "QR code for adding a device" });
  // The margin scanners need, and one square per dark module.
  expect(qr.getAttribute("viewBox")).toBe("0 0 11 11");
  expect(qr.querySelector("path")!.getAttribute("d")!.match(/M/g)).toHaveLength(5);
  expect(screen.getByText(/UrBudget will never ask you for it/)).toBeTruthy();
  expect(commands("sync_pair_cancel")).toEqual([]);
  fireEvent.click(screen.getByText("Done"));
  await waitFor(() => expect(commands("sync_pair_cancel")).toEqual([["sync_pair_cancel", { id: "first" }]]));
  expect(screen.queryByText("0123-4567-89AB-CDEF-GHJK-MNPQ")).toBeNull();
});

it("replaces a code that has run out with a new one", async () => {
  status = { ...status, sync: { protocol: 2, unlocked: true, last: { at: "2026-11-10T00:00:00Z", state: "ok", message: null, openChanged: false, budgets: [] } } };
  offers = [offer("first", new Date(Date.now() + 40).toISOString()), offer("second")];
  render(view(state()));
  fireEvent.click(await screen.findByText("Add a device…"));
  expect(await screen.findByText("This code has expired.")).toBeTruthy();
  expect(screen.queryByText("0123-4567-89AB-CDEF-GHJK-MNPQ")).toBeNull();
  fireEvent.click(screen.getByText("Make a new code"));
  expect(await screen.findByText("0123-4567-89AB-CDEF-GHJK-MNPQ")).toBeTruthy();
  expect(commands("sync_pair_offer")).toHaveLength(2);
  // The one that ran out is withdrawn too, so a late answer to it finds nothing.
  expect(commands("sync_pair_cancel")).toEqual([["sync_pair_cancel", { id: "first" }]]);
});

it("says so when this computer can't make a code", async () => {
  status = { ...status, sync: { protocol: 2, unlocked: true, last: { at: "2026-11-10T00:00:00Z", state: "ok", message: null, openChanged: false, budgets: [] } } };
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "cloud_status") return status;
    if (command === "account_overview") return overview;
    if (command === "sync_pair_offer") throw "Too many devices are waiting to be added. Finish or cancel one first.";
    throw new Error(`Unexpected native command: ${command}`);
  });
  render(view(state()));
  fireEvent.click(await screen.findByText("Add a device…"));
  expect(await screen.findByText("Too many devices are waiting to be added. Finish or cancel one first.")).toBeTruthy();
  expect(screen.queryByRole("img")).toBeNull();
});
