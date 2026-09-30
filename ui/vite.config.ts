/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";
import { libraryEdit, type LibraryEditOptions } from "./build/library-edit";

function editedLibrary(name: string, options: LibraryEditOptions): Plugin {
  const edit = libraryEdit(options);
  let building = false;
  return {
    name,
    enforce: "pre",
    configResolved(config) {
      building = config.command === "build";
    },
    transform(code, id) {
      const edited = edit.transform(code, id);
      return edited === null ? null : { code: edited, map: null };
    },
    buildEnd(error) {
      // Only a finished production build has seen every file.
      if (building && error === undefined) edit.assertTargetSeen();
    },
  };
}

// sonner carries its stylesheet inside its script and adds it to <head> as a
// <style> element when the script loads. The Content Security Policy of the
// console allows no such element, so that call is taken out here and the same
// stylesheet, `sonner/dist/styles.css`, is imported from `styles/globals.css`.
const sonnerWithoutStyleInjection = editedLibrary("sonner-without-style-injection", {
  library: "sonner",
  file: /\/sonner\/dist\/index\.m?js$/,
  find: /^__insertCSS\(".*"\);?$/m,
  replaceWith: "",
  what: "the call that injects its stylesheet (__insertCSS)",
});

const gateway = "http://127.0.0.1:3900";

export default defineConfig({
  base: "/",
  plugins: [
    sonnerWithoutStyleInjection,
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
    rollupOptions: {
      output: {
        entryFileNames: "assets/[name]-[hash].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash][extname]",
      },
    },
  },
  server: {
    proxy: { "/api": gateway, "/v1": gateway, "/health": gateway },
  },
  optimizeDeps: { exclude: ["sonner"] },
  test: {
    server: { deps: { inline: ["sonner"] } },
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    css: false,
    // The page tests drive dialogs step by step, and every file imports the
    // whole app. On a busy machine a test that takes a second alone can take
    // more than the 5 seconds a test has by default, and a timeout then says
    // nothing about the code. One value, here: no test file sets its own.
    testTimeout: 15_000,
  },
});
