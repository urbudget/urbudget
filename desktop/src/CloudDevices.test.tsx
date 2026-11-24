// Account & sync top to bottom: the account, the plan only when it needs something, sync's one
// line, the devices that sync, the budgets that need someone, and More. The list of devices is
// the server's, through the engine; these check what the window does with it.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SyncSettings } from "./CloudSettings";
import type { BudgetSync, Cloud, CloudStatus, Overview, SyncDevice } from "./cloud";
import { ToastProvider } from "./ui";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("./bridge", () => ({ desktop: true, api: { library: async () => ({ version: "1", activeId: null, entries: [] }) } }));

let status: CloudStatus;
let overview: Overview;
/** What the engine answers each time the list of devices is asked for; the last one from then on. */
let lists: (SyncDevice[] | Error)[];
let pairing: { waiting: boolean; collected: boolean };
const manageBudgets = vi.fn();
const commands = (command: string) => native.invoke.mock.calls.filter(([name]) => name === command);
const minutesAgo = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();
const view = (model: Cloud) => (
  <ToastProvider>
    <SyncSettings cloud={model} budgetId="open" isClean={() => true} openBudget={vi.fn()} manageBudgets={manageBudgets} />
  </ToastProvider>
);
const state = (): Cloud => ({ status, setStatus: vi.fn(), refresh: vi.fn(async () => status), syncNow: vi.fn(async () => null), syncing: false });
const budget = (id: string, phase: BudgetSync["state"], more: Partial<BudgetSync> = {}): BudgetSync => ({ id, name: `Budget ${id}`, state: phase, device: null, updatedAt: null, message: null, ...more });
const syncing = (budgets: BudgetSync[] = [], sync: Partial<CloudStatus["sync"]> = {}, report: Partial<NonNullable<CloudStatus["sync"]["last"]>> = {}): CloudStatus => ({
  ...status,
  sync: { protocol: 2, unlocked: true, secret: "code", last: { at: minutesAgo(2), state: "ok", message: null, openChanged: false, budgets, ...report }, ...sync },
});
const mac: SyncDevice = { id: "mac", name: "Studio Mac", addedAt: "2026-11-01T09:00:00Z", lastSeenAt: minutesAgo(1), current: true };
const phone: SyncDevice = { id: "phone", name: "Pixel 9", addedAt: "2026-11-02T09:00:00Z", lastSeenAt: minutesAgo(5), current: false };
const unnamed: SyncDevice = { id: "old", name: null, addedAt: "2026-11-03T09:00:00Z", lastSeenAt: null, current: false };
const rows = async () => within(await screen.findByRole("list", { name: "Devices" })).getAllByRole("listitem");
const headings = () => screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent!.trim());

