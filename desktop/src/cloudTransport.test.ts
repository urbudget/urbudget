import { expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn(async () => ({})));
vi.mock("@tauri-apps/api/core", () => ({invoke}));
vi.mock("./bridge", () => ({desktop:false,api:{}}));
import { cloud } from "./cloud";
it("returns the displayed immutable review identity with each conflict answer", async () => {
  const review = {token:"local-token",version:"remote-version",identity:{accountId:"account",remoteId:"remote",incarnationId:"incarnation",keyGeneration:"2"},conflicts:[{key:"category/food/name",fingerprint:"sha256"}]};
  await cloud.syncResolve("budget","merge",true,{"category/food/name":"cloud"},review);
  expect(invoke).toHaveBeenCalledWith("sync_resolve",{id:"budget",keep:"merge",openClean:true,choices:{"category/food/name":"cloud"},review});
});
