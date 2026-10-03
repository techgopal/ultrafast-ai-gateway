import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "src/wasm", "node_modules"] },
  ...tseslint.configs.recommended,
);
