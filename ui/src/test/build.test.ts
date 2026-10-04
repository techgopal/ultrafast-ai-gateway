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
import { build, createLogger, type Logger } from "vite";
import { afterAll, beforeAll, describe, expect, test } from "vitest";

const ui = resolve(__dirname, "../..");
let dist = "";
/** What the build warned of. A warning that stands in every build hides the next one. */
const warnings: string[] = [];

/** A logger that keeps the warnings for the test, and prints only errors. */
function keepingWarnings(): Logger {
  const quiet = createLogger("error");
  return {
    ...quiet,
    warn: (message) => {
      warnings.push(message);
    },
    warnOnce: (message) => {
      warnings.push(message);
    },
  };
}

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

// A URL with a host: a scheme and `//`, or a protocol-relative `//`, and then
// the whole run of text up to the first closing delimiter: a quote, whitespace,
// `)`, `<`, `>` or the end. The run is not cut where a host would end, so that
// what is compared with the allow-list is all of what stands in the file.
const urlWithHost =
  /(?:\b[a-z][a-z0-9+.-]*:\/\/(?=[^\s"'`)<>/])|(?<![:/\w.*-])\/\/(?=[a-z0-9[]))[^\s"'`)<>]+/gi;

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
  // pages/Providers.tsx: the known base URLs of providers. They are choices of
  // a form: choosing one fills the "Base URL" field with this text, which the
  // admin then sends to the gateway as a value. No request: the console never
  // asks any of them for anything; it is the gateway that calls a provider.
  { text: "https://api.openai.com/v1", match: "exact" },
  { text: "https://api.anthropic.com", match: "exact" },
  { text: "https://api.groq.com/openai/v1", match: "exact" },
  { text: "https://api.mistral.ai/v1", match: "exact" },
  { text: "https://openrouter.ai/api/v1", match: "exact" },
  { text: "http://localhost:11434/v1", match: "exact" },
  { text: "https://generativelanguage.googleapis.com", match: "exact" },
  // components/BaseUrlField.tsx: the hint of an Azure OpenAI provider, which says what a base
  // URL looks like ("... for example https://my-resource.openai.azure.com."). It is a text of
  // the page, with the full stop that ends the sentence. No request: nothing fetches it.
  { text: "https://my-resource.openai.azure.com.", match: "exact" },
];

// What may follow a `prefix` entry: characters of a path and a query, nothing else.
const pathAndQuery = /^[A-Za-z0-9\-_.~/?=&%[\]]*$/;

function allowedEntry(url: string): AllowedUrl | undefined {
  return allowedUrls.find((entry) =>
    entry.match === "exact"
      ? url === entry.text
      : url.startsWith(entry.text) && pathAndQuery.test(url.slice(entry.text.length)),
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
    // The known base URLs of providers are allowed as they are written, and no other address of their hosts.
    "https://api.openai.com/v1/models",
    "https://api.openai.com",
    "https://api.anthropic.com/v1",
    "https://api.groq.com",
    "https://api.mistral.ai/v1/chat",
    "https://openrouter.ai",
    "http://localhost:11434",
    "http://localhost:11434/v1/models",
    "https://generativelanguage.googleapis.com/v1beta",
    "https://my-resource.openai.azure.com",
    "https://my-resource.openai.azure.com/openai",
  ])("does not allow %s", (url) => {
    expect(allowedEntry(url)).toBeUndefined();
  });

  test("allows exactly what is listed", () => {
    expect(allowedEntry("http://localhost")?.text).toBe("http://localhost");
    expect(allowedEntry("https://react.dev/errors/")?.match).toBe("prefix");
    expect(allowedEntry("https://react.dev/errors/418")?.match).toBe("prefix");
    for (const url of [
      "https://api.openai.com/v1",
      "https://api.anthropic.com",
      "https://api.groq.com/openai/v1",
      "https://api.mistral.ai/v1",
      "https://openrouter.ai/api/v1",
      "http://localhost:11434/v1",
      "https://generativelanguage.googleapis.com",
      "https://my-resource.openai.azure.com.",
    ]) {
      expect(allowedEntry(url)).toEqual({ text: url, match: "exact" });
    }
    expect(allowedUrls).toHaveLength(10);
  });
});

/** The URLs the scan finds in the text that the allow-list does not accept. */
function refused(text: string): string[] {
  return urlsIn(text).filter((url) => allowedEntry(url) === undefined);
}

describe("the scan and the allow-list together", () => {
  const dollar = "$";
  test.each([
    // A listed text that goes on as something else must be seen whole.
    ["`http://localhost" + dollar + "{h}/x`", "http://localhost" + dollar + "{h}/x"],
    ["`http://localhost:" + dollar + "{port}`", "http://localhost:" + dollar + "{port}"],
    ['"http://localhost_a.evil.example/x"', "http://localhost_a.evil.example/x"],
    ['"http://localhost%2eevil.example"', "http://localhost%2eevil.example"],
    ['"http://localhost\\@evil.example"', "http://localhost\\@evil.example"],
    ['"http://localhost\u00e9.example"', "http://localhost\u00e9.example"],
    ['"http://localhost."', "http://localhost."],
    ['"http://localhost.evil.example"', "http://localhost.evil.example"],
    ['"https://react.dev.evil.example/errors/"', "https://react.dev.evil.example/errors/"],
    ['"http://localhost:3900"', "http://localhost:3900"],
    ['"http://localhost@evil.example"', "http://localhost@evil.example"],
    ["`https://react.dev/errors/" + dollar + "{x}`", "https://react.dev/errors/" + dollar + "{x}"],
    ['"https://react.dev/errors/1#x"', "https://react.dev/errors/1#x"],
    ['"https://react.dev/errors/1;x"', "https://react.dev/errors/1;x"],
  ])("refuses %s", (text, whole) => {
    expect(refused(`a=${text};b()`)).toEqual([whole]);
  });

  test.each([
    // The two entries in the forms they have in the build.
    "window.origin:`http://localhost`;let n",
    'this.origin = "http://localhost";',
    "`https://react.dev/errors/`+e",
    '"https://react.dev/errors/"+e',
    '"https://react.dev/errors/418?args[]=a&args[]=b%20c"',
  ])("allows %s", (text) => {
    expect(urlsIn(text)).toHaveLength(1);
    expect(refused(text)).toEqual([]);
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
    customLogger: keepingWarnings(),
    build: { outDir: dist, emptyOutDir: true },
  });
}

afterAll(() => {
  if (dist !== "") rmSync(dist, { recursive: true, force: true });
});

describe("build output", () => {
  test("the build and its config warn of nothing", () => {
    expect(warnings).toEqual([]);
  });

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
