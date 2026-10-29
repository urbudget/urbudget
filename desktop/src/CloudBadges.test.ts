import { describe, expect, it } from "vitest";
import { syncLabel } from "./CloudBadges";
import type { SyncReport, BudgetSync } from "./cloud";
const report = (...states: BudgetSync["state"][]): SyncReport => ({ at: "2026-10-28", state: "ok", message: null, openChanged: false, budgets: states.map((state, i) => ({ id: String(i), name: "Budget", state, device: null, updatedAt: null, message: null })) });
describe("truthful sync status", () => {
  it("does not label local-only or queued budgets synced", () => {
    expect(syncLabel(report("local_only"), "0")).toBe("Saved on this device");
    expect(syncLabel(report("waiting"), "0")).toBe("Waiting to sync");
    expect(syncLabel(report("synced"), "0", true)).toBe("Waiting to sync");
  });
  it("renders durable v2 waiting and paused phases", () => {
    expect(syncLabel({...report("queued"),state:"waiting"},"0")).toBe("Waiting to sync");
    expect(syncLabel(report("paused"),"0")).toBe("Sync paused");
    expect(syncLabel({...report("recovery"),state:"attention"},"0")).toBe("Sync needs you");
  });
  it("prioritizes deletion review over queued changes", () => {
    expect(syncLabel(report("queued", "deletion_conflict"), "0")).toBe("Sync needs you");
  });
  it("keeps conflicts elsewhere visible and no-report status unconfirmed", () => {
    expect(syncLabel(report("synced", "conflict"), "0")).toBe("Sync needs you");
    expect(syncLabel(undefined, "0")).toBe("Sync");
    expect(syncLabel(report("synced"), "0")).toBe("Synced");
  });
});
