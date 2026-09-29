import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test, vi } from "vitest";
import { setDevice } from "@/test/device";
import { renderWithApp } from "@/test/render";

const root = document.documentElement;
const themeScript = readFileSync(resolve(__dirname, "../../public/theme.js"), "utf8");

function runThemeScript(): void {
  // The script is a plain file served next to the app; here it runs as the browser runs it.
  // eslint-disable-next-line @typescript-eslint/no-implied-eval, @typescript-eslint/no-unsafe-call -- the file under test is a script, not a module
  new Function(themeScript)();
}

function blockStorage(): void {
  const blocked = (): never => {
    throw new DOMException("blocked", "SecurityError");
  };
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(blocked);
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(blocked);
  vi.spyOn(Storage.prototype, "removeItem").mockImplementation(blocked);
}

function themeButton(name: string): HTMLElement {
  const group = screen.getByRole("group", { name: "Theme" });
  return within(group).getByRole("button", { name });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("theme script", () => {
  test("theme follows the device by default", () => {
    setDevice({ prefersDark: true });
    runThemeScript();
    expect(root).toHaveClass("dark");
    expect(root.style.colorScheme).toBe("dark");

    setDevice({ prefersDark: false });
    runThemeScript();
    expect(root).not.toHaveClass("dark");
    expect(root.style.colorScheme).toBe("light");
  });

  test.each([
    ["dark", false, true],
    ["light", true, false],
    ["system", true, true],
    ["system", false, false],
    ["nonsense", true, true],
  ])("saved %s, device dark %s", (saved, prefersDark, dark) => {
    window.localStorage.setItem("uf-theme", saved);
    setDevice({ prefersDark });
    runThemeScript();
    expect(root.classList.contains("dark")).toBe(dark);
  });

  test("theme survives blocked storage", () => {
    blockStorage();
    setDevice({ prefersDark: true });
    expect(runThemeScript).not.toThrow();
    expect(root).toHaveClass("dark");
  });
});

describe("theme provider", () => {
  test("theme follows the device by default", async () => {
    const dark = await renderWithApp(null, { device: "dark" });
    expect(root).toHaveClass("dark");
    expect(root.style.colorScheme).toBe("dark");
    expect(themeButton("System")).toHaveAttribute("aria-pressed", "true");
    dark.unmount();

    await renderWithApp(null, { device: "light" });
    expect(root).not.toHaveClass("dark");
    expect(root.style.colorScheme).toBe("light");
  });

  test("theme follows the device when it changes", async () => {
    await renderWithApp(null, { device: "light" });
    act(() => {
      setDevice({ prefersDark: true });
    });
    expect(root).toHaveClass("dark");
    act(() => {
      setDevice({ prefersDark: false });
    });
    expect(root).not.toHaveClass("dark");
  });

  test("a fixed choice ignores the device", async () => {
    await renderWithApp(null, { theme: "light", device: "dark" });
    expect(root).not.toHaveClass("dark");
    act(() => {
      setDevice({ prefersDark: false });
      setDevice({ prefersDark: true });
    });
    expect(root).not.toHaveClass("dark");
  });

  test("theme choice is kept", async () => {
    const user = userEvent.setup();
    const first = await renderWithApp(null, { device: "light" });
    await user.click(themeButton("Dark"));
    expect(root).toHaveClass("dark");
    expect(window.localStorage.getItem("uf-theme")).toBe("dark");
    expect(themeButton("Dark")).toHaveAttribute("aria-pressed", "true");
    first.unmount();
    root.classList.remove("dark");

    // A fresh render keeps it.
    await renderWithApp(null, { device: "light" });
    expect(root).toHaveClass("dark");
    expect(themeButton("Dark")).toHaveAttribute("aria-pressed", "true");

    // System removes the override and follows the device.
    await user.click(themeButton("System"));
    expect(window.localStorage.getItem("uf-theme")).toBeNull();
    expect(root).not.toHaveClass("dark");
    act(() => {
      setDevice({ prefersDark: true });
    });
    expect(root).toHaveClass("dark");
  });

  test("theme survives blocked storage", async () => {
    blockStorage();
    const user = userEvent.setup();
    await renderWithApp(null, { device: "dark" });
    expect(screen.getByRole("navigation", { name: "Main" })).toBeInTheDocument();
    expect(root).toHaveClass("dark");

    // The choice still works for this visit, it is just not kept.
    await user.click(themeButton("Light"));
    expect(root).not.toHaveClass("dark");
  });
});
