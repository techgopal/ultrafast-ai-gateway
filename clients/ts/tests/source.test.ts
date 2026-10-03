import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = new URL("../src", import.meta.url).pathname;

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((d) => {
    const p = join(dir, d.name);
    return d.isDirectory() ? files(p) : /\.(ts|js)$/.test(d.name) ? [p] : [];
  });
}

// The wasm-bindgen glue is generated; the package's own sources are scanned
// whole, the glue for the same forbidden names.
describe("src/ is free of Node-specific APIs", () => {
  const all = files(SRC);
  it("scans something", () => {
    expect(all.some((f) => f.endsWith("client.ts"))).toBe(true);
    expect(all.some((f) => f.endsWith("ultrafast_client_wasm.js"))).toBe(true);
  });
  const forbidden: Array<[string, RegExp]> = [
    ["Buffer", /\bBuffer\b/],
    ["process.", /\bprocess\s*\./],
    ["require(", /\brequire\s*\(/],
    ["node: import", /["']node:/],
    ["__dirname", /__dirname|__filename/],
    ["Node built-in import", /(?:from|import)\s*\(?\s*["'](?:fs|path|http|https|net|os|url|util|stream|crypto|child_process|zlib|events|buffer)(?:\/[a-z]+)?["']/],
  ];
  for (const f of all) {
    const text = readFileSync(f, "utf8").replace(/^\s*(?:\/\/|\*).*$/gm, "");
    for (const [name, re] of forbidden) {
      it(`${f.slice(SRC.length + 1)} has no ${name}`, () => {
        expect(text).not.toMatch(re);
      });
    }
  }
  it("streams through a ReadableStream reader (text decoding is in wasm)", () => {
    const stream = readFileSync(join(SRC, "stream.ts"), "utf8");
    expect(stream).toMatch(/getReader\(\)/);
  });
});
