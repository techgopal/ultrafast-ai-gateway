import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { createQueryClient } from "@/api/queries";
import { SessionProvider } from "@/auth/session";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ThemeProvider } from "@/theme/theme";

interface AppProvidersProps {
  /** Default: the one client of the app. A test gives its own. */
  queryClient?: QueryClient;
  children: ReactNode;
}

// Made once, when the app loads.
const appQueryClient = createQueryClient();

/** Everything the pages need around them, in the app and in the tests. */
export function AppProviders({ queryClient = appQueryClient, children }: AppProvidersProps) {
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <SessionProvider>
          <TooltipProvider>{children}</TooltipProvider>
          <Toaster />
        </SessionProvider>
      </ThemeProvider>
    </QueryClientProvider>
  );
}
