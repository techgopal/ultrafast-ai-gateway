import init, * as bindings from "./wasm/ultrafast_client_wasm.js";
import { WASM_BASE64 } from "./wasm/inline.js";
import { MalformedError } from "./errors.js";

export type Wasm = typeof bindings;

let loading: Promise<Wasm> | undefined;

function bytes(): Uint8Array {
  const bin = atob(WASM_BASE64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/**
 * The compiled module, loaded once from the bytes embedded in the package
 * (no file or URL access, so it works in every runtime). Loading is
 * asynchronous because browsers refuse to compile large modules
 * synchronously on the main thread.
 */
export function wasm(): Promise<Wasm> {
  loading ??= init({ module_or_path: bytes() }).then(
    () => bindings,
    () => {
      loading = undefined;
      throw new MalformedError({
        kind: "malformed",
        retryable: false,
        message: "the WebAssembly module could not be loaded",
      });
    },
  );
  return loading;
}
