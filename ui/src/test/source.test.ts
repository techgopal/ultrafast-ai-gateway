// @vitest-environment node
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { describe, expect, test } from "vitest";

const ui = resolve(__dirname, "../..");
const src = join(ui, "src");

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? files(path) : [path];
  });
}

function ours(extensions: string[]): string[] {
  return files(src)
    .map((path) => relative(src, path).split(sep).join("/"))
    .filter((path) => extensions.some((ext) => path.endsWith(ext)))
    .filter((path) => path !== "styles/globals.css")
    .filter((path) => path !== "styles/shadcn.css")
    .filter((path) => !path.startsWith("components/ui/"));
}

function findings(paths: string[], pattern: RegExp): string[] {
  return paths.flatMap((path) =>
    readFileSync(join(src, path), "utf8")
      .split("\n")
      .flatMap((line, index) =>
        pattern.test(line) ? [`${path}:${index + 1}: ${line.trim()}`] : [],
      ),
  );
}

// A hex colour of 3, 4, 6 or 8 digits, or a colour function with a literal inside.
const colour =
  /#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})\b|\b(?:rgba?|hsla?|oklch|oklab)\(\s*[\d.]/i;
const inlineStyle = /\bstyle\s*=/;

// An import, a re-export, a dynamic import or a CSS @import of the bare module or a path inside it.
function importOf(name: string): RegExp {
  return new RegExp(
    `(?:\\bfrom\\s*|\\bimport\\s*\\(?\\s*|@import\\s+(?:url\\(\\s*)?)["']${name}(?:/[^"']*)?["']`,
  );
}

function allSources(): string[] {
  return files(src).map((path) => relative(src, path).split(sep).join("/"));
}

function listedPackages(): string[] {
  const manifest: unknown = JSON.parse(readFileSync(join(ui, "package.json"), "utf8"));
  if (typeof manifest !== "object" || manifest === null) throw new Error("package.json is not an object");
  return ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"].flatMap(
    (field) => {
      const group: unknown = (manifest as Record<string, unknown>)[field];
      return typeof group === "object" && group !== null ? Object.keys(group) : [];
    },
  );
}

describe.each(["shadcn", "cn"])("the package %s is not used", (name) => {
  test("the import scan sees what it should", () => {
    const quote = '"';
    expect(importOf(name).test(`import { x } from ${quote}${name}${quote}`)).toBe(true);
    expect(importOf(name).test(`export { x } from '${name}'`)).toBe(true);
    expect(importOf(name).test(`@import ${quote}${name}/tailwind.css${quote};`)).toBe(true);
    expect(importOf(name).test(`import(${quote}${name}${quote})`)).toBe(true);
    expect(importOf(name).test(`import { cn } from ${quote}@/lib/utils${quote}`)).toBe(false);
    expect(importOf(name).test(`import x from ${quote}${name}-extra${quote}`)).toBe(false);
  });

  test("no file under src imports it", () => {
    expect(findings(allSources(), importOf(name))).toEqual([]);
  });

  test("package.json does not list it", () => {
    expect(listedPackages()).not.toContain(name);
  });
});

describe("source rules", () => {
  test("the scans see what they should", () => {
    const hash = "#";
    expect(colour.test(`color: ${hash}fff`)).toBe(true);
    expect(colour.test(`color: ${hash}1A2b3C`)).toBe(true);
    expect(colour.test("rgb" + "(0 0 0)")).toBe(true);
    expect(colour.test("hsla" + "(0, 0%, 0%, 1)")).toBe(true);
    expect(colour.test("bg-background text-muted-foreground")).toBe(false);
    expect(inlineStyle.test("<div sty" + "le={{}} />")).toBe(true);
    expect(ours([".tsx", ".css"]).length).toBeGreaterThan(5);
    expect(ours([".tsx"])).toContain("components/AppSidebar.tsx");
    expect(ours([".tsx"]).some((p) => p.startsWith("components/ui/"))).toBe(false);
  });

  test("colours come from the theme", () => {
    expect(findings(ours([".tsx", ".ts", ".css"]), colour)).toEqual([]);
  });

  test("no inline styles in our code", () => {
    expect(findings(ours([".tsx"]), inlineStyle)).toEqual([]);
  });

  test("no raw html in our code", () => {
    expect(findings(ours([".tsx"]), /dangerouslySetInnerHTML/)).toEqual([]);
  });

  test("theme script is not inline", () => {
    const html = readFileSync(join(ui, "index.html"), "utf8");
    const scripts = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)];
    expect(scripts.length).toBeGreaterThan(0);
    for (const [, attributes = "", body = ""] of scripts) {
      expect(body.trim()).toBe("");
      expect(attributes).toMatch(/\bsrc="\/[^"/][^"]*"/);
    }
    expect(html).toMatch(/<script src="\/theme\.js"><\/script>/);
    // It is in <head>, before the app.
    expect(html.indexOf("/theme.js")).toBeLessThan(html.indexOf("</head>"));
    expect(html.indexOf("/theme.js")).toBeLessThan(html.indexOf("/src/main.tsx"));
    expect(html).not.toMatch(/<style\b/i);
    expect(html).not.toMatch(/\son[a-z]+\s*=/i);
    expect(html).not.toMatch(/(?:href|src)="(?:[a-z]+:)?\/\//i);
  });
});

