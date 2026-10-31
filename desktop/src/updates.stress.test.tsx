import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocked = vi.hoisted(() => ({ check: vi.fn(), invoke: vi.fn(), version: vi.fn() }));
vi.mock("./bridge", () => ({ desktop: true }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocked.check }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocked.invoke,
  Channel: class { onmessage = (_message: unknown) => {}; },
}));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: mocked.version }));

import { useUpdates, type StoreInstallStatus } from "./updates";
import { setPref } from "./prefs";
import { UpdateInstalling } from "./UpdateNotice";

const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const packageUpdate = () => ({
  version: "1.0.11",
  body: "Update fixes",
  download: vi.fn(async (..._args: unknown[]) => {}),
  install: vi.fn(async () => {}),
  close: vi.fn(async () => {}),
});
const save = async (install: () => Promise<void>) => install();
const calls = (command: string) => mocked.invoke.mock.calls.filter(([name]) => name === command);

beforeEach(() => {
  vi.resetAllMocks();
  localStorage.clear();
  mocked.version.mockResolvedValue("1.0.10");
  mocked.invoke.mockImplementation(async (command: string) => {
    if (command === "update_channel") return "direct";
    return undefined;
  });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

async function prepared(channel: "direct" | "microsoftStore", download = "ready") {
  const update = packageUpdate();
  mocked.check.mockResolvedValue(update);
  mocked.invoke.mockImplementation(async (command: string) => {
    if (command === "update_channel") return channel;
    if (command === "check_store_update") return true;
    if (command === "download_store_update") return download;
    if (command === "apply_store_update") return "installed";
    if (command === "install_store_update") return true;
    return undefined;
  });
  const hook = renderHook(() => useUpdates());
  await act(async () => { await hook.result.current.check(); });
  return { ...hook, update };
}

describe("updater failure and concurrency stress", () => {
  it.each(["direct", "microsoftStore"] as const)("rechecks the automatic-install preference after a pending save (%s)", async (channel) => {
    const { result, update } = await prepared(channel);
    const saving = deferred<void>();
    let installing!: Promise<void>;
    await act(async () => {
      installing = result.current.install(async (install) => { await saving.promise; await install(); }, true);
    });
    setPref("autoUpdate", false);
    await act(async () => { saving.resolve(); await installing; });
    expect(update.install).not.toHaveBeenCalled();
    expect(calls("apply_store_update")).toHaveLength(0);
    expect(calls("install_store_update")).toHaveLength(0);
    expect(calls("restart_app")).toHaveLength(0);
    expect(result.current.phase).toBe("idle");
    // The same package remains available to someone choosing to install it.
    await act(async () => { await result.current.install(save); });
    expect(calls("restart_app")).toHaveLength(1);
  });

  it.each(["direct", "microsoftStore"] as const)("enforces automatic retry suppression at the install entry point (%s)", async (channel) => {
    localStorage.setItem("lb:update-attempt", JSON.stringify({ from: "1.0.10", to: channel === "direct" ? "1.0.11" : "store", at: Date.now() }));
    const { result, update } = await prepared(channel);
    expect(result.current.automatic).toBe(false);
    const saving = vi.fn(save);
    await act(async () => { await result.current.install(saving, true); });
    expect(saving).not.toHaveBeenCalled();
    expect(update.install).not.toHaveBeenCalled();
    expect(calls("apply_store_update")).toHaveLength(0);
    expect(calls("restart_app")).toHaveLength(0);
  });

  it("never falls back to a Store consent prompt from a stale automatic callback", async () => {
    const { result } = await prepared("microsoftStore", "consent");
    await act(async () => { await result.current.install(save, true); });
    expect(calls("install_store_update")).toHaveLength(0);
    expect(calls("restart_app")).toHaveLength(0);
  });

  it("rechecks preference and retry suppression before installing on exit", async () => {
    const { result } = await prepared("microsoftStore");
    setPref("autoUpdate", false);
    expect(await result.current.installOnExit()).toBe(false);
    expect(calls("apply_store_update")).toHaveLength(0);
    setPref("autoUpdate", true);
    localStorage.setItem("lb:update-attempt", JSON.stringify({ from: "1.0.10", to: "store", at: Date.now() }));
    expect(await result.current.installOnExit()).toBe(false);
    expect(calls("apply_store_update")).toHaveLength(0);
  });

  it("bounds the native standalone download request and recovers after it rejects", async () => {
    const update = packageUpdate();
    update.download.mockRejectedValueOnce(new Error("Request timed out"));
    mocked.check.mockResolvedValue(update);
    const { result } = renderHook(() => useUpdates());
    await act(async () => { await result.current.check(true); });
    expect(update.download.mock.calls[0][1]).toEqual({ timeout: expect.any(Number) });
    const options = update.download.mock.calls[0][1] as { timeout: number };
    expect(options.timeout).toBeGreaterThan(0);
    expect(options.timeout).toBeLessThanOrEqual(30 * 60_000);
    expect(result.current).toMatchObject({ phase: "idle", ready: false, error: "Request timed out" });
    await act(async () => { await result.current.install(save); });
    expect(update.download).toHaveBeenCalledTimes(2);
    expect(update.install).toHaveBeenCalledOnce();
  });

  it("serializes bursts of checks, installs and closes around a slow Store download", async () => {
    const downloading = deferred<string>();
    const installing = deferred<string>();
    mocked.invoke.mockImplementation(async (command: string) => {
      if (command === "update_channel") return "microsoftStore";
      if (command === "check_store_update") return true;
      if (command === "download_store_update") return downloading.promise;
      if (command === "apply_store_update") return installing.promise;
    });
    const { result } = renderHook(() => useUpdates());
    let checking!: Promise<void>;
    await act(async () => { checking = result.current.check(); });
    const saveOnce = vi.fn(save);
    await act(async () => {
      await Promise.all(Array.from({ length: 40 }, (_, i) =>
        i % 3 === 0 ? result.current.check(true) : i % 3 === 1 ? result.current.install(saveOnce) : result.current.installOnExit()));
    });
    expect(calls("check_store_update")).toHaveLength(1);
    expect(calls("download_store_update")).toHaveLength(1);
    expect(saveOnce).not.toHaveBeenCalled();
    await act(async () => { downloading.resolve("ready"); await checking; });
    let installed!: Promise<void>;
    await act(async () => { installed = result.current.install(saveOnce); });
    await act(async () => {
      await Promise.all(Array.from({ length: 40 }, () => result.current.install(saveOnce)));
      await result.current.check(true);
      expect(await result.current.installOnExit()).toBe(false);
    });
    expect(saveOnce).toHaveBeenCalledOnce();
    expect(calls("apply_store_update")).toHaveLength(1);
    await act(async () => { installing.resolve("installed"); await installed; });
    expect(calls("restart_app")).toHaveLength(1);
  });

  it("releases a standalone update discovered after its component unmounts", async () => {
    const checking = deferred<ReturnType<typeof packageUpdate>>();
    const update = packageUpdate();
    mocked.check.mockReturnValue(checking.promise);
    const { result, unmount } = renderHook(() => useUpdates());
    let checked!: Promise<void>;
    await act(async () => { checked = result.current.check(); });
    unmount();
    await act(async () => { checking.resolve(update); await checked; });
    expect(update.download).not.toHaveBeenCalled();
    expect(update.close).toHaveBeenCalledOnce();
  });

  it("releases a downloaded standalone resource after unmount without installing it", async () => {
    const downloading = deferred<void>();
    const update = packageUpdate();
    update.download.mockReturnValue(downloading.promise);
    mocked.check.mockResolvedValue(update);
    const { result, unmount } = renderHook(() => useUpdates());
    let checked!: Promise<void>;
    await act(async () => { checked = result.current.check(); });
    unmount();
    await act(async () => { downloading.resolve(); await checked; });
    expect(update.close).toHaveBeenCalledOnce();
    expect(update.install).not.toHaveBeenCalled();
    expect(calls("restart_app")).toHaveLength(0);
  });

  it("never treats transferred bytes as ready when signature verification fails", async () => {
    const update = packageUpdate();
    update.download.mockImplementation(async (event) => {
      const report = event as (value: { event: string; data?: { contentLength?: number; chunkLength?: number } }) => void;
      report({ event: "Started", data: { contentLength: 100 } });
      report({ event: "Progress", data: { chunkLength: 100 } });
      report({ event: "Finished" });
      throw new Error("Signature verification failed");
    });
    mocked.check.mockResolvedValue(update);
    const { result } = renderHook(() => useUpdates());
    await act(async () => { await result.current.check(true); });
    expect(result.current).toMatchObject({ phase: "idle", ready: false, automatic: false, progress: null });
    expect(result.current.error).toContain("Signature verification failed");
    await act(async () => { await expect(result.current.install(save)).rejects.toThrow("Signature verification failed"); });
    expect(update.install).not.toHaveBeenCalled();
    expect(calls("restart_app")).toHaveLength(0);
  });

  it("keeps the budget locked until native install cancellation completes and exposes safe recovery", async () => {
    vi.useFakeTimers();
    const nativeInstall = deferred<boolean>();
    let report!: (status: StoreInstallStatus) => void;
    mocked.invoke.mockImplementation(async (command: string, args?: { onStatus?: { onmessage: typeof report } }) => {
      if (command === "update_channel") return "microsoftStore";
      if (command === "check_store_update") return true;
      if (command === "download_store_update") return "consent";
      if (command === "install_store_update") {
        report = args!.onStatus!.onmessage;
        return nativeInstall.promise;
      }
    });
    const { result } = renderHook(() => useUpdates());
    await act(async () => { await result.current.check(); });
    let locked = false;
    let installation!: Promise<void>;
    await act(async () => {
      installation = result.current.install(async (install) => {
        locked = true;
        try { await install(); } finally { locked = false; }
      }).catch(() => {});
    });
    const onClose = vi.fn();
    const view = render(<UpdateInstalling updates={result.current} onClose={onClose} />);
    expect(screen.queryByRole("button", { name: "Close UrBudget" })).toBeNull();
    act(() => report({ stage: "installing", progress: 0.6 }));
    view.rerender(<UpdateInstalling updates={result.current} onClose={onClose} />);
    expect(screen.getByText("Installing 60%")).toBeTruthy();
    act(() => report({ stage: "stopping", progress: null }));
    act(() => report({ stage: "installing", progress: 0.8 }));
    expect(result.current.installStatus?.stage).toBe("stopping");
    await act(async () => { await vi.advanceTimersByTimeAsync(60 * 60_000); });
    expect(locked).toBe(true);
    expect(result.current.phase).toBe("installing");
    view.rerender(<UpdateInstalling updates={result.current} onClose={onClose} />);
    expect(screen.getByRole("status").textContent).toContain("Your budget is saved");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Open Microsoft Store" })); });
    expect(calls("open_update_store")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Close UrBudget" }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(locked).toBe(true);
    await act(async () => { nativeInstall.reject(new Error("Update canceled")); await installation; });
    expect(locked).toBe(false);
    expect(result.current).toMatchObject({ phase: "idle", installStatus: null, error: "Update canceled" });
    act(() => report({ stage: "installing", progress: 1 }));
    expect(result.current.installStatus).toBeNull();
    expect(calls("restart_app")).toHaveLength(0);
  });
});
