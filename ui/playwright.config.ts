import { defineConfig } from "@playwright/test";

// The browser tests run the real gateway binary (see e2e/gateway.ts), which
// must be built first:
//   pnpm --dir ui build && cargo build --release -p ultrafast-gateway
//
// The report holds no test account and no secret of the gateway: no traces,
// screenshots or videos, which would show what is typed and shown; and no
// snapshot of the page with a failure, which would show the text of fields.
process.env.PLAYWRIGHT_NO_COPY_PROMPT = "1";

const CI = process.env.CI !== undefined && process.env.CI !== "";

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
  reporter: CI ? [["list"], ["html", { open: "never" }]] : [["list"]],
  use: {
    browserName: "chromium",
    trace: "off",
    screenshot: "off",
    video: "off",
    // The Copy buttons of the dialogs write to the clipboard; the tests read it.
    permissions: ["clipboard-read", "clipboard-write"],
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
