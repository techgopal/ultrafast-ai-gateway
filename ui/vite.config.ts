/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

// sonner carries its stylesheet inside its script and adds it to <head> as a
// <style> element when the script loads. The Content Security Policy of the
// console allows no such element, so that call is taken out here and the same
// stylesheet, `sonner/dist/styles.css`, is imported from `styles/globals.css`.
// The build fails if a new version of sonner no longer has the call in this form.
function sonnerWithoutStyleInjection(): Plugin {
  const call = /^__insertCSS\(".*"\);?$/m;
  return {
    name: "sonner-without-style-injection",
    enforce: "pre",
    transform(code, id) {
      if (!/\/sonner\/dist\/index\.m?js/.test(id)) return null;
      if (!call.test(code)) {
        this.error("sonner: the style injection call was not found; check how this version adds its styles.");
      }
      return { code: code.replace(call, ""), map: null };
    },
  };
}

const gateway = "http://127.0.0.1:3900";

// React's production build names the page of its error codes in the text of its
// errors. Nothing fetches it, but the build is to hold no URL of another host,
// so the scheme is dropped and the text reads "react.dev/errors/<code>".
function withoutReactErrorUrl(): Plugin {
  return {
    name: "without-react-error-url",
    apply: "build",
    renderChunk(code) {
      if (!code.includes("https://react.dev/errors/")) return null;
      return { code: code.replaceAll("https://react.dev/errors/", "react.dev/errors/"), map: null };
    },
  };
}

export default defineConfig({
  base: "/",
  plugins: [sonnerWithoutStyleInjection(), withoutReactErrorUrl(), react(), tailwindcss()],
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
