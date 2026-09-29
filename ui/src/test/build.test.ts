// @vitest-environment node
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";
import { build } from "vite";
import { afterAll, beforeAll, describe, expect, test } from "vitest";

const ui = resolve(__dirname, "../..");
let dist = "";

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? files(path) : [path];
  });
}

/** Removes licence comments: block comments that start with `/*!` or carry `@license`. */
function withoutLicenceComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, (comment) =>
    /^\/\*!|@license|@preserve|licen[cs]e/i.test(comment) ? "" : comment,
  );
}

beforeAll(async () => {
  dist = mkdtempSync(join(tmpdir(), "uf-console-build-"));
  // The test runner sets NODE_ENV to "test"; the build under test is the production one.
  const nodeEnv = process.env.NODE_ENV;
  process.env.NODE_ENV = "production";
  try {
    await buildConsole();
  } finally {
    process.env.NODE_ENV = nodeEnv;
  }
}, 120_000);

async function buildConsole(): Promise<void> {
  await build({
    mode: "production",
    root: ui,
    configFile: join(ui, "vite.config.ts"),
    logLevel: "error",
    build: { outDir: dist, emptyOutDir: true },
  });
}

afterAll(() => {
  if (dist !== "") rmSync(dist, { recursive: true, force: true });
});

describe("build output", () => {
  test("build output is self-contained", () => {
    const allowed = /^https?:\/\/www\.w3\.org\//;
    const found: string[] = [];
    const namespaces = new Set<string>();
    for (const path of files(dist)) {
      const text = withoutLicenceComments(readFileSync(path, "utf8"));
      // A URL with a host: a scheme or a protocol-relative `//host`.
      const urls = text.match(/(?:\b[a-z][a-z0-9+.-]*:)?\/\/[a-z0-9][a-z0-9.-]*\.[a-z]{2,}[^\s"'`)<>\\]*/gi);
      for (const url of urls ?? []) {
        if (allowed.test(url)) namespaces.add(url);
        else found.push(`${relative(dist, path)}: ${url}`);
      }
    }
    console.info(
      `build output: ${files(dist).length} files; namespace URIs: ${[...namespaces].sort().join(", ") || "none"}; other URLs: ${found.length}`,
    );
    expect(found).toEqual([]);
  });

  test("assets have hashed names and no source maps", () => {
    const names = files(dist).map((path) => relative(dist, path));
    expect(names).toContain("index.html");
    expect(names).toContain("theme.js");
    const assets = names.filter((name) => name.startsWith("assets/"));
    expect(assets.some((name) => name.endsWith(".js"))).toBe(true);
    expect(assets.some((name) => name.endsWith(".css"))).toBe(true);
    for (const name of assets) {
      expect(name).toMatch(/^assets\/[\w.-]+-[\w-]{8,}\.\w+$/);
    }
    expect(names.filter((name) => name.endsWith(".map"))).toEqual([]);
  });

  test("built page has no inline script or style", () => {
    const html = readFileSync(join(dist, "index.html"), "utf8");
    for (const [, body = ""] of html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)) {
      expect(body.trim()).toBe("");
    }
    expect(html).not.toMatch(/<style\b/i);
    expect(html).not.toMatch(/\sstyle=/i);
    expect(html).toMatch(/<script src="\/theme\.js"><\/script>/);
  });

  test("toasts do not inject a style element", () => {
    const scripts = files(dist).filter((path) => path.endsWith(".js"));
    const css = files(dist).filter((path) => path.endsWith(".css"));
    for (const path of scripts) {
      // sonner ships its stylesheet inside its script and adds it to <head> at run time.
      expect(readFileSync(path, "utf8")).not.toContain("data-sonner-toaster][dir=ltr]");
    }
    // The same rules come from the stylesheet instead.
    expect(css.some((path) => readFileSync(path, "utf8").includes("data-sonner-toaster"))).toBe(true);
  });
});
