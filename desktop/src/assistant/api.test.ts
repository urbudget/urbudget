import { expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { assistant, type Status } from "./api";

it("passes each status the app is given on to whoever is watching", async () => {
  const changed = { provider: "anthropic", ready: true, noticeDue: true } as Status;
  invoke.mockResolvedValue(changed);
  const heard = vi.fn();
  const stop = assistant.onStatus(heard);
  // Settings changes the service: the open panel hears the status that comes back.
  expect(await assistant.settings({ provider: "anthropic" })).toBe(changed);
  expect(invoke).toHaveBeenLastCalledWith("assistant_settings", { change: { provider: "anthropic" } });
  expect(heard).toHaveBeenCalledWith(changed);
  stop();
  await assistant.status();
  expect(heard).toHaveBeenCalledTimes(1);
});

it("forgets a file taken off a message, and marks the notice of what gets sent as read", async () => {
  invoke.mockResolvedValue(undefined);
  await assistant.detach("file_1");
  expect(invoke).toHaveBeenLastCalledWith("assistant_detach", { id: "file_1" });
  await assistant.settings({ told: true });
  expect(invoke).toHaveBeenLastCalledWith("assistant_settings", { change: { told: true } });
});