describe("errors of the API", () => {
  const making = /\bnew\s+ApiError\b/;

  test("the scan sees what it should", () => {
    expect(making.test("throw new ApiError" + "(502, x, y)")).toBe(true);
    expect(making.test("if (error instanceof ApiError) return")).toBe(false);
  });

  // An `ApiError` is what the gateway answered. What the console refuses
  // itself is a `ConsoleRefusal`.
  test("no page makes one", () => {
    const pages = allSources().filter(
      (path) => path.startsWith("pages/") && !path.includes(".test."),
    );
    expect(pages.length).toBeGreaterThan(5);
    expect(findings(pages, making)).toEqual([]);
  });
});

/**
 * The area of a page file: the list and the page of one thing are one area,
 * as `Teams` and `TeamDetail`, `Users` and `UserDetail`.
 */
function areaOf(page: string): string {
  return page.replace(/Detail$/, "").replace(/s$/, "");
}

/** The page files that a source imports, by their names without the extension. */
function pagesImportedBy(source: string): string[] {
  const imports = /(?:\bfrom\s*|\bimport\s*\(?\s*)["'](?:@\/pages|\.|\.\.\/pages)\/([A-Za-z]+)["']/g;
  return [...source.matchAll(imports)].flatMap(([, name]) => (name === undefined ? [] : [name]));
}

describe("what depends on what", () => {
  test("no shared component imports a page", () => {
    const shared = allSources().filter(
      (path) => path.startsWith("components/") && !path.includes(".test."),
    );
    expect(shared.length).toBeGreaterThan(10);
    expect(findings(shared, importOf("@/pages"))).toEqual([]);
  });

  test("the scan of the pages sees what it should", () => {
    const quote = '"';
    const from = (path: string) => `import { x } from ${quote}${path}${quote};`;
    expect(pagesImportedBy(from("@/pages/Users"))).toEqual(["Users"]);
    expect(pagesImportedBy(from("./UserDetail"))).toEqual(["UserDetail"]);
    expect(pagesImportedBy(from("../pages/Teams"))).toEqual(["Teams"]);
    expect(pagesImportedBy(`const page = import(${quote}@/pages/Keys${quote});`)).toEqual(["Keys"]);
    expect(pagesImportedBy(from("@/components/YouBadge"))).toEqual([]);
    expect(pagesImportedBy(from("@/lib/id"))).toEqual([]);
    expect(areaOf("Teams")).toBe(areaOf("TeamDetail"));
    expect(areaOf("Users")).toBe(areaOf("UserDetail"));
    expect(areaOf("Keys")).toBe(areaOf("KeyDetail"));
    expect(areaOf("Users")).not.toBe(areaOf("TeamDetail"));
    expect(areaOf("UserDetail")).not.toBe(areaOf("TeamDetail"));
  });

  // What two areas share is in `components/` or `lib/`, so that the pages do
  // not grow into each other.
  test("a page imports another page only of its own area", () => {
    const pages = allSources().filter(
      (path) => /^pages\/[^/]+\.tsx?$/.test(path) && !path.includes(".test."),
    );
    expect(pages.length).toBeGreaterThan(5);
    const across = pages.flatMap((path) => {
      const name = path.replace(/^pages\//, "").replace(/\.tsx?$/, "");
      return pagesImportedBy(readFileSync(join(src, path), "utf8"))
        .filter((imported) => areaOf(imported) !== areaOf(name))
        .map((imported) => `${path} imports pages/${imported}`);
    });
    expect(across).toEqual([]);
    // Within an area it is allowed, and used.
    expect(pagesImportedBy(readFileSync(join(src, "pages/UserDetail.tsx"), "utf8"))).toContain(
      "Users",
    );
    expect(pagesImportedBy(readFileSync(join(src, "pages/TeamDetail.tsx"), "utf8"))).toContain(
      "Teams",
    );
  });
});
