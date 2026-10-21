import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("the package", () => {
  it("is named @ufaigw/client", () => {
    const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")) as { name: string };
    expect(pkg.name).toBe("@ufaigw/client");
  });
});
