import { messageOfError } from "@/api/errors";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

interface ErrorStateProps {
  /** What the query failed with. */
  error: unknown;
  /** With it, a Retry button is shown. */
  onRetry?: () => void;
}

/** What a page shows in place of data that could not be loaded. */
export function ErrorState({ error, onRetry }: ErrorStateProps) {
  const message = messageOfError(error);
  if (message === null) return null;
  return (
    <Alert variant="destructive">
      <AlertDescription>
        <p>{message}</p>
        {onRetry === undefined ? null : (
          <Button type="button" variant="outline" className="mt-2 max-md:min-h-11" onClick={onRetry}>
            Retry
          </Button>
        )}
      </AlertDescription>
    </Alert>
  );
}
