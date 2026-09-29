import { ApiError } from "@/api/errors";
import { ErrorState } from "@/components/ErrorState";
import { NotAvailable } from "@/pages/NotAvailable";
import { NotFound } from "@/pages/NotFound";

interface QueryProblemProps {
  /** What the query of the page failed with. */
  error: unknown;
  /** Asks again. */
  onRetry: () => void;
  /** For a detail page: a 404 is the NotFound page. */
  notFound?: boolean;
}

/**
 * What a page shows in place of data that its query could not load: "not
 * available" for a 403, the NotFound page for a 404 of a detail page, the
 * error with Retry for the rest, and nothing for an answer of a session that
 * is over.
 */
export function QueryProblem({ error, onRetry, notFound = false }: QueryProblemProps) {
  if (error instanceof ApiError && error.status === 403) return <NotAvailable />;
  if (notFound && error instanceof ApiError && error.status === 404) return <NotFound />;
  return <ErrorState error={error} onRetry={onRetry} />;
}
