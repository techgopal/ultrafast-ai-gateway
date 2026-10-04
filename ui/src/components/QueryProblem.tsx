import { ApiError, messageOfError } from "@/api/errors";
import { ErrorState } from "@/components/ErrorState";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { NotAvailableNote } from "@/components/NotAvailableNote";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";

interface Problem {
  /** What the query failed with. */
  error: unknown;
  /** Asks again. */
  onRetry: () => void;
}

interface OfAPage extends Problem {
  part?: false;
  /**
   * What the page is called: the heading above an error. The page renders
   * no header of its own beside the problem. "Not available" and "not found"
   * take the place of the page, and have their own heading in place of this.
   */
  title: string;
  /** For a detail page: a 404 is the NotFound page. */
  notFound?: boolean;
}

interface OfAPart extends Problem {
  /**
   * The query is the one of a part of a page, a tile or a section, and the
   * rest of the page stays: a 403 is a note in the part, under the heading
   * the part has, and not the "not available" of a whole page.
   */
  part: true;
  title?: never;
  notFound?: never;
}

/**
 * What is shown in place of data that a query could not load: "not
 * available" for a 403, the NotFound page for a 404 of a detail page, the
 * error with Retry for the rest, and nothing for an answer of a session that
 * is over. It is the one place that says which failure is which, for a whole
 * page and, with `part`, for a part of one.
 *
 * For a whole page it is all the page shows, and it brings the one `h1` of
 * the screen in every case: the heading of "not available", the one of "not
 * found", or the title of the page above the error.
 */
export function QueryProblem({
  error,
  onRetry,
  title,
  notFound = false,
  part = false,
}: OfAPage | OfAPart) {
  if (error instanceof ApiError && error.status === 403) {
    return part ? <NotAvailableNote /> : <NotAvailableContent />;
  }
  if (notFound && error instanceof ApiError && error.status === 404) return <NotFoundContent />;
  // A part has its heading; and what says nothing needs none.
  if (title === undefined || messageOfError(error) === null) {
    return <ErrorState error={error} onRetry={onRetry} />;
  }
  return (
    <>
      <PageHeader title={title} />
      <ErrorState error={error} onRetry={onRetry} />
    </>
  );
}
