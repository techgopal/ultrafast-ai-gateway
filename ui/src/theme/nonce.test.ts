import { readFileSync } from "node:fs";
import { join } from "node:path";
import { getNonce, setNonce } from "get-nonce";
import { afterEach, describe, expect, test, vi } from "vitest";
import { applyNonce, readNonce, styleNonce } from "./nonce";

vi.mock("get-nonce", () => {
  let current: string | undefined;
  return {
    getNonce: vi.fn(() => current),
    setNonce: vi.fn((value: string) => {
      current = value;
    }),
  };
});

const PLACEHOLDER = "__CSP_NONCE__";

function page(head: string): Document {
  return new DOMParser().parseFromString(`<!doctype html><html><head>${head}</head><body></body></html>`, "text/html");
}

afterEach(() => {
  vi.mocked(setNonce).mockClear();
});

describe("nonce", () => {
  test("is the value of the meta tag", () => {
    const doc = page('<meta name="csp-nonce" content="q83vEjRWeJCrze8SNFZ4kA">');
    expect(readNonce(doc)).toBe("q83vEjRWeJCrze8SNFZ4kA");
  });

  test("is read from the page by default", () => {
    const tag = document.createElement("meta");
    tag.setAttribute("name", "csp-nonce");
    tag.setAttribute("content", "AAAAAAAAAAAAAAAAAAAAAA");
    document.head.append(tag);
    try {
      expect(readNonce()).toBe("AAAAAAAAAAAAAAAAAAAAAA");
    } finally {
      tag.remove();
    }
    expect(readNonce()).toBeUndefined();
  });

  test("is undefined for the placeholder, an empty value and a missing tag", () => {
    expect(readNonce(page(`<meta name="csp-nonce" content="${PLACEHOLDER}">`))).toBeUndefined();
    expect(readNonce(page('<meta name="csp-nonce" content="">'))).toBeUndefined();
    expect(readNonce(page('<meta name="csp-nonce">'))).toBeUndefined();
    expect(readNonce(page('<meta name="viewport" content="x">'))).toBeUndefined();
    expect(readNonce(page(""))).toBeUndefined();
  });

  test("is handed to the libraries", () => {
    expect(styleNonce()).toEqual({});
    const doc = page('<meta name="csp-nonce" content="q83vEjRWeJCrze8SNFZ4kA">');
    expect(applyNonce(doc)).toBe("q83vEjRWeJCrze8SNFZ4kA");
    expect(setNonce).toHaveBeenCalledTimes(1);
    expect(setNonce).toHaveBeenCalledWith("q83vEjRWeJCrze8SNFZ4kA");
    expect(getNonce()).toBe("q83vEjRWeJCrze8SNFZ4kA");
    expect(styleNonce()).toEqual({ nonce: "q83vEjRWeJCrze8SNFZ4kA" });
  });

  test("is not handed on when there is none", () => {
    expect(applyNonce(page(`<meta name="csp-nonce" content="${PLACEHOLDER}">`))).toBeUndefined();
    expect(applyNonce(page(""))).toBeUndefined();
    expect(setNonce).not.toHaveBeenCalled();
  });

  test("index.html holds the placeholder exactly once, in the meta tag", () => {
    const html = readFileSync(join(process.cwd(), "index.html"), "utf8");
    expect(html.split(PLACEHOLDER)).toHaveLength(2);
    expect(html).toContain(`<meta name="csp-nonce" content="${PLACEHOLDER}" />`);
  });
});
