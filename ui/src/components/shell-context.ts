import { createContext } from "react";

/** What the shell shows of the signed-in user. The session code fills it. */
export interface ShellUser {
  name: string;
  role: "admin" | "member";
  teams: readonly { name: string }[];
  /** Whether the audit log is theirs to read: `can(me, { type: "viewAudit" })`. */
  mayViewAudit: boolean;
  /** Whether the settings are theirs to change: `can(me, { type: "manageSettings" })`. */
  maySetSettings: boolean;
}

/**
 * True below the shell, whose `main` holds the page. A screen that is shown
 * both inside the shell and without it (the error screen of the router)
 * brings a `main` of its own only where this is false.
 */
export const InShell = createContext(false);
