// On Linux a copy installed from the .deb updates through the system's installer, which asks
// for a password: its updates wait for the person. An AppImage updates like any other copy.
import { act, cleanup, render, renderHook, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
const mocked = vi.hoisted(() => ({
  check: vi.fn(),
  invoke: vi.fn(),
  asksPassword: true as boolean | Error,
  steps: [] as string[],
}));
vi.mock("./bridge", () => ({ desktop: true }));
vi.mock("./platform", async (original) => ({ ...(await original<typeof import("./platform")>()), os: "linux" }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocked.check }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocked.invoke,
  Channel: class {
    onmessage = (_message: number) => {};
  },
}));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: async () => "0.4.0" }));
import { useUpdates } from "./updates";
import { UpdateSetting } from "./UpdateNotice";

const saveAndInstall = async (install: () => Promise<void>) => {
  mocked.steps.push("save");
  await install();
};
const update = () => ({
  version: "0.4.1",
  body: "",
  download: vi.fn(async (onEvent: (event: { event: string }) => void) => {
    mocked.steps.push("download");
    onEvent({ event: "Finished" });
  }),
  install: vi.fn(async () => {
    mocked.steps.push("install");
  }),
  close: vi.fn(async () => {}),
});

beforeEach(() => {
  vi.clearAllMocks();
  mocked.steps = [];
  localStorage.clear();
  mocked.invoke.mockImplementation(async (command: string) => {
    if (command === "update_channel") return "direct";
    if (command === "update_asks_password") {
      if (mocked.asksPassword instanceof Error) throw mocked.asksPassword;
      return mocked.asksPassword;
    }
    mocked.steps.push(command);
  });
});
afterEach(cleanup);

async function found() {
  mocked.check.mockResolvedValue(update());
  const hook = renderHook(() => useUpdates());
  await act(async () => {
    await hook.result.current.check(true);
  });
  return hook;
}

describe("updates on Linux", () => {
  it.each([
    ["a copy installed from the .deb", true],
    ["a copy whose kind the app couldn't tell", new Error("no answer")],
  ])("waits for the person on %s", async (_name, answer) => {
    mocked.asksPassword = answer;
    const { result } = await found();
    expect(result.current).toMatchObject({ ready: true, automatic: false, asksPassword: true });
    await act(async () => {
      await result.current.install(saveAndInstall, true);
    });
    expect(mocked.steps).toEqual(["download"]);

    render(<UpdateSetting updates={result.current} version="0.4.0" onInstall={() => {}} />);
    expect(screen.getByText("It installs when you choose. Your budget is saved first.")).toBeTruthy();

    await act(async () => {
      await result.current.install(saveAndInstall);
    });
    expect(mocked.steps).toEqual(["download", "save", "install", "restart_app"]);
  });

  it("installs an AppImage's update by itself", async () => {
    mocked.asksPassword = false;
    const { result } = await found();
    expect(result.current).toMatchObject({ ready: true, automatic: true, asksPassword: false });
    await act(async () => {
      await result.current.install(saveAndInstall, true);
    });
    expect(mocked.steps).toEqual(["download", "save", "install", "restart_app"]);
  });
});
