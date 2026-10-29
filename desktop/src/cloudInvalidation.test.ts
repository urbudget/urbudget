import { describe, it, expect, vi } from "vitest";
vi.mock("./bridge", () => ({desktop:false, api:{}}));
import { syncInvalidatesActive, syncCanReconcile, syncCadence } from "./cloud";
describe("sync invalidations", () => {
  it("reconciles locked v2 catalogs without enabling legacy locked sync", () => {
    const status = {email:"qa@example.test",sync:{protocol:2,unlocked:false,last:null}};
    expect(syncCanReconcile(status)).toBe(true);
    expect(syncCanReconcile({...status,email:null})).toBe(false);
    expect(syncCanReconcile({...status,sync:{...status.sync,protocol:1}})).toBe(false);
    expect(syncCanReconcile({...status,sync:{...status.sync,protocol:1,unlocked:true}})).toBe(true);
  });
  it("does not apply another identity's legacy openChanged when targeted changes exist", () => {
    expect(syncInvalidatesActive({openChanged:true,changedBudgets:[{id:"former",revision:"next"}]},"current","current-revision","current")).toBe(false);
  });
  it("reloads only a newer active revision and still handles deletion to empty", () => {
    const report = {openChanged:false,changedBudgets:[{id:"current",revision:"next"}]};
    expect(syncInvalidatesActive(report,"current","old","current")).toBe(true);
    expect(syncInvalidatesActive(report,"current","next","current")).toBe(false);
    expect(syncInvalidatesActive({openChanged:false,changedBudgets:[]},"current","next",null)).toBe(true);
  });
  it("falls back to openChanged only when the targeted field is absent", () => {
    expect(syncInvalidatesActive({openChanged:true},"current","old")).toBe(true);
    expect(syncInvalidatesActive({openChanged:true,changedBudgets:[]},"current","old")).toBe(false);
  });
});

it("preserves the legacy automatic network cadence until v2 is enabled", () => {
  expect(syncCadence(1)).toEqual({poll:300000,afterSave:8000,focus:60000});
  expect(syncCadence()).toEqual(syncCadence(1));
  expect(syncCadence(2)).toEqual({poll:30000,afterSave:2000,focus:5000});
});
