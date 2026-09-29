import { createContext, useContext } from "react";

/** What the shell shows of the signed-in user. The session code fills it. */
export interface ShellUser {
  name: string;
  role: "admin" | "member";
  teams: readonly { name: string }[];
}

export interface ShellContextValue {
  /** `null` while nobody is signed in. */
  user: ShellUser | null;
  signOut: () => void;
}

export const ShellContext = createContext<ShellContextValue>({
  user: null,
  signOut: () => undefined,
});

export function useShell(): ShellContextValue {
  return useContext(ShellContext);
}
