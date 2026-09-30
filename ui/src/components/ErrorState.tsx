import { ApiError, ConsoleRefusal, NetworkError, SessionOverError } from "@/api/errors";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

export const SOMETHING_WENT_WRONG = "Something went wrong.";

/**
 * What to tell the user about a failure, or `null` when there is nothing to
 * tell: an answer of a session that is over says nothing to who is signed in
 * now. Only the message of the gateway, the message of the network error and
 * what the console itself refuses are shown; whatever else was thrown may
 * hold what is not for the user.
 */
export function messageOfError(error: unknown): string | null {
  if (error instanceof SessionOverError) return null;
  if (
    error instanceof ApiError ||
    error instanceof NetworkError ||
    error instanceof ConsoleRefusal
  ) {
    return error.message;
  }
  return SOMETHING_WENT_WRONG;
}

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
