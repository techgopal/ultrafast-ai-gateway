import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "src/schema.d.ts", "node_modules"] },
  ...tseslint.configs.recommended,
);
