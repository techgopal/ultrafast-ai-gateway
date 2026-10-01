import { useContext } from "react";
import { ApiError } from "@/api/errors";
import { ErrorState } from "@/components/ErrorState";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { PageHeader } from "@/components/PageHeader";
import { InShell } from "@/components/shell-context";

export const PAGE_NOT_LOADED = "This page could not be loaded";

/**
 * What a route shows in place of its content when it failed: "not
 * available" for a 403, else the error under the one `h1` of the screen. What
 * is said of the error is what `messageOfError` lets through, never the text
 * of whatever was thrown. Inside the shell it is in the shell's `main`; where
 * there is no shell it brings its own.
 */
export function PageProblem({ error }: { error: unknown }) {
  const inShell = useContext(InShell);
  const content =
    error instanceof ApiError && error.status === 403 ? (
      <NotAvailableContent />
    ) : (
      <>
        <PageHeader title={PAGE_NOT_LOADED} />
        <ErrorState error={error} />
      </>
    );
  if (inShell) return content;
  return <main className="flex min-h-svh flex-col gap-6 bg-background p-4 md:p-6">{content}</main>;
}
