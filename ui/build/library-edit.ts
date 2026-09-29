// An edit of a library's code at build time, which fails loudly when the
// library has changed so that the edit no longer applies.

export interface LibraryEditOptions {
  /** The library, as named in messages. */
  library: string;
  /** Matches the path of the one file of the library that is edited. */
  file: RegExp;
  /** The text to take out. */
  find: RegExp | string;
  replaceWith: string;
  /** What the text is, for the message: "the style injection call". */
  what: string;
}

export interface LibraryEdit {
  /** The edited code, or `null` for a file that is not the target. Throws if the text is missing. */
  transform(code: string, id: string): string | null;
  /** Throws if the target file never came by. */
  assertTargetSeen(): void;
}

export function libraryEdit(options: LibraryEditOptions): LibraryEdit {
  const { library, file, find, replaceWith, what } = options;
  let seen = false;
  return {
    transform(code, id) {
      const path = id.split("?")[0] ?? id;
      if (!file.test(path)) return null;
      seen = true;
      const found = typeof find === "string" ? code.includes(find) : new RegExp(find.source, find.flags.replace("g", "")).test(code);
      if (!found) {
        throw new Error(
          `${library}: ${what} was not found in ${path}. This version of ${library} has changed; check it and update the edit in vite.config.ts.`,
        );
      }
      return typeof find === "string"
        ? code.replaceAll(find, replaceWith)
        : code.replace(find, replaceWith);
    },
    assertTargetSeen() {
      if (seen) return;
      throw new Error(
        `${library}: no file matching ${String(file)} was part of the build, so ${what} could not be edited. This version of ${library} has moved its files; check it and update the edit in vite.config.ts.`,
      );
    },
  };
}
