import { getNonce, setNonce } from "get-nonce";

// The gateway writes a fresh nonce into this tag with every page it serves and
// names the same nonce in the Content Security Policy. A <style> element that
// a library adds while the app runs is applied only when it carries it.
const META = 'meta[name="csp-nonce"]';
// What the tag holds when the gateway did not serve the page (the Vite dev server).
const PLACEHOLDER = "__CSP_NONCE__";

export function readNonce(page: Document = document): string | undefined {
  const value = page.querySelector(META)?.getAttribute("content")?.trim();
  if (value === undefined || value === "" || value === PLACEHOLDER) return undefined;
  return value;
}

/** Hands the nonce of the page to the libraries that add styles. Call before rendering. */
export function applyNonce(page: Document = document): string | undefined {
  const nonce = readNonce(page);
  if (nonce !== undefined) setNonce(nonce);
  return nonce;
}

/** The `nonce` prop for a component that renders a <style> element. */
export function styleNonce(): { nonce?: string } {
  const nonce = getNonce();
  return typeof nonce === "string" && nonce !== "" ? { nonce } : {};
}
