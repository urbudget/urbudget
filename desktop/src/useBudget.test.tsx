import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Budget, Summary } from "./types";
const mocked = vi.hoisted(() => ({
  load: vi.fn(),
  push: vi.fn(),
  commit: vi.fn(),
  status: vi.fn(),
  calculate: vi.fn(),
  validate: vi.fn(),
  destroy: vi.fn(),
  onCloseRequested: vi.fn(),
  recoveryRead: vi.fn(),
  discardRecovery: vi.fn(),
  runSchedules: vi.fn(),
  openBudget: vi.fn(),
  libraryAction: vi.fn(),
  backup: vi.fn(),
  pruneBackups: vi.fn(),
  restoreBackup: vi.fn(),
}));
vi.mock("./bridge", () => ({ desktop: true, api: mocked }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: mocked.onCloseRequested,
    destroy: mocked.destroy,
  }),
}));
import { AppliedChangeError, useBudget } from "./useBudget";

const budget: Budget = {
  schemaVersion: 1,
  name: "Original",
  currency: "SAR",
  accounts: [],
  categories: [],
  transactions: [],
  assignments: [],
};
const status = {
  token: "0:1",
  savedAt: "",
  databasePath: "/test/budget.sqlite3",
};
const summary: Summary = {
  readyToAssign: 0,
  assigned: 0,
  activity: 0,
  available: 0,
  income: 0,
  netWorth: 0,
  accounts: [],
  categories: [],
};
const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
};
async function start() {
  const hook = renderHook(() => useBudget("2026-10"));
  await act(async () => {});
  return hook;
}
const lastPushed = () => mocked.push.mock.calls.at(-1)![0] as Budget;
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-20T12:00:00"));
  localStorage.clear();
  vi.clearAllMocks();
  mocked.onCloseRequested.mockResolvedValue(() => {});
  mocked.recoveryRead.mockResolvedValue(null);
  mocked.discardRecovery.mockResolvedValue(undefined);
  mocked.load.mockResolvedValue({ ...status, budget: structuredClone(budget) });
  mocked.push.mockResolvedValue({});
  mocked.status.mockResolvedValue(status);
  mocked.calculate.mockResolvedValue(summary);
  mocked.validate.mockResolvedValue(undefined);
  mocked.commit.mockResolvedValue({ ...status, token: "1:1" });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("calculation identity and calendar rollover", () => {
  it("refreshes an unchanged viewed month at the local month boundary", async () => {
    vi.setSystemTime(new Date("2026-10-31T23:59:50"));
    const future = { ...summary, categories: [{ id: "food", assigned: 0, activity: 0, available: 5000, carried: 5000, needed: 10000 }] };
    mocked.calculate.mockResolvedValueOnce(future).mockResolvedValue({ ...future, categories: [{ ...future.categories[0], needed: 5000 }] });
    const { result } = renderHook(() => useBudget("2026-11"));
    await act(async () => {});
    expect(result.current.summary?.categories[0].needed).toBe(10000);
    await act(async () => { await vi.advanceTimersByTimeAsync(10000); });
    expect(mocked.calculate).toHaveBeenCalledTimes(2);
    expect(mocked.calculate.mock.calls.at(-1)?.slice(1)).toEqual(["2026-11", expect.objectContaining({ asOfMonth: "2026-11" })]);
    expect(result.current.summary?.categories[0].needed).toBe(5000);
  });

  it.each(["focus", "visibilitychange"])("refreshes on %s after a suspended app crosses a month boundary", async (event) => {
    const { result } = await start();
    vi.setSystemTime(new Date("2026-11-01T09:00:00"));
    await act(async () => { (event === "focus" ? window : document).dispatchEvent(new Event(event)); });
    expect(result.current.calculationMonth).toBe("2026-11");
    expect(mocked.calculate).toHaveBeenCalledTimes(2);
    // Viewing October is intentional: the clock must not navigate the user's plan.
    expect(mocked.calculate.mock.calls.at(-1)?.[1]).toBe("2026-10");
  });

  it("hides a mismatched month's summary on failure and retries calculation independently of saves", async () => {
    const original = { ...summary, readyToAssign: 10000 };
    mocked.calculate.mockResolvedValueOnce(original).mockRejectedValueOnce(new Error("Calculation unavailable"));
    const hook = renderHook(({ month }) => useBudget(month), { initialProps: { month: "2026-10" } });
    await act(async () => {});
    await act(async () => hook.rerender({ month: "2026-11" }));
    expect(hook.result.current.summary).toBeNull();
    expect(hook.result.current.displaySummary).toBeNull();
    expect(hook.result.current.calculationError).toContain("Calculation unavailable");
    expect(hook.result.current.sync).toBe("saved");
    expect(hook.result.current.error).toBe("");
    act(() => hook.result.current.dismissError());
    expect(hook.result.current.calculationError).toContain("Calculation unavailable");
    await act(async () => hook.result.current.retryCalculation());
    expect(hook.result.current.summary).toBe(summary);
    expect(hook.result.current.calculationError).toBe("");
    expect(mocked.commit).not.toHaveBeenCalled();
  });

  it("keeps only a same-month inert display while the current draft calculates and ignores obsolete results", async () => {
    const { result } = await start();
    const obsolete = deferred<Summary>();
    const latest = deferred<Summary>();
    mocked.calculate.mockReturnValueOnce(obsolete.promise).mockReturnValueOnce(latest.promise);
    act(() => result.current.edit((b) => { b.name = "First edit"; }));
    expect(result.current.summary).toBeNull();
    expect(result.current.displaySummary).toBe(summary);
    const oldSignal = mocked.calculate.mock.calls.at(-1)?.[2].signal as AbortSignal;
    act(() => result.current.edit((b) => { b.name = "Latest edit"; }));
    expect(oldSignal.aborted).toBe(true);
    await act(async () => obsolete.resolve({ ...summary, readyToAssign: 999 }));
    expect(result.current.summary).toBeNull();
    await act(async () => latest.resolve({ ...summary, readyToAssign: 123 }));
    expect(result.current.summary?.readyToAssign).toBe(123);
  });

  it("recovers a deterministically rejected draft by undoing the invalid edit", async () => {
    mocked.calculate.mockImplementation(async (b: Budget) => {
      if (b.name === "Invalid edit") throw new Error("Invalid target amount");
      return summary;
    });
    const { result } = await start();
    await act(async () => result.current.edit((b) => { b.name = "Invalid edit"; }));
    expect(result.current.summary).toBeNull();
    expect(result.current.calculationError).toContain("Invalid target amount");
    await act(async () => result.current.undo());
    expect(result.current.summary).toBe(summary);
    expect(result.current.calculationError).toBe("");
    expect(result.current.budget?.name).toBe("Original");
  });
});

