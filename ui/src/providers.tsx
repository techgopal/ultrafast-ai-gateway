import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import { useMemo, type ReactNode } from "react";
import { createQueryClient } from "@/api/queries";
import { ShellContext, type ShellUser } from "@/components/shell-context";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ThemeProvider } from "@/theme/theme";

interface AppProvidersProps {
  user: ShellUser | null;
  onSignOut: () => void;
  /** Default: the one client of the app. A test gives its own. */
  queryClient?: QueryClient;
  children: ReactNode;
}

// Made once, when the app loads.
const appQueryClient = createQueryClient();

/** Everything the pages need around them, in the app and in the tests. */
export function AppProviders({
  user,
  onSignOut,
  queryClient = appQueryClient,
  children,
}: AppProvidersProps) {
  const shell = useMemo(() => ({ user, signOut: onSignOut }), [user, onSignOut]);
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <ShellContext.Provider value={shell}>
          <TooltipProvider>{children}</TooltipProvider>
          <Toaster />
        </ShellContext.Provider>
      </ThemeProvider>
    </QueryClientProvider>
  );
}
