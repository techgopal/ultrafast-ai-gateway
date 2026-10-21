// tsc emits inline.ts; the wasm-bindgen glue (plain JS and its types) and the .wasm itself (for initWasm on edge runtimes) is copied beside it.
import { copyFileSync } from "node:fs";
for (const f of ["ufaigw_client_wasm.js", "ufaigw_client_wasm.d.ts", "ufaigw_client_wasm_bg.wasm"]) {
  copyFileSync(`src/wasm/${f}`, `dist/wasm/${f}`);
}
