import init, * as bindings from "./wasm/ultrafast_client_wasm.js";
import { WASM_BASE64 } from "./wasm/inline.js";
import { MalformedError } from "./errors.js";

export type Wasm = typeof bindings;

/** What `initWasm` accepts: a compiled module (for runtimes that forbid compiling), bytes, a URL or a `Response`. */
export type WasmSource = WebAssembly.Module | BufferSource | URL | string | Request | Response | Promise<Response>;

let loading: Promise<Wasm> | undefined;

function bytes(): Uint8Array {
  const bin = atob(WASM_BASE64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** The cause's name and message: the module's own text, never a key. */
function hint(e: unknown): string {
  if (e instanceof Error) return `: ${e.name}: ${e.message}`;
  return "";
}

function load(source: () => WasmSource): Promise<Wasm> {
  const p = (async () => {
    try {
      await init({ module_or_path: source() });
      return bindings;
    } catch (e) {
      loading = undefined;
      throw new MalformedError({
        kind: "malformed",
        retryable: false,
        message: `the WebAssembly module could not be loaded${hint(e)}`,
      });
    }
  })();
  loading = p;
  return p;
}

/**
 * Loads the wire module from `source`. Call it once, before the first call,
 * on runtimes that forbid compiling WebAssembly at run time (Cloudflare
 * Workers, Vercel Edge, a strict Content-Security-Policy): pass the
 * precompiled `WebAssembly.Module` the platform gives you. Without a call,
 * the first use compiles the copy embedded in the package. A no-op once a
 * load has started.
 */
export async function initWasm(source: WasmSource): Promise<void> {
  if (loading) {
    await loading;
    return;
  }
  await load(() => source);
}

/**
 * The module, loaded once from the bytes embedded in the package (no file or
 * URL access, so it works in every runtime that may compile WebAssembly).
 * Asynchronous because browsers refuse to compile large modules
 * synchronously on the main thread.
 */
export function wasm(): Promise<Wasm> {
  return loading ?? load(bytes);
}