describe("autosave and recovery", () => {
  it("retries persistence of an applied money move without applying it again", async () => {
    const initial = { ...budget, assignments: [
      { month: "2026-10", categoryId: "food", amount: 10000 },
      { month: "2026-10", categoryId: "travel", amount: 0 },
    ] };
    mocked.load.mockResolvedValue({ ...status, budget: initial });
    mocked.commit.mockRejectedValueOnce(new Error("Disk unavailable")).mockResolvedValue({ ...status, token: "1:1" });
    const { result } = await start();
    const move = vi.fn(async (b: Budget) => ({ ...b, assignments: b.assignments.map(a => ({
      ...a, amount: a.amount + (a.categoryId === "food" ? -1000 : 1000),
    })) }));
    let failure: unknown;
    await act(async () => { try { await result.current.transform(move); } catch (e) { failure = e; } });
    expect(failure).toBeInstanceOf(AppliedChangeError);
    expect(result.current.budget!.assignments.map(a => a.amount)).toEqual([9000, 1000]);
    await act(async () => { await (failure as AppliedChangeError).retrySave(); });
    expect(move).toHaveBeenCalledTimes(1);
    expect(mocked.commit).toHaveBeenCalledTimes(2);
    expect(result.current.sync).toBe("saved");
    expect(result.current.budget!.assignments.map(a => a.amount)).toEqual([9000, 1000]);
  });
  it("saves outstanding edits before restoring and accepts the replacement without stale extras", async () => {
    const { result } = await start();
    act(() => result.current.edit((draft) => {
      draft.name = "Before restoring";
      draft.schedules = [];
      draft.ynabPlanId = "old-import";
    }));
    const replacement = { ...budget, name: "Portable backup" };
    mocked.restoreBackup.mockResolvedValue({ ...status, token: "2:1", budget: replacement });
    await act(async () => result.current.restoreBackup(replacement));
    expect(mocked.restoreBackup).toHaveBeenCalledWith(replacement, "1:1");
    expect(mocked.restoreBackup.mock.invocationCallOrder[0]).toBeGreaterThan(mocked.commit.mock.invocationCallOrder[0]);
    expect(result.current.budget).toEqual(replacement);
    expect(result.current.canUndo).toBe(false);
    expect(result.current.dirty()).toBe(false);
  });
  it("keeps the current budget when native restore rejects a stale or invalid backup", async () => {
    const { result } = await start();
    mocked.restoreBackup.mockRejectedValue(new Error("Invalid backup"));
    await act(async () => {
      await expect(result.current.restoreBackup({ ...budget, name: "Rejected" })).rejects.toThrow("Invalid backup");
    });
    expect(result.current.budget?.name).toBe("Original");
    expect(result.current.operation).toBe("");
  });
  it("does not report a successful save when an idle database check fails", async () => {
    const { result } = await start();
    mocked.status.mockRejectedValueOnce("Database unavailable");
    await act(async () => {
      await result.current.flush();
    });
    expect(result.current.sync).toBe("error");
  });
  it("preserves an open entry form when an outside writer changes the database", async () => {
    const { result } = renderHook(() => useBudget("2026-10", true));
    await act(async () => {});
    mocked.status.mockResolvedValue({ ...status, token: "1:2" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(result.current.sync).toBe("conflict");
    expect(mocked.load).toHaveBeenCalledTimes(1);
  });
  it("sends each edit as a draft and only reports saved after the database confirms", async () => {
    const response = deferred<typeof status>();
    mocked.commit.mockReturnValue(response.promise);
    const { result } = await start();
    act(() =>
      result.current.edit((d) => {
        d.name = "Edited";
      }),
    );
    expect(result.current.sync).toBe("pending");
    expect(lastPushed().name).toBe("Edited");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(result.current.sync).toBe("saving");
    expect(mocked.commit).toHaveBeenCalledWith("0:1");
    await act(async () => response.resolve({ ...status, token: "1:1" }));
    expect(result.current.sync).toBe("saved");
    expect(result.current.dirty()).toBe(false);
  });
  it("keeps unchanged records identical so only edits are sent", async () => {
    const two: Budget = {
      ...budget,
      accounts: [
        { id: "a", name: "A", kind: "cash", openingBalance: 0, archived: false },
        { id: "b", name: "B", kind: "cash", openingBalance: 0, archived: false },
      ],
    };
    mocked.load.mockResolvedValue({ ...status, budget: two });
    const { result } = await start();
    const before = result.current.budget!;
    act(() =>
      result.current.edit((d) => {
        d.accounts[1].name = "Renamed";
      }),
    );
    const after = result.current.budget!;
    expect(after.accounts[0]).toBe(before.accounts[0]);
    expect(after.accounts[1]).not.toBe(before.accounts[1]);
    act(() => result.current.edit(() => {}));
    expect(result.current.budget).toBe(after);
  });
  it("serializes a new edit behind an in-flight save using the returned token", async () => {
    const first = deferred<typeof status>();
    mocked.commit
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce({ ...status, token: "2:1" });
    const { result } = await start();
    act(() =>
      result.current.edit((d) => {
        d.name = "One";
      }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    act(() =>
      result.current.edit((d) => {
        d.name = "Two";
      }),
    );
    await act(async () => first.resolve({ ...status, token: "1:1" }));
    expect(mocked.commit).toHaveBeenCalledTimes(2);
    expect(mocked.commit).toHaveBeenLastCalledWith("1:1");
    expect(lastPushed().name).toBe("Two");
    expect(result.current.sync).toBe("saved");
  });
  it("preserves the draft and pauses writes on a conflict", async () => {
    mocked.commit.mockRejectedValue(
      "CONFLICT: Another writer changed the database.",
    );
    const { result } = await start();
    act(() =>
      result.current.edit((d) => {
        d.name = "Mine";
      }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(result.current.sync).toBe("conflict");
    expect(lastPushed().name).toBe("Mine");
    act(() =>
      result.current.edit((d) => {
        d.name = "Still mine";
      }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
      await result.current.flush();
    });
    expect(mocked.commit).toHaveBeenCalledTimes(1);
    expect(result.current.budget?.name).toBe("Still mine");
    // Rust keeps the draft (and its recovery file) current even while blocked.
    expect(lastPushed().name).toBe("Still mine");
  });
  it("saves pending edits and makes the on-close backup before quitting for an update", async () => {
    localStorage.setItem("lb:prefs", JSON.stringify({ backupOnClose: true }));
    mocked.backup.mockResolvedValue("/test/backup.sqlite3");
    const { result } = await start();
    act(() =>
      result.current.edit((d) => {
        d.name = "Before the update";
      }),
    );
    let ready = false;
    await act(async () => {
      ready = await result.current.prepareExit();
    });
    expect(ready).toBe(true);
    expect(lastPushed().name).toBe("Before the update");
    expect(mocked.commit).toHaveBeenCalledWith("0:1");
    expect(mocked.backup.mock.invocationCallOrder[0]).toBeGreaterThan(mocked.commit.mock.invocationCallOrder[0]);
    expect(result.current.dirty()).toBe(false);
  });
  it("refuses to quit for an update when the changes can't be saved", async () => {
    mocked.commit.mockRejectedValue("CONFLICT: Another writer changed the database.");
    const { result } = await start();
    act(() =>
      result.current.edit((d) => {
        d.name = "Mine";
      }),
    );
    let ready = true;
    await act(async () => {
      ready = await result.current.prepareExit();
    });
    expect(ready).toBe(false);
    expect(result.current.sync).toBe("conflict");
    expect(result.current.dirty()).toBe(true);
  });
  it("reports a recovery file that could not be written", async () => {
    mocked.push.mockResolvedValue({ recoveryError: "disk full" });
    const { result } = await start();
    await act(async () =>
      result.current.edit((d) => {
        d.name = "Edited";
      }),
    );
    expect(result.current.error).toContain("recovery draft could not be written");
  });
  it("does not overwrite an edit that arrives during a fetch", async () => {
    const { result } = await start();
    const response = deferred<{ budget: Budget } & typeof status>();
    mocked.load.mockReturnValueOnce(response.promise);
    let pending!: Promise<void>;
    act(() => {
      pending = result.current.fetchDatabase();
    });
    act(() =>
      result.current.edit((d) => {
        d.name = "Just typed";
      }),
    );
    await act(async () => {
      response.resolve({ ...status, budget });
      await pending;
    });
    expect(result.current.budget?.name).toBe("Just typed");
    expect(result.current.error).toContain("another edit");
    expect(lastPushed().name).toBe("Just typed");
  });
  it("offers recovery before writing and restores when the base still matches", async () => {
    mocked.recoveryRead.mockResolvedValue({
      budget: { ...budget, name: "Recovered" },
      matches: true,
    });
    const { result } = await start();
    expect(result.current.recovery).not.toBeNull();
    expect(mocked.commit).not.toHaveBeenCalled();
    await act(async () => {
      await result.current.restoreRecovery();
    });
    expect(result.current.budget?.name).toBe("Recovered");
    expect(mocked.commit).toHaveBeenCalledWith("0:1");
    expect(result.current.sync).toBe("saved");
  });
  it("never autosaves a recovery draft based on an older database", async () => {
    mocked.recoveryRead.mockResolvedValue({
      budget: { ...budget, name: "Recovered" },
      matches: false,
    });
    const { result } = await start();
    await act(async () => {
      await result.current.restoreRecovery();
    });
    expect(result.current.sync).toBe("conflict");
    expect(mocked.commit).not.toHaveBeenCalled();
  });
  it("keeping the database discards the recovery file", async () => {
    mocked.recoveryRead.mockResolvedValue({
      budget: { ...budget, name: "Recovered" },
      matches: true,
    });
    const { result } = await start();
    await act(async () => {
      await result.current.fetchDatabase();
    });
    expect(mocked.discardRecovery).toHaveBeenCalled();
    expect(result.current.recovery).toBeNull();
    expect(result.current.budget?.name).toBe("Original");
  });
  it("fetches a competing write automatically only when the draft is clean", async () => {
    const { result } = await start();
    mocked.status.mockResolvedValue({ ...status, token: "1:2" });
    mocked.load.mockResolvedValue({
      ...status,
      token: "1:2",
      budget: { ...budget, name: "Other window" },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(result.current.budget?.name).toBe("Other window");
    expect(mocked.commit).not.toHaveBeenCalled();
  });
  it("does not adopt a competing revision while an exclusive change is running", async () => {
    const { result } = await start();
    const check = deferred<typeof status>();
    mocked.status.mockReturnValueOnce(check.promise);
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });

    const calculation = deferred<Budget>();
    let changing!: Promise<void>;
    await act(async () => {
      changing = result.current.transform(() => calculation.promise);
    });
    mocked.load.mockResolvedValue({ ...status, token: "0:2", budget: { ...budget, name: "Other writer" } });
    await act(async () => { check.resolve({ ...status, token: "0:2" }); });
    expect(mocked.load).toHaveBeenCalledTimes(1);
    expect(result.current.budget?.name).toBe("Original");

    mocked.commit.mockRejectedValue("CONFLICT: Another writer changed the database.");
    await act(async () => {
      calculation.resolve({ ...budget, name: "Local calculation" });
      await expect(changing).rejects.toThrow("could not be saved");
    });
    expect(mocked.commit).toHaveBeenCalledWith("0:1");
    expect(result.current.sync).toBe("conflict");
    expect(result.current.budget?.name).toBe("Local calculation");
  });
});

it("posts due schedules after recovery is checked on startup", async () => {
  const b: Budget = {
    ...structuredClone(budget),
    schedules: [
      {
        id: "s",
        nextDate: "2020-01-01",
        anchorDate: "2020-01-01",
        frequency: "never",
        paused: false,
        transaction: {
          id: "s",
          date: "2020-01-01",
          accountId: "a",
          payee: "Rent",
          memo: "",
          amount: -100,
          status: "uncleared",
          splits: [{ categoryId: null, amount: -100 }],
          transferAccountId: null,
          transferStatus: null,
        },
      },
    ],
  };
  mocked.load.mockResolvedValue({ ...status, budget: b });
  const completed = structuredClone(b);
  completed.schedules![0].nextDate = null;
  mocked.runSchedules.mockResolvedValue({
    ...status,
    token: "1:1",
    budget: completed,
  });
  const { result } = await start();
  await act(async () => {});
  expect(mocked.runSchedules).toHaveBeenCalledOnce();
  expect(result.current.budget?.schedules?.[0].nextDate).toBeNull();
  expect(result.current.sync).toBe("saved");
});

it("asks the scheduler once to move a transaction dated ahead into Scheduled", async () => {
  const b: Budget = {
    ...structuredClone(budget),
    transactions: [
      {
        id: "later",
        date: "2999-01-01",
        accountId: "a",
        payee: "Rent",
        memo: "",
        amount: -100,
        status: "uncleared",
        splits: [{ categoryId: null, amount: -100 }],
        transferAccountId: null,
        transferStatus: null,
      },
    ],
  };
  mocked.load.mockResolvedValue({ ...status, budget: b });
  // Rust keeps it (it could be imported from YNAB): asking again wouldn't change that.
  mocked.runSchedules.mockResolvedValue({ ...status, budget: structuredClone(b) });
  await start();
  await act(async () => {});
  expect(mocked.runSchedules).toHaveBeenCalledOnce();
  await act(async () => {
    vi.advanceTimersByTime(120000);
  });
  expect(mocked.runSchedules).toHaveBeenCalledOnce();
});

it("undo and redo autosave with the latest database revision and fetch clears history", async () => {
  const { result } = await start();
  act(() =>
    result.current.edit((b) => {
      b.name = "Changed";
    }),
  );
  expect(result.current.canUndo).toBe(true);
  act(() => result.current.undo());
  expect(result.current.budget?.name).toBe("Original");
  expect(result.current.canRedo).toBe(true);
  act(() => result.current.redo());
  expect(result.current.budget?.name).toBe("Changed");
  await act(async () => {
    await vi.advanceTimersByTimeAsync(400);
  });
  expect(lastPushed().name).toBe("Changed");
  expect(mocked.commit).toHaveBeenLastCalledWith("0:1");
  act(() => result.current.undo());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(400);
  });
  expect(lastPushed().name).toBe("Original");
  expect(mocked.commit).toHaveBeenLastCalledWith("1:1");
  await act(async () => {
    await result.current.fetchDatabase();
  });
  expect(result.current.canUndo).toBe(false);
  expect(result.current.canRedo).toBe(false);
});
it("a new edit after undo drops the redo branch", async () => {
  const { result } = await start();
  act(() =>
    result.current.edit((b) => {
      b.name = "One";
    }),
  );
  act(() => result.current.undo());
  act(() =>
    result.current.edit((b) => {
      b.name = "Two";
    }),
  );
  expect(result.current.canRedo).toBe(false);
});

it("holds the saved budget unchanged during update installation and unlocks on failure", async () => {
  localStorage.setItem("lb:prefs", JSON.stringify({ backupOnClose: true }));
  mocked.backup.mockResolvedValue("/test/backup.sqlite3");
  const { result } = await start();
  act(() => result.current.edit((b) => { b.name = "Before update"; }));
  const pending = deferred<void>();
  let installing!: Promise<void>;
  const installer = vi.fn(async () => { await pending.promise; throw new Error("Store canceled"); });
  let outcome!: Promise<void>;
  await act(async () => {
    installing = result.current.installUpdate(installer);
    outcome = expect(installing).rejects.toThrow("Store canceled");
  });
  expect(mocked.commit).toHaveBeenCalled();
  expect(installer.mock.invocationCallOrder[0]).toBeGreaterThan(mocked.backup.mock.invocationCallOrder[0]);
  expect(result.current.operation).toContain("Installing");
  act(() => { result.current.edit((b) => { b.name = "Must not apply"; }); result.current.undo(); });
  expect(result.current.budget?.name).toBe("Before update");
  await act(async () => { await expect(result.current.switchBudget("other")).rejects.toThrow(/Finish the current/); });
  await act(async () => { pending.resolve(); await outcome; });
  expect(result.current.operation).toBe("");
  act(() => result.current.edit((b) => { b.name = "Editable again"; }));
  expect(result.current.budget?.name).toBe("Editable again");
});

it("does not start installation when a pending save fails", async () => {
  const { result } = await start();
  act(() => result.current.edit((b) => { b.name = "Unsaved"; }));
  mocked.commit.mockRejectedValue(new Error("disk full"));
  const installer = vi.fn();
  await act(async () => { await expect(result.current.installUpdate(installer)).rejects.toThrow(/unsaved changes/); });
  expect(installer).not.toHaveBeenCalled();
  expect(result.current.operation).toBe("");
  expect(result.current.budget?.name).toBe("Unsaved");
});

it("allows installer shutdown only after saving and resets the normal close policy after cancellation", async () => {
  const { result } = await start();
  act(() => result.current.edit((b) => { b.name = "Save before closing"; }));
  const save = deferred<typeof status>();
  const installed = deferred<void>();
  mocked.commit.mockReturnValueOnce(save.promise);
  const installer = vi.fn(() => installed.promise);
  let installing!: Promise<void>;
  await act(async () => { installing = result.current.installUpdate(installer); });
  const close = () => mocked.onCloseRequested.mock.calls.at(-1)![0] as (event: { preventDefault: () => void }) => Promise<void>;
  const event = { preventDefault: vi.fn() };
  await act(async () => { await close()(event); });
  expect(event.preventDefault).toHaveBeenCalledOnce();
  expect(installer).not.toHaveBeenCalled();
  await act(async () => { save.resolve({ ...status, token: "saved-update" }); });
  event.preventDefault.mockClear();
  await act(async () => { await close()(event); });
  expect(event.preventDefault).not.toHaveBeenCalled();
  expect(installer).toHaveBeenCalledOnce();
  await act(async () => { installed.resolve(); await installing; });
  act(() => result.current.edit((b) => { b.name = "A later change"; }));
  mocked.commit.mockRejectedValueOnce(new Error("disk full"));
  await act(async () => { await close()(event); });
  expect(event.preventDefault).toHaveBeenCalledOnce();
  expect(mocked.destroy).not.toHaveBeenCalled();
});

it("runs a waiting update on close only after saving, and closes even if it fails", async () => {
  const { result } = await start();
  const close = () => mocked.onCloseRequested.mock.calls.at(-1)![0] as (event: { preventDefault: () => void }) => Promise<void>;
  const steps: string[] = [];
  mocked.commit.mockImplementation(async () => {
    steps.push("save");
    return { ...status, token: "saved-on-close" };
  });
  mocked.destroy.mockImplementation(async () => { steps.push("close"); });
  act(() => result.current.edit((b) => { b.name = "Unsaved at close"; }));
  act(() => result.current.setExitTask(async () => { steps.push("update"); }));
  const event = { preventDefault: vi.fn() };
  await act(async () => { await close()(event); });
  expect(event.preventDefault).toHaveBeenCalledOnce();
  expect(steps).toEqual(["save", "update", "close"]);

  steps.length = 0;
  mocked.status.mockResolvedValue({ ...status, token: "saved-on-close" });
  act(() => result.current.setExitTask(async () => { throw new Error("Windows declined"); }));
  await act(async () => { await close()({ preventDefault: vi.fn() }); });
  expect(steps).toEqual(["close"]);
});

it("holds the edit lock while a Store update installs on close, so nothing changes after the save (Codex review R1)", async () => {
  const { result } = await start();
  const close = () => mocked.onCloseRequested.mock.calls.at(-1)![0] as (event: { preventDefault: () => void }) => Promise<void>;
  const installing = deferred<void>();
  act(() => result.current.edit((b) => { b.name = "Saved before the update"; }));
  act(() => result.current.setExitTask(() => installing.promise));
  let closing!: Promise<void>;
  await act(async () => { closing = close()({ preventDefault: vi.fn() }); });
  expect(result.current.operation).toBe("Installing the update…");
  // Edits, undo and switching budgets all wait for the update.
  act(() => result.current.edit((b) => { b.name = "Changed while installing"; }));
  act(() => result.current.undo());
  await expect(result.current.switchBudget("other")).rejects.toThrow("Finish the current operation");
  expect(result.current.budget?.name).toBe("Saved before the update");
  expect(result.current.dirty()).toBe(false);
  // Windows (or a second close) may end the app meanwhile: everything is saved.
  const second = { preventDefault: vi.fn() };
  await act(async () => { await close()(second); });
  expect(second.preventDefault).not.toHaveBeenCalled();
  mocked.destroy.mockImplementation(async () => {
    expect(result.current.dirty()).toBe(false);
  });
  await act(async () => { installing.resolve(); await closing; });
  expect(mocked.destroy).toHaveBeenCalledOnce();
  expect(result.current.operation).toBe("");
});

it("closes without waiting on a Store update that never answers", async () => {
  const { result } = await start();
  const close = () => mocked.onCloseRequested.mock.calls.at(-1)![0] as (event: { preventDefault: () => void }) => Promise<void>;
  act(() => result.current.setExitTask(() => new Promise<void>(() => {})));
  let closing!: Promise<void>;
  await act(async () => { closing = close()({ preventDefault: vi.fn() }); });
  expect(mocked.destroy).not.toHaveBeenCalled();
  await act(async () => { await vi.advanceTimersByTimeAsync(2 * 60_000); await closing; });
  expect(mocked.destroy).toHaveBeenCalledOnce();
});

it("switching budgets saves first, then starts the other budget with fresh history", async () => {
  const other = { ...status, token: "other:3:1", budget: { ...budget, name: "Other" } };
  mocked.openBudget.mockResolvedValue(other);
  const { result } = await start();
  act(() =>
    result.current.edit((b) => {
      b.name = "Unsaved";
    }),
  );
  await act(async () => {
    await result.current.switchBudget("other");
  });
  expect(mocked.commit).toHaveBeenCalledWith("0:1");
  expect(mocked.openBudget).toHaveBeenCalledWith("other");
  expect(result.current.budget?.name).toBe("Other");
  expect(result.current.budgetId).toBe("other");
  expect(result.current.canUndo).toBe(false);
  expect(result.current.sync).toBe("saved");
});


it("background adoption leaves an open editor and pending recovery untouched", async () => {
  const hook = renderHook(() => useBudget("2026-10", true));
  await act(async () => {});
  expect(hook.result.current.canAdoptIncoming()).toBe(false);
  mocked.load.mockClear();
  await act(async () => { await hook.result.current.refreshDatabase(); });
  expect(mocked.load).not.toHaveBeenCalled();
  expect(mocked.discardRecovery).not.toHaveBeenCalled();
});

it("clean exit attempts queued network work but remains bounded", async () => {
  const {result} = await start();
  const drain = vi.fn(() => new Promise<void>(() => {}));
  act(() => result.current.setExitDrain(drain));
  let closing!: Promise<boolean>;
  await act(async () => { closing = result.current.prepareExit(); });
  expect(drain).toHaveBeenCalledOnce();
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); expect(await closing).toBe(true); });
});

it("saves the active draft before reviewing a deletion and adopts the reviewed revision", async () => {
  const {result} = await start();
  const calls: string[] = [];
  mocked.commit.mockImplementation(async () => { calls.push("save"); return {...status, token: "1:1"}; });
  mocked.libraryAction.mockImplementation(async () => {
    calls.push("review");
    mocked.load.mockResolvedValue({...status, token: "reviewed:7:1", budget: {...budget, name: "Newer remote version"}});
    return {};
  });
  act(() => result.current.edit(draft => { draft.name = "Unsaved current budget"; }));
  await act(async () => { await result.current.switchBudget("reviewed", true); });
  expect(calls).toEqual(["save", "review"]);
  expect(mocked.libraryAction).toHaveBeenCalledWith("review_deletion", "reviewed");
  expect(result.current.budget?.name).toBe("Newer remote version");
  expect(result.current.metadata?.token).toBe("reviewed:7:1");
});