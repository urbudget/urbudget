// Linux keeps tokens only where a keyring is running, so the form asks the app instead of going by the system.
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const secretStore = vi.hoisted(() => vi.fn());
vi.mock("./platform", async (original) => ({
  ...(await original<typeof import("./platform")>()),
  os: "linux",
  tokenStore: null,
}));
vi.mock("./bridge", () => ({ api: { secretStore } }));
import { YnabImport } from "./YnabImport";

afterEach(() => {
  cleanup();
  secretStore.mockReset();
});
const token = { id: "t1", label: "Home", hint: "abcd", savedAt: "2026-10-01T09:00:00Z", lastUsed: null };
const plans = () => vi.fn().mockResolvedValue([{ id: "p", name: "My budget", lastModifiedOn: null }]);

it("offers to remember a token, and lists saved ones, once the app says a keyring is running", async () => {
  secretStore.mockResolvedValue("System keyring");
  const listTokens = vi.fn().mockResolvedValue([token]);
  render(<YnabImport onImport={vi.fn()} listPlans={plans()} listTokens={listTokens} />);
  // The saved token is found only after the keyring answers, and is then the one in use.
  await waitFor(() => expect(screen.getByText("••••abcd")).toBeTruthy());
  expect(secretStore).toHaveBeenCalledTimes(1);
  expect(listTokens).toHaveBeenCalledTimes(1);
});

it("keeps to one-time tokens where there is no keyring", async () => {
  secretStore.mockResolvedValue(null);
  const listTokens = vi.fn().mockResolvedValue([token]);
  render(<YnabImport onImport={vi.fn()} listPlans={plans()} listTokens={listTokens} />);
  await waitFor(() => expect(secretStore).toHaveBeenCalled());
  expect(screen.getByLabelText("YNAB personal access token")).toBeTruthy();
  expect(screen.queryByLabelText("Remember on this device")).toBeNull();
  expect(listTokens).not.toHaveBeenCalled();
});

it("doesn't ask when the store is given", async () => {
  render(<YnabImport onImport={vi.fn()} listPlans={plans()} listTokens={vi.fn().mockResolvedValue([])} tokenStore="Keychain" />);
  expect(await screen.findByLabelText("Remember on this device")).toBeTruthy();
  expect(secretStore).not.toHaveBeenCalled();
});
