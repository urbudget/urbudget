// The engine hears the resolved language at startup and on every change, and a failure is ignored.
import { afterEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, isTauri: () => true }));
import { applyLanguage, getLanguage } from "./index";
import { setPref } from "../prefs";

afterEach(() => {
  vi.restoreAllMocks();
  setPref("language", "system");
  localStorage.clear();
});

it("tells the engine the resolved language, never System, and ignores a failure", async () => {
  native.invoke.mockRejectedValue(new Error("unknown command set_language"));
  applyLanguage();
  expect(native.invoke).toHaveBeenLastCalledWith("set_language", { language: "en" });

  setPref("language", "ar");
  expect(getLanguage()).toBe("ar");
  expect(native.invoke).toHaveBeenLastCalledWith("set_language", { language: "ar" });

  vi.spyOn(navigator, "languages", "get").mockReturnValue(["ar-EG"]);
  native.invoke.mockClear();
  // System on an Arabic system: still Arabic, so nothing changed and nothing is sent.
  setPref("language", "system");
  expect(native.invoke).not.toHaveBeenCalled();
  setPref("language", "en");
  expect(native.invoke).toHaveBeenLastCalledWith("set_language", { language: "en" });
  for (const [, args] of native.invoke.mock.calls) expect(args).not.toEqual({ language: "system" });
  // The rejected calls were swallowed.
  await Promise.resolve();
});
