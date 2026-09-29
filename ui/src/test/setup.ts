import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach } from "vitest";
import { resetDevice } from "./device";

// Tests of the source and of the build run in Node, without a page.
const hasPage = typeof window !== "undefined";

beforeEach(() => {
  if (hasPage) resetDevice();
});

afterEach(() => {
  if (!hasPage) return;
  cleanup();
  window.localStorage.clear();
  document.documentElement.classList.remove("dark");
  document.documentElement.style.colorScheme = "";
});
