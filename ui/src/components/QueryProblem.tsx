import { ApiError } from "@/api/errors";
import { ErrorState } from "@/components/ErrorState";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { NotAvailableNote } from "@/components/NotAvailableNote";
import { NotFoundContent } from "@/components/NotFoundContent";

interface Problem {
  /** What the query failed with. */
  error: unknown;
  /** Asks again. */
  onRetry: () => void;
}

interface PageProblem extends Problem {
  part?: false;
  /** For a detail page: a 404 is the NotFound page. */
  notFound?: boolean;
}

interface PartProblem extends Problem {
  /**
   * The query is the one of a part of a page, a tile or a section, and the
   * rest of the page stays: a 403 is a note in the part, under the heading
   * the part has, and not the "not available" of a whole page.
   */
  part: true;
  notFound?: never;
}

/**
 * What is shown in place of data that a query could not load: "not
 * available" for a 403, the NotFound page for a 404 of a detail page, the
 * error with Retry for the rest, and nothing for an answer of a session that
 * is over. It is the one place that says which failure is which, for a whole
 * page and, with `part`, for a part of one.
 */
export function QueryProblem({
  error,
  onRetry,
  notFound = false,
  part = false,
}: PageProblem | PartProblem) {
  if (error instanceof ApiError && error.status === 403) {
    return part ? <NotAvailableNote /> : <NotAvailableContent />;
  }
  if (notFound && error instanceof ApiError && error.status === 404) return <NotFoundContent />;
  return <ErrorState error={error} onRetry={onRetry} />;
}
