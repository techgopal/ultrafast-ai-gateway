import { ApiError } from "@/api/errors";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

/** The whole screen, for where there is no shell. Inside the shell: `NotAvailableContent`. */
export function NotAvailable() {
  return (
    <main className="flex min-h-svh flex-col p-6">
      <NotAvailableContent />
    </main>
  );
}

/**
 * What a route shows in place of its content when its data could not be
 * loaded. It is shown inside the shell, so it brings no `main`.
 */
export function PageProblem({ error }: { error: unknown }) {
  if (error instanceof ApiError && error.status === 403) return <NotAvailableContent />;
  const message =
    error instanceof Error && error.message !== "" ? error.message : "Something went wrong.";
  return (
    <Alert variant="destructive">
      <AlertTitle>This page could not be loaded</AlertTitle>
      <AlertDescription>{message}</AlertDescription>
    </Alert>
  );
}
