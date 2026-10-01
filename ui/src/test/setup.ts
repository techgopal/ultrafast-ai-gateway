import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach } from "vitest";
import { setCsrfToken } from "@/api/client";
import { resetDevice } from "./device";
import { server } from "./handlers";

// Tests of the source and of the build run in Node, without a page.
const hasPage = typeof window !== "undefined";

// A request that no handler answers fails the test: nothing leaves the process.
beforeAll(() => {
  server.listen({ onUnhandledRequest: "error" });
});

afterAll(() => {
  server.close();
});

beforeEach(() => {
  if (hasPage) resetDevice();
});

afterEach(() => {
  server.resetHandlers();
  // Setting a token lets the next test hear of a 401 again; then no token is left.
  setCsrfToken("token-of-the-test-before");
  setCsrfToken(null);
  if (!hasPage) return;
  cleanup();
  window.localStorage.clear();
  document.documentElement.classList.remove("dark");
  document.documentElement.style.colorScheme = "";
});
