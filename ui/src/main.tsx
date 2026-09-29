import "./styles/globals.css";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppProviders } from "./providers";
import { createAppRouter } from "./router";
import { applyNonce } from "./theme/nonce";

// Before anything renders: the libraries read the nonce when they add a style.
applyNonce();

const router = createAppRouter();
const root = document.getElementById("root");
if (root === null) throw new Error("index.html has no #root element.");

createRoot(root).render(
  <StrictMode>
    <AppProviders>
      <RouterProvider router={router} />
    </AppProviders>
  </StrictMode>,
);