beforeEach(() => {
  vi.clearAllMocks();
  status = { email: "person@example.test", credentialStore: "Keychain", sync: { unlocked: false, last: null }, site: "https://urbudget.app" };
  overview = { user: { id: "user", email: "person@example.test", createdAt: "2026-01-01" }, subscription: { status: "active", renewsAt: "2026-12-24T00:00:00Z", endsAt: null }, sync: { active: true, budgets: 1, bytes: 1 } };
  lists = [[mac, phone, unnamed]];
  pairing = { waiting: false, collected: true };
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "account_overview") return overview;
    if (command === "sync_key_info") return { exists: false, secret: null, recovery: false, protocol: 2 };
    if (command === "sync_devices") {
      const answer = lists.length > 1 ? lists.shift()! : lists[0];
      if (answer instanceof Error) throw answer.message;
      return { devices: answer };
    }
    if (command === "sync_pair_offer") return { id: "offer", code: "0123-4567-89AB-CDEF-GHJK-MNPQ", qr: { size: 3, modules: "101010101" }, expiresAt: new Date(Date.now() + 5 * 60_000).toISOString() };
    if (command === "sync_pair_state") return pairing;
    if (command === "sync_pair_cancel" || command === "account_open") return null;
    throw new Error(`Unexpected native command: ${command}`);
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it("lists the devices that sync: this computer, when the others last synced, and one with no name", async () => {
  status = syncing();
  render(view(state()));
  const [first, second, third] = await rows();
  expect(within(first).getByText("Studio Mac")).toBeTruthy();
  expect(within(first).getByText("This computer")).toBeTruthy();
  // The device in hand says which it is, not when it synced.
  expect(within(first).queryByText(/Last synced/)).toBeNull();
  expect(within(second).getByText("Pixel 9")).toBeTruthy();
  expect(within(second).getByText("Last synced 5 min ago")).toBeTruthy();
  expect(within(third).getByText(/^A device added on .*November/)).toBeTruthy();
  // When it last synced isn't known for a device that joined before the account kept that: nothing is said.
  expect(within(third).queryByText(/synced/)).toBeNull();
  // The list only shows: nothing on a row removes a device, and the line under it says what does.
  expect(within(screen.getByRole("list", { name: "Devices" })).queryByRole("button")).toBeNull();
  expect(screen.getByText("To stop a device syncing, sign out on it. To cut off a device you no longer have, reset sync.")).toBeTruthy();
  expect(commands("sync_devices")).toHaveLength(1);
});

it("calls this computer just that until it has a name", async () => {
  status = syncing();
  lists = [[{ ...mac, name: null }]];
  render(view(state()));
  const [only] = await rows();
  expect(only.textContent).toBe("This computer");
});

it("reads the list again once a device is added", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  status = syncing();
  lists = [[mac], [mac, phone]];
  render(view(state()));
  expect(await rows()).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Add a device" }));
  await screen.findByRole("img", { name: "QR code for adding a device" });
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(screen.getByText("Device added.")).toBeTruthy();
  await waitFor(() => expect(screen.getByText("Pixel 9")).toBeTruthy());
  expect(commands("sync_devices")).toHaveLength(2);
});

