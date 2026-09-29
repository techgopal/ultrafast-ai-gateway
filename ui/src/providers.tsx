import { useMemo, type ReactNode } from "react";
import { ShellContext, type ShellUser } from "@/components/shell-context";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ThemeProvider } from "@/theme/theme";

interface AppProvidersProps {
  user: ShellUser | null;
  onSignOut: () => void;
  children: ReactNode;
}

/** Everything the pages need around them, in the app and in the tests. */
export function AppProviders({ user, onSignOut, children }: AppProvidersProps) {
  const shell = useMemo(() => ({ user, signOut: onSignOut }), [user, onSignOut]);
  return (
    <ThemeProvider>
      <ShellContext.Provider value={shell}>
        <TooltipProvider>{children}</TooltipProvider>
        <Toaster />
      </ShellContext.Provider>
    </ThemeProvider>
  );
}
