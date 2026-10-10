import { defineConfig } from "@playwright/test";

// The browser tests run the real gateway binary (see e2e/gateway.ts), which
// must be built first:
//   pnpm --dir ui build && cargo build --release -p ultrafast-gateway
//
// What a failure writes may hold the test's credentials: an error message can
// show a value, and Playwright writes a snapshot of the page, with the text of
// its fields, into `test-results` (`PLAYWRIGHT_NO_COPY_PROMPT` stops only one
// of its snapshots). Those credentials are random, made for the one test, and
// die with its gateway. So the output stays on the machine that ran the tests:
// the only reporter is the list, which CI keeps in the job's log, and CI
// uploads neither an HTML report nor `test-results`. There are no traces,
// screenshots or videos, which would show what is typed and shown.
process.env.PLAYWRIGHT_NO_COPY_PROMPT = "1";

const CI = process.env.CI !== undefined && process.env.CI !== "";

// `UF_E2E_BROWSER=firefox` or `webkit` runs the tests in that browser (use
// `--project=desktop`: the phone project emulates a touch device Firefox lacks).
const BROWSERS = ["chromium", "firefox", "webkit"] as const;
const asked = process.env.UF_E2E_BROWSER;
const browserName = BROWSERS.find((name) => name === asked) ?? "chromium";

export default defineConfig({
  testDir: "./e2e",
  outputDir: "./test-results",
  fullyParallel: true,
  forbidOnly: CI,
  // A test that passes only when tried again is a defect.
  retries: 0,
  workers: CI ? 2 : 3,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  reporter: [["list"]],
  use: {
    browserName,
    trace: "off",
    screenshot: "off",
    video: "off",
    // The Copy buttons of the dialogs write to the clipboard; the tests read it.
    // Only Chromium knows these permissions; another browser refuses to start with them.
    permissions: browserName === "chromium" ? ["clipboard-read", "clipboard-write"] : [],
  },
  projects: [
    {
      name: "desktop",
      use: { viewport: { width: 1280, height: 800 } },
    },
    {
      name: "phone",
      use: {
        viewport: { width: 390, height: 844 },
        deviceScaleFactor: 3,
        isMobile: true,
        hasTouch: true,
      },
    },
  ],
});
