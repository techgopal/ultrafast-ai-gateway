import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["test/**/*.test.ts"],
    globalSetup: ["test/global-setup.ts"],
    // One gateway per file; building and starting it can take a while on a loaded machine.
    testTimeout: 60_000,
    hookTimeout: 120_000,
  },
});
