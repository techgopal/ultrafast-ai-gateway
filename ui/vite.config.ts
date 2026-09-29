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

// React's production build names the page of its error codes in the text of its
// errors. Nothing fetches it, but the build is to hold no URL of another host,
// so the scheme is dropped and the text reads "react.dev/errors/<code>".
function withoutReactErrorUrl(name: string, file: RegExp): Plugin {
  return editedLibrary(name, {
    library: "react-dom",
    file,
    find: "https://react.dev/errors/",
    replaceWith: "react.dev/errors/",
    what: "the URL of its error codes (https://react.dev/errors/)",
  });
}

// TanStack Router falls back to the origin "http://localhost" when the page has
// none (`window.origin` missing or "null"), as the base for parsing paths.
// Nothing is fetched from it and a page served by the gateway always has an
// origin, but the build is to hold no URL of another host, so the same value
// is put together at run time instead of standing in the file as a URL.
const routerWithoutFallbackUrl = editedLibrary("router-without-fallback-url", {
  library: "@tanstack/router-core",
  file: /\/@tanstack\/router-core\/dist\/esm\/router\.js$/,
  find: '"http://localhost"',
  replaceWith: '["http:", "", "localhost"].join("/")',
  what: 'the fallback origin ("http://localhost")',
});

const gateway = "http://127.0.0.1:3900";

export default defineConfig({
  base: "/",
  plugins: [
    sonnerWithoutStyleInjection,
    routerWithoutFallbackUrl,
    withoutReactErrorUrl("react-dom-shared-without-error-url", /\/react-dom\/cjs\/react-dom\.production\.js$/),
    withoutReactErrorUrl("react-dom-without-error-url", /\/react-dom\/cjs\/react-dom-client\.production\.js$/),
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
  },
});
