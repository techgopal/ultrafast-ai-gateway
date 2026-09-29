// @vitest-environment node
import { describe, expect, test } from "vitest";
import { libraryEdit } from "../../build/library-edit";

const edit = () =>
  libraryEdit({
    library: "somelib",
    file: /\/somelib\/dist\/index\.js$/,
    find: /^inject\(".*"\);$/m,
    replaceWith: "",
    what: "the style injection call",
  });

describe("build-time edits of library code", () => {
  test("edits the file it targets and leaves others alone", () => {
    const e = edit();
    expect(e.transform('a();\ninject("css");\nb();', "/x/node_modules/somelib/dist/index.js")).toBe(
      "a();\n\nb();",
    );
    expect(e.transform('inject("css");', "/x/node_modules/other/index.js")).toBeNull();
    expect(() => {
      e.assertTargetSeen();
    }).not.toThrow();
  });

  test("fails when the text is not in the target file", () => {
    expect(() => edit().transform("a();", "/x/node_modules/somelib/dist/index.js")).toThrow(
      /somelib: the style injection call was not found in .*somelib\/dist\/index\.js/,
    );
  });

  test("fails when the target file was never seen", () => {
    const e = edit();
    e.transform("a();", "/x/node_modules/other/index.js");
    expect(() => {
      e.assertTargetSeen();
    }).toThrow(/somelib: no file matching .* was part of the build/);
  });
});
