import { beforeEach, expect, it, vi } from "vitest";
import type { Budget } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));
const budget: Budget = { schemaVersion: 2, name: "Queue", currency: "USD", accounts: [], categories: [], transactions: [], assignments: [] };
const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
};
beforeEach(() => {
  vi.resetModules();
  invoke.mockReset().mockImplementation(async (command: string) => command === "load_budget" ? { budget } : {});
});

it("drops obsolete queued reads without dispatching them or dropping a later edit/save", async () => {
  const { api, read } = await import("./bridge");
  await api.load();
  const busy = deferred<unknown>();
  invoke.mockImplementation(async (command: string) => command === "busy_read" ? busy.promise : {});
  const first = read(budget, "busy_read");
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("busy_read", { patch: null }));
  const controller = new AbortController();
  const obsolete = read(budget, "obsolete_read", {}, controller.signal).catch((e) => e);
  const edit = api.push({ ...budget, name: "Edited" });
  const save = api.commit("v1");
  controller.abort();
  busy.resolve({});
  await Promise.all([first, edit, save]);
  expect(await obsolete).toMatchObject({ name: "AbortError" });
  expect(invoke.mock.calls.map(([command]) => command)).toEqual(["load_budget", "busy_read", "apply_patch", "commit_budget"]);
});

it("waits for an aborted running read before dispatching a later save", async () => {
  const { api, read } = await import("./bridge");
  await api.load();
  const busy = deferred<unknown>();
  invoke.mockImplementation(async (command: string) => command === "busy_read" ? busy.promise : {});
  const controller = new AbortController();
  const first = read(budget, "busy_read", {}, controller.signal).catch((e) => e);
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("busy_read", { patch: null }));
  controller.abort();
  const save = api.commit("v1");
  await Promise.resolve();
  expect(invoke).not.toHaveBeenCalledWith("commit_budget", expect.anything());
  busy.resolve({});
  expect(await first).toMatchObject({ name: "AbortError" });
  await save;
  expect(invoke).toHaveBeenLastCalledWith("commit_budget", { expectedToken: "v1" });
});

it("keeps the draft synchronized if a read is aborted during initial draft setup", async () => {
  const { api, read } = await import("./bridge");
  const setup = deferred<unknown>();
  invoke.mockImplementation(async (command: string) => command === "replace_draft" ? setup.promise : {});
  const controller = new AbortController();
  const first = read(budget, "obsolete_read", {}, controller.signal).catch((e) => e);
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("replace_draft", { budget }));
  controller.abort();
  setup.resolve({});
  expect(await first).toMatchObject({ name: "AbortError" });
  await api.push(budget);
  expect(invoke.mock.calls.map(([command]) => command)).toEqual(["replace_draft"]);
});

it("passes calculation context as a read argument without changing the draft", async () => {
  const { api } = await import("./bridge");
  await api.load();
  await api.autoAssignResult(budget, "2026-11", "resetAvailable", ["food"], false, { asOfMonth: "2026-10" });
  expect(invoke).toHaveBeenLastCalledWith("preview_auto_assign_result", {
    patch: null, month: "2026-11", mode: "resetAvailable", categories: ["food"], limitToAvailable: false, asOfMonth: "2026-10",
  });
  await api.push(budget);
  expect(invoke.mock.calls.map(([command]) => command)).toEqual(["load_budget", "preview_auto_assign_result"]);
});

it("preserves local-only creation for private budgets, fresh starts and samples", async () => {
  const { api } = await import("./bridge");
  await api.createBudget(budget, undefined, true);
  expect(invoke).toHaveBeenLastCalledWith("create_budget", {budget, localOnly: true});
  await api.load();
  await api.createBudget(budget, {budget, token: "source:1"}, true);
  expect(invoke).toHaveBeenLastCalledWith("create_budget", {budget, localOnly: true, freshStartToken: "source:1", patch: null});
  const sample = {...budget, sample: {from: "source"}};
  await api.createBudget(sample);
  expect(invoke).toHaveBeenLastCalledWith("create_budget", {budget: sample, localOnly: true});
});