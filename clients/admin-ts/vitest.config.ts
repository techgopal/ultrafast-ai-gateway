import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["test/**/*.test.ts"],
    // One gateway per file; building and starting it can take a while on a loaded machine.
    testTimeout: 60_000,
    // The first start of a file builds the gateway when it is not built.
    hookTimeout: 900_000,
  },
});
