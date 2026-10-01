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

  // Tailwind makes a class of every word that names one in the files it
  // reads; the browser tests are no part of the console.
  test("the stylesheet is made without the browser tests", () => {
    const css = readFileSync(join(src, "styles/globals.css"), "utf8");
    expect(css).toMatch(/^@source not "\.\.\/\.\.\/e2e";$/m);
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

/** The sources of the app that are no test and no generated component. */
function written(): string[] {
  return ours([".ts", ".tsx"]).filter(
    (path) => !path.includes(".test.") && !path.startsWith("test/"),
  );
}

describe("dialogs and forms", () => {
  // Any use of it: called, taken apart from the form, called if it is there, handed on.
  const sending = /\bhandleSubmit\b/;

  test("the scans see what they should", () => {
    const quote = '"';
    const from = (path: string) => `} from ${quote}${path}${quote};`;
    expect(importOf("@/components/ui/dialog").test(from("@/components/ui/dialog"))).toBe(true);
    expect(importOf("@/components/ui/dialog").test(from("@/components/ui/alert-dialog"))).toBe(false);
    expect(importOf("@/components/ui/alert-dialog").test(from("@/components/ui/alert-dialog"))).toBe(
      true,
    );
    expect(importOf("@/components/ui/dialog").test(from("@/components/FormDialog"))).toBe(false);
    expect(sending.test("void form" + ".handleSubmit();")).toBe(true);
    // Also taken apart, called if it is there, or handed on.
    expect(sending.test("const { handle" + "Submit } = form;")).toBe(true);
    expect(sending.test("handle" + "Submit();")).toBe(true);
    expect(sending.test("form.handle" + "Submit?.();")).toBe(true);
    expect(sending.test("onClick={form.handle" + "Submit}")).toBe(true);
    expect(sending.test("<form onSubmit={onSubmit}>")).toBe(false);
    expect(sending.test("const onSubmit = useSubmit(form);")).toBe(false);
    expect(sending.test("const handleSubmitted = done;")).toBe(false);
    expect(written()).toContain("pages/Users.tsx");
    expect(written()).toContain("components/FormDialog.tsx");
    expect(written().some((path) => path.includes(".test."))).toBe(false);
  });

  // A dialog is one of three: `FormDialog` for a form, `ConfirmDialog` for a
  // question, `SecretDialog` for what is shown once. What a dialog does while
  // its request runs is written once, in them.
  test("only the three dialogs of components/ are built from the dialog primitives", () => {
    const users = (name: string) =>
      written().filter((path) => importOf(name).test(readFileSync(join(src, path), "utf8")));
    expect(users("@/components/ui/dialog")).toEqual([
      "components/FormDialog.tsx",
      "components/SecretDialog.tsx",
    ]);
    expect(users("@/components/ui/alert-dialog")).toEqual([
      "components/ConfirmDialog.tsx",
      "components/SecretDialog.tsx",
    ]);
    // The primitives are also in the package itself: only the field takes one from there.
    expect(users("radix-ui")).toEqual(["components/Field.tsx"]);
  });

  // `useSubmit` of `components/form.ts` sends a form, refuses a second
  // submit while the first one runs, and keeps the dialog of the form from
  // being left from the submit on. A form that called `handleSubmit` itself
  // would be without that.
  test("a form is sent by useSubmit, and by nothing else", () => {
    const where = findings(written(), sending).map((finding) => finding.replace(/:\d+: .*$/, ""));
    expect([...new Set(where)]).toEqual(["components/form.ts"]);
  });
});

describe("class names that are shared", () => {
  const declaring = /^(?:export\s+)?const\s+control\s*=/;
  // The class of `control` written out, in any quotes.
  const writtenOut = /(["'`])min-h-11 md:min-h-8\1/;

  test("the scan sees what it should", () => {
    const quote = '"';
    expect(declaring.test(`const control = ${quote}min-h-11 md:min-h-8${quote};`)).toBe(true);
    expect(declaring.test(`export const control = ${quote}min-h-11 w-full${quote};`)).toBe(true);
    expect(declaring.test(`import { control } from ${quote}@/components/classes${quote};`)).toBe(
      false,
    );
    expect(declaring.test("for (const control of [filter()]) {")).toBe(false);
    expect(writtenOut.test(`className=${quote}min-h-11 md:min-h-8${quote}`)).toBe(true);
    expect(writtenOut.test("const size = 'min-h-11 md:min-h-8';")).toBe(true);
    expect(writtenOut.test("className={control}")).toBe(false);
    expect(writtenOut.test(`className=${quote}min-h-11 w-full${quote}`)).toBe(false);
  });

  // The class of a control that is high enough to touch is said once, in
  // `components/classes.ts`. The pages before the sign-in have their own,
  // which is as wide as their form.
  test("no file declares the class of a control for itself", () => {
    expect(
      findings(written(), declaring).map((finding) => finding.replace(/:\d+: .*$/, "")),
    ).toEqual(["components/AuthForm.tsx", "components/classes.ts"]);
  });

  // Nor writes it out: it is `control` of `components/classes.ts` everywhere.
  test("no file writes the class of a control out", () => {
    expect(
      findings(written(), writtenOut).map((finding) => finding.replace(/: .*$/, "")),
    ).toEqual(["components/classes.ts:4"]);
  });
});

/**
 * The checkboxes and radios of a source that are not inside a `<Label` whose
 * class makes it high enough to touch, by their lines. The opening tag of the
 * label is on one line.
 */
function choicesOutsideTheirLabel(source: string): number[] {
  const lineOf = (index: number) => source.slice(0, index).split("\n").length;
  return [...source.matchAll(/<(?:Checkbox|RadioGroupItem)\b/g)].flatMap((match) => {
    const before = source.slice(0, match.index);
    const open = before.lastIndexOf("<Label");
    const inside = open > before.lastIndexOf("</Label>");
    const tag = source.slice(open, source.indexOf("\n", open));
    const high = /className=\{control\}|className="min-h-11[ "]/.test(tag);
    return inside && high ? [] : [lineOf(match.index)];
  });
}

describe("checkboxes and radios", () => {
  test("the scan sees what it should", () => {
    const row = (label: string) => `${label}\n  <RadioGroupItem id="a" value="a" />\n  A\n</Label>`;
    expect(choicesOutsideTheirLabel(row('<Label htmlFor="a" className={control}>'))).toEqual([]);
    expect(choicesOutsideTheirLabel(row('<Label htmlFor="a" className="min-h-11">'))).toEqual([]);
    expect(choicesOutsideTheirLabel(row('<Label htmlFor="a">'))).toEqual([2]);
    expect(choicesOutsideTheirLabel(row('<Label htmlFor="a" className="min-h-11x">'))).toEqual([2]);
    expect(
      choicesOutsideTheirLabel('<div className={control}>\n  <Checkbox id="b" />\n  <Label htmlFor="b">B</Label>\n</div>'),
    ).toEqual([2]);
    expect(
      choicesOutsideTheirLabel('<Label className={control}>A</Label>\n<RadioGroupItem value="b" />'),
    ).toEqual([2]);
  });

  // On a narrow screen the box of a checkbox or a radio is 16 px, and the
  // text beside it 14 px high: what is touched is the label, which holds its
  // control and is 44 px high (`control`). A row around them that is 44 px
  // high is no target: a touch beside the text does nothing.
  test("each is inside its label, which is high enough to touch", () => {
    const loose = written().flatMap((path) =>
      choicesOutsideTheirLabel(readFileSync(join(src, path), "utf8")).map((line) => `${path}:${String(line)}`),
    );
    expect(loose).toEqual([]);
    // The rule has something to look at.
    const choices = written().filter((path) =>
      /<(?:Checkbox|RadioGroupItem)\b/.test(readFileSync(join(src, path), "utf8")),
    );
    expect(choices.length).toBeGreaterThanOrEqual(6);
  });
});

/** The files directly in `pages/` that are no tests, by their names without the extension. */
function pageFiles(): string[] {
  return allSources()
    .filter((path) => /^pages\/[^/]+\.tsx?$/.test(path) && !path.includes(".test."))
    .map((path) => path.replace(/^pages\//, "").replace(/\.tsx?$/, ""));
}

/**
 * The area of a page file: the list and the page of one thing are one area,
 * as `Teams` and `TeamDetail`, `Users` and `UserDetail`.
 *
 * A page that grew is split into files of the same folder. A file that holds
 * a part of a page has the name of that page and then a name of its own,
 * which begins with a capital letter: `KeysCreate` and `KeysFilters` are
 * parts of `Keys`, `TeamDetailAddMember` is one of `TeamDetail`. A part is of
 * the area of its page, also a part of a part. `files` are the page files
 * there are.
 */
function areaOf(page: string, files: readonly string[] = pageFiles()): string {
  const [whole = page] = files
    .filter((other) => page.startsWith(other) && /^[A-Z]/.test(page.slice(other.length)))
    // Of two pages it begins with, the longer one is its page.
    .sort((a, b) => b.length - a.length);
  // That page can be a part itself: `KeysCreateOwner` is a part of `KeysCreate`.
  if (whole !== page) return areaOf(whole, files);
  return page.replace(/Detail$/, "").replace(/s$/, "");
}

/**
 * The page files that a source imports, by what follows `pages/` in the
 * path. For a file directly in `pages/` that is its name. A path that goes
 * deeper or has an extension is given as it is written: it is the name of no
 * area, so the rule of the areas reports it and does not pass it by.
 */
function pagesImportedBy(source: string): string[] {
  const imports = /(?:\bfrom\s*|\bimport\s*\(?\s*)["'](?:@\/pages|\.|\.\.\/pages)\/([\w./-]+)["']/g;
  return [...source.matchAll(imports)].flatMap(([, name]) => (name === undefined ? [] : [name]));
}

/** The sources that lie below a folder of `pages/`. Tests are none. */
function belowAFolderOfPages(paths: string[]): string[] {
  return paths.filter((path) => /^pages\/[^/]+\//.test(path) && !path.includes(".test."));
}

describe("what depends on what", () => {
  // An import of `components/` or `pages/`, by the alias or by a path that goes up.
  const ofComponentsOrPages =
    /(?:\bfrom\s*|\bimport\s*\(?\s*)["'](?:@\/|(?:\.\.\/)+)(?:components|pages)(?:\/[^"']*)?["']/;

  test("the scan of lib sees what it should", () => {
    const quote = '"';
    const from = (path: string) => `import { x } from ${quote}${path}${quote};`;
    expect(ofComponentsOrPages.test(from("@/components/ErrorState"))).toBe(true);
    expect(ofComponentsOrPages.test(from("../components/ErrorState"))).toBe(true);
    expect(ofComponentsOrPages.test(from("@/pages/Keys"))).toBe(true);
    expect(ofComponentsOrPages.test(`const page = import(${quote}../pages/Keys${quote});`)).toBe(true);
    expect(ofComponentsOrPages.test(`export { x } from ${quote}@/components/form${quote};`)).toBe(true);
    expect(ofComponentsOrPages.test(from("@/api/errors"))).toBe(false);
    expect(ofComponentsOrPages.test(from("@/lib/id"))).toBe(false);
    expect(ofComponentsOrPages.test(from("./expiry"))).toBe(false);
    expect(ofComponentsOrPages.test(from("@/components-extra"))).toBe(false);
  });

  // `lib/` holds rules that are pure functions. What they need of the errors
  // is in `api/errors.ts`; a component, with its markup, is no part of them.
  test("no file of lib imports from components or pages", () => {
    const lib = written().filter((path) => path.startsWith("lib/"));
    expect(lib.length).toBeGreaterThan(5);
    expect(findings(lib, ofComponentsOrPages)).toEqual([]);
  });

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
    // A path that goes deeper, or is written in another way, is seen as well.
    // It is the name of no area, so the rule reports it.
    expect(pagesImportedBy(from("@/pages/teams/AddMember"))).toEqual(["teams/AddMember"]);
    expect(pagesImportedBy(from("./team-parts/AddMember"))).toEqual(["team-parts/AddMember"]);
    expect(pagesImportedBy(from("../pages/teams/index"))).toEqual(["teams/index"]);
    expect(pagesImportedBy(from("./Users.tsx"))).toEqual(["Users.tsx"]);
    expect(pagesImportedBy(from("./team_detail-2"))).toEqual(["team_detail-2"]);
    expect(areaOf("teams/AddMember")).not.toBe(areaOf("TeamDetail"));
    expect(areaOf("teams/Teams")).not.toBe(areaOf("TeamDetail"));
    expect(areaOf("Users.tsx")).not.toBe(areaOf("UserDetail"));
    expect(
      belowAFolderOfPages([
        "pages/Teams.tsx",
        "pages/teams.test.tsx",
        "pages/teams/AddMember.tsx",
        "pages/teams/parts/names.ts",
        "pages/teams/add.test.tsx",
        "components/ui/button.tsx",
        "lib/id.ts",
      ]),
    ).toEqual(["pages/teams/AddMember.tsx", "pages/teams/parts/names.ts"]);
    expect(areaOf("Teams")).toBe(areaOf("TeamDetail"));
    expect(areaOf("Users")).toBe(areaOf("UserDetail"));
    expect(areaOf("Keys")).toBe(areaOf("KeyDetail"));
    expect(areaOf("Users")).not.toBe(areaOf("TeamDetail"));
    expect(areaOf("UserDetail")).not.toBe(areaOf("TeamDetail"));
    // A part of a page is named after the page, and is of its area.
    const files = ["Keys", "KeysCreate", "KeysFilters", "Teams", "TeamDetail", "TeamDetailAddMember"];
    expect(areaOf("KeysCreate", files)).toBe(areaOf("Keys", files));
    expect(areaOf("KeysFilters", files)).toBe(areaOf("KeysCreate", files));
    expect(areaOf("TeamDetailAddMember", files)).toBe(areaOf("TeamDetail", files));
    expect(areaOf("TeamDetailAddMember", files)).toBe(areaOf("Teams", files));
    expect(areaOf("KeysCreate", files)).not.toBe(areaOf("TeamDetailAddMember", files));
    // A part of a part is of the area of the page as well.
    const deeper = [...files, "KeysCreateOwner", "TeamDetailAddMemberList"];
    expect(areaOf("KeysCreateOwner", deeper)).toBe(areaOf("KeysCreate", deeper));
    expect(areaOf("KeysCreateOwner", deeper)).toBe(areaOf("Keys", deeper));
    expect(areaOf("TeamDetailAddMemberList", deeper)).toBe(areaOf("Teams", deeper));
    expect(areaOf("KeysCreateOwner", deeper)).not.toBe(areaOf("TeamDetailAddMemberList", deeper));
    // Only of a page that is there, and only when its own name follows with a capital letter.
    expect(areaOf("KeysCreate", ["Teams", "KeysCreate"])).not.toBe(areaOf("Keys", ["Teams"]));
    expect(areaOf("Keyseeker", files)).not.toBe(areaOf("Keys", files));
    expect(areaOf("Keys.tsx", files)).not.toBe(areaOf("Keys", files));
    expect(areaOf("Keys/Create", files)).not.toBe(areaOf("Keys", files));
    expect(areaOf("keysCreate", files)).not.toBe(areaOf("Keys", files));
    // Two pages whose names begin alike are no parts of each other.
    expect(areaOf("NotFound", ["NotFound", "NotAvailable"])).not.toBe(
      areaOf("NotAvailable", ["NotFound", "NotAvailable"]),
    );
    expect(areaOf("Users", ["User", "Users"])).toBe(areaOf("User", ["User", "Users"]));
    // The files that are there are what the rule asks by default.
    expect(pageFiles()).toContain("TeamDetail");
    expect(pageFiles()).toContain("Keys");
    expect(pageFiles().some((name) => name.includes("."))).toBe(false);
  });

  // The rule of the areas reads the files directly in `pages/`, and takes the
  // name of a file for its area. Teach it folders before adding one: a page
  // below a folder would be read by nothing.
  test("no page lies below a folder of pages", () => {
    expect(belowAFolderOfPages(allSources())).toEqual([]);
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
