// tsc emits inline.ts; the wasm-bindgen glue (plain JS and its types) is copied beside it.
import { copyFileSync } from "node:fs";
for (const f of ["ultrafast_client_wasm.js", "ultrafast_client_wasm.d.ts"]) {
  copyFileSync(`src/wasm/${f}`, `dist/wasm/${f}`);
}
