import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { TitleBar } from "./WindowControls";

const win = vi.hoisted(() => {
  const focus: ((e: { payload: boolean }) => void)[] = [];
  return {
    focus,
    name: "UrBudget (assistant preview)",
    title: async () => win.name,
    isMaximized: async () => false,
    onResized: async () => () => {},
    onFocusChanged: async (handler: (e: { payload: boolean }) => void) => {
      focus.push(handler);
      return () => {};
    },
    minimize: vi.fn(async () => {}),
    toggleMaximize: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
  };
});
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => win }));

afterEach(cleanup);
const shownName = (container: HTMLElement) => container.querySelector(".titlebar-name")?.textContent;

it("names the window, holds the app's buttons and moves the window from its empty space", async () => {
  const { container } = render(
    <TitleBar>
      <button>Assistant</button>
    </TitleBar>,
  );
  await waitFor(() => expect(shownName(container)).toBe("UrBudget (assistant preview)"));
  expect(screen.getByRole("button", { name: "Assistant" }).closest(".titlebar-actions")).toBeTruthy();
  expect(container.querySelector(".titlebar")?.getAttribute("data-tauri-drag-region")).toBe("deep");

  fireEvent.click(screen.getByRole("button", { name: "Minimize" }));
  fireEvent.click(screen.getByRole("button", { name: "Maximize" }));
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(win.minimize).toHaveBeenCalled();
  expect(win.toggleMaximize).toHaveBeenCalled();
  expect(win.close).toHaveBeenCalled();
});

it("shows the mark and wordmark for the app's name, and any other name as it is", async () => {
  const { container, unmount } = render(<TitleBar />);
  await waitFor(() => expect(shownName(container)).toBe("UrBudget (assistant preview)"));
  // The wordmark's "Ur" is set apart in the mark's colour; the preview label follows it.
  expect(container.querySelector(".titlebar-name.wordmark .titlebar-wordmark")?.textContent).toBe("UrBudget");
  expect(container.querySelector(".titlebar-wordmark .titlebar-ur")?.textContent).toBe("Ur");
  expect(container.querySelector(".titlebar-name.wordmark .titlebar-label")?.textContent).toBe(" (assistant preview)");
  unmount();

  win.name = "UrBudget";
  const plain = render(<TitleBar />);
  await waitFor(() => expect(plain.container.querySelector(".titlebar-name.wordmark")).toBeTruthy());
  expect(plain.container.querySelector(".titlebar-label")).toBeNull();
  plain.unmount();

  win.name = "Something else";
  const other = render(<TitleBar />);
  await waitFor(() => expect(shownName(other.container)).toBe("Something else"));
  expect(other.container.querySelector(".wordmark")).toBeNull();
  win.name = "UrBudget (assistant preview)";
});

it("fades while another window has focus", async () => {
  const { container } = render(<TitleBar />);
  await waitFor(() => expect(shownName(container)).toBe("UrBudget (assistant preview)"));
  expect(container.querySelector(".titlebar-actions")).toBeNull();
  act(() => win.focus.at(-1)!({ payload: false }));
  expect(container.querySelector(".titlebar.inactive")).toBeTruthy();
  act(() => win.focus.at(-1)!({ payload: true }));
  expect(container.querySelector(".titlebar.inactive")).toBeNull();
});