it("keeps Add a device when the list can't be read, and tries again when asked", async () => {
  status = syncing();
  lists = [new Error("Couldn't reach urbudget.app. Check your internet connection and try again."), [mac]];
  render(view(state()));
  expect(await screen.findByText("Couldn't reach urbudget.app. Check your internet connection and try again.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Add a device" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await rows()).toHaveLength(1);
});

it("asks for no devices until sync is on here", async () => {
  // Sync is off for the account: there's one thing to do, and nothing below it.
  render(view(state()));
  expect(await screen.findByRole("button", { name: "Turn on sync" })).toBeTruthy();
  expect(headings()).toEqual(["Account", "Sync"]);
  expect(screen.queryByRole("button", { name: "Add a device" })).toBeNull();
  expect(commands("sync_devices")).toEqual([]);
});

it("lists only the budgets that need someone, and counts the ones that are simply syncing", async () => {
  status = syncing(
    [
      budget("a", "synced"),
      budget("b", "uploaded"),
      budget("c", "downloaded"),
      budget("kept", "local_only"),
      budget("copy", "recovery", { message: "The account's encryption key changed. Recover this preserved local budget as a new copy." }),
      budget("failed", "error", { message: "Couldn't read this budget's file." }),
    ],
    {},
    { state: "attention" },
  );
  render(view(state()));
  expect(await screen.findByText("3 budgets are syncing.")).toBeTruthy();
  for (const id of ["a", "b", "c"]) expect(screen.queryByText(`Budget ${id}`)).toBeNull();
  expect(screen.queryByText("Synced")).toBeNull();
  for (const id of ["kept", "copy", "failed"]) expect(screen.getByText(`Budget ${id}`)).toBeTruthy();
  // One kept on this computer has its button, and a kept copy leads to where it's recovered.
  expect(screen.getByRole("button", { name: "Sync this budget" })).toBeTruthy();
  expect(screen.getByText(/Couldn't read this budget's file\./)).toBeTruthy();
  // Sync's one line says how many need someone.
  expect(screen.getByText("Sync is on. 3 budgets need you.")).toBeTruthy();
  // The count leads to the budget library, where every budget is.
  const links = screen.getAllByRole("button", { name: "Manage budgets…" });
  expect(links).toHaveLength(2);
  fireEvent.click(links[1]);
  expect(manageBudgets).toHaveBeenCalledOnce();
});

it("takes no room for budgets or a plan with nothing to say, and keeps what few need under More", async () => {
  status = syncing([]);
  render(view(state()));
  expect(await screen.findByText(/^Sync is on\. Last synced 2 min ago\.$/)).toBeTruthy();
  expect(headings()).toEqual(["Account", "Sync", "Devices"]);
  // A plan in good standing takes no room: its renewal is under More, with the rest few need.
  const more = screen.getByText("More").closest("details")!;
  expect(more.open).toBe(false);
  expect(within(more).getByText(/^Renews on /)).toBeTruthy();
  for (const name of ["New recovery code…", "Lock on this computer", "Reset sync…", "Manage account", "Privacy policy", "Delete account"]) expect(within(more).getByRole("button", { name })).toBeTruthy();
  // Sign out is on the account's row.
  expect(within(more).queryByRole("button", { name: "Sign out" })).toBeNull();
  expect(screen.getByRole("button", { name: "Sign out" })).toBeTruthy();
});

it("keeps a budget that says something in the list, though it syncs", async () => {
  status = syncing([budget("merged", "merged", { notes: ["Assignments made on both devices were added together."] }), budget("plain", "merged")]);
  render(view(state()));
  expect(await screen.findByText("Assignments made on both devices were added together.")).toBeTruthy();
  expect(screen.getByText("Budget merged")).toBeTruthy();
  expect(screen.queryByText("Budget plain")).toBeNull();
  expect(screen.getByText("1 budget is syncing.")).toBeTruthy();
});

it("shows the plan only when there's something to do or to know about it", async () => {
  status = syncing();
  overview = { ...overview, subscription: { status: "active", renewsAt: null, endsAt: "2026-12-24T00:00:00Z" } };
  const ending = render(view(state()));
  expect(await screen.findByText(/^Your plan ends on /)).toBeTruthy();
  expect(headings()).toEqual(["Account", "Plan", "Sync", "Devices"]);
  ending.unmount();
  overview = { ...overview, subscription: { status: "past_due", renewsAt: "2026-12-24T00:00:00Z", endsAt: null } };
  render(view(state()));
  expect(await screen.findByText(/^The last payment didn't go through/)).toBeTruthy();
  // Its one action is beside it, where the card is updated.
  fireEvent.click(screen.getAllByRole("button", { name: "Manage account" })[0]);
  expect(commands("account_open")).toEqual([["account_open", { next: "/account/" }]]);
});

it("says in sync's one line what it's waiting for", async () => {
  status = syncing([], {}, { state: "offline" });
  const offline = render(view(state()));
  expect(await screen.findByText("Offline. Sync picks up when you're back online.")).toBeTruthy();
  expect(screen.queryByText(/Last synced .* ago\.$/)).toBeNull();
  expect(screen.getByRole("button", { name: "Sync now" })).toBeTruthy();
  offline.unmount();
  status = syncing([budget("a", "queued")], {}, { state: "waiting" });
  render(view(state()));
  expect(await screen.findByText("Sync is on. Changes are waiting to sync.")).toBeTruthy();
});

it("opens the budgets that are simply syncing on the older sync, where each is stopped by hand", async () => {
  status = syncing([budget("a", "synced"), budget("b", "synced")], { protocol: 1, secret: "passphrase" });
  lists = [new Error("Not available for this account.")];
  render(view(state()));
  const count = await screen.findByText("2 budgets are syncing.");
  const fold = count.closest("details")!;
  expect(fold.open).toBe(false);
  expect(within(fold).getAllByRole("button", { name: "Stop syncing" })).toHaveLength(2);
  // An account with no list of devices says nothing about one.
  expect(screen.queryByText("Not available for this account.")).toBeNull();
  expect(screen.getByRole("button", { name: "Add a device" })).toBeTruthy();
});
