import { expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { assistant, type Proposal, type Status } from "./api";
import { demoBudget } from "../demo";

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

it("asks for a change without having its card marked applied, and reads a conversation with the window's budget", async () => {
  const saved = demoBudget();
  invoke.mockResolvedValue({ patch: null, proposal: {} });
  // The card is marked once the change is in the budget, by the app: not here.
  await assistant.apply(saved, "t_1", "p_1");
  expect(invoke).toHaveBeenLastCalledWith("assistant_apply", { threadId: "t_1", proposalId: "p_1", settle: false, patch: null });
  // Unsaved edits go with the request, so waiting cards are brought up to date with them too.
  const edited = { ...saved, name: "Edited" };
  await assistant.thread(edited, "t_1");
  expect(invoke).toHaveBeenLastCalledWith("assistant_thread", { id: "t_1", patch: { name: "Edited" } });
  await assistant.notes(edited, ["Rent is due on the 1st"]);
  expect(invoke).toHaveBeenLastCalledWith("assistant_notes", { notes: ["Rent is due on the 1st"], patch: { name: "Edited" } });
});

it("marks a card with what the app has to say about it, and tells whoever is watching", async () => {
  const applied = { id: "p_1", status: "applied" } as Proposal;
  invoke.mockResolvedValue(applied);
  const heard = vi.fn();
  const stop = assistant.onMarked(heard);
  // The window's Undo, an import that finished: the open panel hears of the card as it is now.
  expect(await assistant.mark("t_1", "p_1", "applied", "Fixes applied: 2 of 3. Not applied: Line 30")).toBe(applied);
  expect(invoke).toHaveBeenLastCalledWith("assistant_mark", {
    threadId: "t_1",
    proposalId: "p_1",
    status: "applied",
    note: "Fixes applied: 2 of 3. Not applied: Line 30",
  });
  expect(heard).toHaveBeenCalledWith("t_1", applied);
  // A step the core refuses is an error for whoever asked, and no card to tell of.
  invoke.mockRejectedValueOnce("This proposal was already handled.");
  await expect(assistant.mark("t_1", "p_1", "discarded")).rejects.toBe("This proposal was already handled.");
  expect(heard).toHaveBeenCalledTimes(1);
  stop();
  await assistant.mark("t_1", "p_1", "undone");
  expect(heard).toHaveBeenCalledTimes(1);
});
