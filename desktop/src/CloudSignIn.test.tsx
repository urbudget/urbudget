import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SyncSettings } from "./CloudSettings";
import { useCloud, type Cloud, type CloudStatus, type Overview } from "./cloud";
import { ToastProvider } from "./ui";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("./bridge", () => ({ desktop: true, api: { library: async () => ({version:"1",activeId:null,entries:[]}) } }));
let status: CloudStatus;
let overview: Overview;
let start: { ok: boolean; reviewAccess?: boolean; devCode?: string };
const commands = (command: string) => native.invoke.mock.calls.filter(([name]) => name === command);
const state = (): Cloud => ({ status, setStatus: vi.fn(), refresh: vi.fn(async () => status), syncNow: vi.fn(async () => null), syncing: false });
const syncView = (model: Cloud) => <ToastProvider><SyncSettings cloud={model} budgetId="test" isClean={() => true} openBudget={vi.fn()} /></ToastProvider>;

beforeEach(() => {
  vi.clearAllMocks();
  status = {
    email: "person@example.test", credentialStore: "Keychain", sync: { unlocked: false, last: null }, site: "https://urbudget.app",
  };
  overview = { user: { id: "user", email: status.email!, createdAt: "2026-01-01" }, subscription: null, sync: { active: false, budgets: 0, bytes: 0 } };
  start = { ok: true };
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "cloud_status") return status;
    if (command === "account_overview") return overview;
    if (command === "account_start") return start;
    if (command === "account_verify") return status;
    if (command === "sync_has_key") return true;
    throw new Error(`Unexpected native command: ${command}`);
  });
});
afterEach(cleanup);

it("renders lifecycle states with no open budget, including a newer native state", async () => {
  overview = { ...overview, sync: { ...overview.sync, active: true } };
  const phases = ["trashed", "purged", "recovery", "queued", "signed_out", "evicted", "locked", "paused", "subscription_required", "future_native_state"];
  status = { ...status, sync: { protocol: 2, unlocked: true, last: {
    at: "2026-10-29T00:00:00Z", state: "ok", message: null, openChanged: false,
    // Simulate the native JSON boundary, where older frontends may see newer values.
    budgets: phases.map(phase => ({ id: phase, name: `Budget ${phase}`, state: phase, device: null, updatedAt: null, message: null })) as NonNullable<CloudStatus["sync"]["last"]>["budgets"],
  } } };
  render(<ToastProvider><SyncSettings cloud={state()} budgetId="" isClean={() => true} openBudget={vi.fn()} /></ToastProvider>);
  expect(await screen.findByText("In Trash")).toBeTruthy();
  expect(screen.getByText("Permanently deleted")).toBeTruthy();
  expect(screen.getByText("Recovered local changes")).toBeTruthy();
  expect(screen.getByText("Available online")).toBeTruthy();
  expect(screen.getByText("Sync needs you")).toBeTruthy();
  for (const phase of phases) expect(screen.getByText(`Budget ${phase}`)).toBeTruthy();
});

it("only loads the account's status when the window opens", async () => {
  status = { ...status, email: null };
  const hook = renderHook(() => useCloud({ dirty: () => false, sync: "saved", operation: "", token: "", fetchDatabase: vi.fn() }));
  await waitFor(() => expect(hook.result.current.status).toMatchObject(status));
  expect(native.invoke.mock.calls.map(([name]) => name)).toEqual(["cloud_status"]);
});

it("discards an old account's overview after the signed-in account changes", async () => {
  let finish!: (value: Overview) => void;
  native.invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const model = state();
  const view = render(syncView(model));
  status = { ...status, email: "second@example.test" };
  view.rerender(syncView({ ...model, status }));
  await screen.findByText("$3 a month");
  await act(async () => finish({ ...overview, sync: { ...overview.sync, active: true } }));
  expect(screen.getByRole("button", { name: "Subscribe" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Unlock sync" })).toBeNull();
});

it("shows account errors with a retry", async () => {
  native.invoke.mockImplementationOnce(async () => { throw new Error("Account service unavailable"); });
  render(syncView(state()));
  await screen.findByText("Account service unavailable");
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await screen.findByText("$3 a month");
  expect(commands("account_overview")).toHaveLength(2);
});

async function beginSignIn(email: string) {
  fireEvent.change(screen.getByLabelText("Email"), { target: { value: email } });
  fireEvent.click(screen.getByRole("button", { name: "Email me a code" }));
  await waitFor(() => expect(commands("account_start")).toHaveLength(1));
}

it("keeps the requested email fixed until the sign-in response arrives", async () => {
  status = { ...status, email: null };
  let finish!: (value: typeof start) => void;
  native.invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  render(syncView(state()));
  const email = screen.getByLabelText("Email") as HTMLInputElement;
  fireEvent.change(email, { target: { value: "review@example.test" } });
  fireEvent.click(screen.getByRole("button", { name: "Email me a code" }));
  expect(email.disabled).toBe(true);
  expect((screen.getByRole("button", { name: "Email me a code" }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => finish({ ok: true, reviewAccess: true }));
  const input = await screen.findByLabelText(/Review access code/);
  const code = "Synthetic_Review-Code_For_Testing_1234567890";
  fireEvent.change(input, { target: { value: code } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
  await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("account_verify", { email: "review@example.test", code }));
});

it("accepts the supplied review access code without truncating it to an email OTP", async () => {
  start = { ok: true, reviewAccess: true };
  status = { ...status, email: null };
  const model = state();
  render(syncView(model));
  await beginSignIn("review@example.test");
  const input = await screen.findByLabelText(/Review access code/) as HTMLInputElement;
  expect(input.type).toBe("password");
  expect(input.maxLength).toBe(128);
  expect(screen.getByText(/Use the access code supplied with the app review instructions/)).toBeTruthy();
  fireEvent.change(input, { target: { value: "too_short" } });
  expect((screen.getByRole("button", { name: "Sign in" }) as HTMLButtonElement).disabled).toBe(true);
  const code = "Synthetic_Review-Code_For_Testing_1234567890";
  fireEvent.change(input, { target: { value: code } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
  await waitFor(() => expect(model.setStatus).toHaveBeenCalledOnce());
  expect(native.invoke).toHaveBeenCalledWith("account_verify", { email: "review@example.test", code });
});

it("resets review access when changing email and preserves the normal six-digit flow", async () => {
  start = { ok: true, reviewAccess: true };
  status = { ...status, email: null };
  render(syncView(state()));
  await beginSignIn("review@example.test");
  const input = await screen.findByLabelText(/Review access code/);
  fireEvent.change(input, { target: { value: "Synthetic_Review-Code_For_Testing" } });
  fireEvent.click(screen.getByRole("button", { name: "Use another email" }));
  start = { ok: true };
  fireEvent.change(screen.getByLabelText("Email"), { target: { value: "ordinary@example.test" } });
  fireEvent.click(screen.getByRole("button", { name: "Email me a code" }));
  const otp = await screen.findByLabelText(/The 6-digit code we sent/) as HTMLInputElement;
  expect(otp.value).toBe("");
  expect(otp.type).toBe("text");
  expect(otp.inputMode).toBe("numeric");
  fireEvent.change(otp, { target: { value: "a12 34-56789" } });
  expect(otp.value).toBe("123456");
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
  await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("account_verify", { email: "ordinary@example.test", code: "123456" }));
});
