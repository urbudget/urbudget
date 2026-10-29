import { cleanup, renderHook, act } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("./bridge", () => ({ desktop: true }));
import { EditorCheckpoint, flushAllEditorDrafts, useEditorCheckpoint } from "./editorCheckpoint";
describe("editor checkpoint ordering", () => {
  it("the exit drain awaits independent leaf editor writes", async () => {
    let release!: () => void;
    invoke.mockReset().mockImplementationOnce(() => new Promise<void>(done => { release = done; }));
    const leaf = new EditorCheckpoint("budget-a", "payeeRule", {}, vi.fn());
    leaf.save({amount:"12 +"});
    let exited = false;
    const drain = flushAllEditorDrafts().then(() => { exited = true; });
    await Promise.resolve(); await Promise.resolve();
    expect(exited).toBe(false);
    release(); await drain;
    expect(exited).toBe(true);
  });
  it("coalesces a burst to the latest durable raw values", async () => {
    invoke.mockReset().mockResolvedValue(undefined);
    const checkpoint = new EditorCheckpoint("budget-a", "transaction", {id:"tx"}, vi.fn());
    for (let i = 0; i < 1000; i++) checkpoint.save({amount: `${i} +`});
    await checkpoint.flush();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke.mock.calls[0][1]).toMatchObject({epoch:"1000",values:{amount:"999 +"}});
  });
  it("persists partial strings and serializes discard behind every pending epoch", async () => {
    let release!: () => void;
    invoke.mockReset().mockImplementationOnce(() => new Promise<void>(done => { release = done; })).mockResolvedValue(undefined);
    const failure = vi.fn();
    const checkpoint = new EditorCheckpoint("budget-a", "transaction", {memo:"before"}, failure);
    checkpoint.save({amount:"12 +",memo:"one"});
    await Promise.resolve(); await Promise.resolve();
    checkpoint.save({amount:"12 +",memo:"two"});
    checkpoint.discard();
    await Promise.resolve(); await Promise.resolve();
    expect(invoke).toHaveBeenCalledTimes(1);
    release(); await checkpoint.flush();
    expect(invoke.mock.calls.map(call => [call[0], call[1].epoch])).toEqual([["editor_checkpoint","1"],["editor_checkpoint","2"],["editor_discard","2"]]);
    expect(invoke.mock.calls[1][1].values.amount).toBe("12 +");
    expect(invoke.mock.calls.every(call => call[1].budgetId === "budget-a")).toBe(true);
    expect(failure).not.toHaveBeenCalled();
  });
});


afterEach(cleanup);
it("clears completed leaf editors on unmount through the native active-identity guard", async () => {
  invoke.mockReset().mockResolvedValue(undefined);
  const {unmount} = renderHook(() => useEditorCheckpoint("budget-a","transaction",{},vi.fn(),{amount:"12 +"}));
  await act(async () => { await flushAllEditorDrafts(); });
  unmount();
  await act(async () => { await Promise.resolve(); await flushAllEditorDrafts(); });
  expect(invoke).toHaveBeenCalledWith("editor_discard",expect.objectContaining({budgetId:"budget-a",onlyIfActive:true}));
});
it("clears a former editor kind but preserves an editor from another identity", async () => {
  invoke.mockReset().mockResolvedValue(undefined);
  const {rerender,unmount} = renderHook(({id,kind}) => useEditorCheckpoint(id,kind,{},vi.fn(),{amount:"12 +"}),{initialProps:{id:"a",kind:"transaction"}});
  await act(async () => { await flushAllEditorDrafts(); });
  rerender({id:"a",kind:"category"});
  await act(async () => { await flushAllEditorDrafts(); });
  expect(invoke.mock.calls.filter(c => c[0] === "editor_discard")).toHaveLength(1);
  invoke.mockClear();
  rerender({id:"b",kind:"category"});
  await act(async () => { await flushAllEditorDrafts(); });
  expect(invoke.mock.calls.filter(c => c[0] === "editor_discard")).toHaveLength(0);
  unmount();
});
it("does not discard a live StrictMode editor or lose a failed checkpoint on teardown", async () => {
  invoke.mockReset().mockResolvedValue(undefined);
  const {unmount} = renderHook(() => useEditorCheckpoint("a","transaction",{},vi.fn(),{amount:"12 +"}),{wrapper:StrictMode});
  await act(async () => { await flushAllEditorDrafts(); });
  expect(invoke.mock.calls.filter(c => c[0] === "editor_discard")).toHaveLength(0);
  unmount();
  await act(async () => { await Promise.resolve(); await flushAllEditorDrafts(); });
  invoke.mockReset().mockRejectedValue(new Error("disk full"));
  const failed = vi.fn();
  const result = renderHook(() => useEditorCheckpoint("a","transaction",{},failed,{amount:"12 +"}));
  await act(async () => { await flushAllEditorDrafts(); });
  result.unmount();
  await act(async () => { await Promise.resolve(); await flushAllEditorDrafts(); });
  expect(failed).toHaveBeenCalled();
  expect(invoke.mock.calls.filter(c => c[0] === "editor_discard")).toHaveLength(0);
});
