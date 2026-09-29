import { ApiError } from "@/api/errors";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

/** For a page that the account may not see. It is not an error. */
export function NotAvailable() {
  return (
    <div className="flex flex-col gap-2">
      <h1 className="text-2xl font-semibold">Not available</h1>
      <p className="text-muted-foreground">This page is not available to your account.</p>
    </div>
  );
}

/** What a page shows in place of its content when its data could not be loaded. */
export function PageProblem({ error }: { error: unknown }) {
  if (error instanceof ApiError && error.status === 403) return <NotAvailable />;
  const message =
    error instanceof Error && error.message !== "" ? error.message : "Something went wrong.";
  return (
    <Alert variant="destructive">
      <AlertTitle>This page could not be loaded</AlertTitle>
      <AlertDescription>{message}</AlertDescription>
    </Alert>
  );
}
