import "./styles/globals.css";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppProviders } from "./providers";
import { createAppRouter } from "./router";

const router = createAppRouter();
const root = document.getElementById("root");
if (root === null) throw new Error("index.html has no #root element.");

// The session code supplies the user and the sign-out action.
const signOut = () => undefined;

createRoot(root).render(
  <StrictMode>
    <AppProviders user={null} onSignOut={signOut}>
      <RouterProvider router={router} />
    </AppProviders>
  </StrictMode>,
);
