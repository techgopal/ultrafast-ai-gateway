// @vitest-environment node
//
// What matters is that the app makes no request to another origin. A scan of the
// text of the build cannot prove that: it only finds URLs that stand in the files.
// The browser tests (e2e) prove it: they fail on any request to another origin.
// This scan is the early warning, so that a URL in the build is a decision and
// not an accident.
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

/**
 * Removes licence banners: a block comment at the very start of the file, and
 * any block comment that carries `@license` or `@preserve`. Other comments stay.
 */
function withoutLicenceComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, (comment, offset: number) =>
    text.slice(0, offset).trim() === "" || /@license|@preserve/i.test(comment) ? "" : comment,
  );
}

// A URL with a host: a scheme and `//`, or a protocol-relative `//`, then a host
// name (dotted or not, such as `localhost`), an IPv4 address or an IPv6 address.
const urlWithHost =
  /(?:\b[a-z][a-z0-9+.-]*:\/\/|(?<![:/\w.*-])\/\/)(?:[^\s"'`/@<>\\]*@)?(?:\[[0-9a-f:.]+\]|[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?)(?::\d+)?(?:[/?#][^\s"'`)<>\\]*)?/gi;

function urlsIn(text: string): string[] {
  return withoutLicenceComments(text).match(urlWithHost) ?? [];
}

interface AllowedUrl {
  /** The exact text of the URL as the scan finds it. */
  text: string;
  /** `exact`: the whole URL is this text. `prefix`: the URL starts with it. */
  match: "exact" | "prefix";
}

const allowedUrls: AllowedUrl[] = [
  // react-dom: the production build shortens its error messages to a code and
  // names the page that explains the code. It is text inside an Error message.
  // No request: nothing fetches it; a person reads it in the console.
  { text: "https://react.dev/errors/", match: "prefix" },
  // @tanstack/router-core: the origin the router falls back to when the page has
  // none (`window.origin` missing or "null"). It is the base for parsing paths
  // with `new URL(path, origin)`. No request: it is only parsed, and a page
  // served by the gateway always has its own origin.
  { text: "http://localhost", match: "exact" },
];

function allowedEntry(url: string): AllowedUrl | undefined {
  return allowedUrls.find((entry) =>
    entry.match === "exact" ? url === entry.text : url.startsWith(entry.text),
  );
}

describe("the allow-list of the URL scan", () => {
  test.each([
    "http://localhost:3900",
    "http://localhost/api",
    "https://localhost",
    "//localhost",
    "http://127.0.0.1",
    "http://localhost.example.com",
    "https://react.dev/",
    "https://react.dev/link/x",
    "http://react.dev/errors/1",
    "https://example.com/?u=https://react.dev/errors/",
  ])("does not allow %s", (url) => {
    expect(allowedEntry(url)).toBeUndefined();
  });

  test("allows exactly what is listed", () => {
    expect(allowedEntry("http://localhost")?.text).toBe("http://localhost");
    expect(allowedEntry("https://react.dev/errors/")?.match).toBe("prefix");
    expect(allowedEntry("https://react.dev/errors/418")?.match).toBe("prefix");
  });
});

describe("the URL scan", () => {
  test.each([
    "https://example.com/a",
    "http://localhost",
    "http://localhost:3900/api",
    "http://127.0.0.1:8080/x",
    "http://[::1]/x",
    "ws://intranet/socket",
    "https://user@host/path",
  ])("sees %s", (url) => {
    expect(urlsIn(`fetch("${url}")`)).toEqual([url]);
  });

  test("sees a protocol-relative URL", () => {
    expect(urlsIn('src="//cdn.example.com/x.js"')).toEqual(["//cdn.example.com/x.js"]);
    expect(urlsIn('src="//localhost/x.js"')).toEqual(["//localhost/x.js"]);
  });

  test("does not take paths and comments for URLs", () => {
    expect(urlsIn('href="/assets/index.js"')).toEqual([]);
    expect(urlsIn("a = b; // a comment")).toEqual([]);
    expect(urlsIn("react.dev/errors/418")).toEqual([]);
  });

  test("drops licence banners only", () => {
    const banner = "/*! lib v1 https://banner.example/licence */";
    const tagged = "/** @license MIT https://tagged.example */";
    const other = "/* see the license at https://other.example/terms */";
    expect(urlsIn(`${banner}\ncode();${tagged}${other}`)).toEqual([
      "https://other.example/terms",
    ]);
    // A comment that is not at the start and has no licence tag is kept.
    expect(urlsIn(`code();/*! https://late.example/x */`)).toEqual(["https://late.example/x"]);
  });
});

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
  function scan(): { found: string[]; namespaces: Set<string>; used: Set<string> } {
    const namespace = /^https?:\/\/www\.w3\.org\//;
    const found: string[] = [];
    const namespaces = new Set<string>();
    const used = new Set<string>();
    for (const path of files(dist)) {
      for (const url of urlsIn(readFileSync(path, "utf8"))) {
        const entry = allowedEntry(url);
        if (namespace.test(url)) namespaces.add(url);
        else if (entry !== undefined) used.add(entry.text);
        else found.push(`${relative(dist, path)}: ${url}`);
      }
    }
    return { found, namespaces, used };
  }

  test("build output is self-contained", () => {
    const { found, namespaces, used } = scan();
    console.info(
      `build output: ${files(dist).length} files; namespace URIs: ${[...namespaces].sort().join(", ") || "none"}; allowed: ${[...used].sort().join(", ") || "none"}; other URLs: ${found.length}`,
    );
    expect(found).toEqual([]);
  });

  test("the allow-list has no unused entry", () => {
    const { used } = scan();
    const unused = allowedUrls
      .filter((entry) => !used.has(entry.text))
      .map((entry) => `${entry.text} is no longer in the build: remove it from allowedUrls`);
    expect(unused).toEqual([]);
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
