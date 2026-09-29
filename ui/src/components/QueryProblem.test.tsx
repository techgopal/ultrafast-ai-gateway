import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { ApiError, NetworkError, SessionOverError } from "@/api/errors";
import { QueryProblem } from "@/components/QueryProblem";
import { errors, type GatewayError } from "@/test/errors";
import { NOT_AVAILABLE, NOT_FOUND } from "@/test/pages";
import { renderWithApp } from "@/test/render";

function errorOf(fixture: GatewayError): ApiError {
  return new ApiError(fixture.status, fixture.body.error.code, fixture.body.error.message);
}

/** The problem inside the one `main` of a page, as the shell has it. */
function inPage(error: unknown, options: { notFound?: boolean; onRetry?: () => void } = {}) {
  return renderWithApp(
    <main>
      <QueryProblem
        error={error}
        onRetry={options.onRetry ?? (() => undefined)}
        {...(options.notFound === undefined ? {} : { notFound: options.notFound })}
      />
    </main>,
  );
}

describe("query problem", () => {
  test("a 403 is not available: no error, no Retry", async () => {
    await inPage(errorOf(errors.forbidden), { notFound: true });
    expect(screen.getByText(NOT_AVAILABLE)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(screen.queryByText(errors.forbidden.body.error.message)).toBeNull();
    expect(screen.getAllByRole("main")).toHaveLength(1);
  });

  test("a 404 of a detail page is not found, and brings no main of its own", async () => {
    await inPage(errorOf(errors.not_found), { notFound: true });
    expect(screen.getByRole("heading", { name: NOT_FOUND })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(screen.getAllByRole("main")).toHaveLength(1);
  });

  test("a 404 of a list is an error with Retry", async () => {
    await inPage(errorOf(errors.not_found));
    expect(screen.queryByRole("heading", { name: NOT_FOUND })).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent(errors.not_found.body.error.message);
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("a 500 is an error with Retry, and Retry asks again", async () => {
    const retry = vi.fn();
    await inPage(errorOf(errors.internal_error), { notFound: true, onRetry: retry });
    expect(screen.getByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retry).toHaveBeenCalledTimes(1);
  });

  test("a network error is an error with Retry", async () => {
    await inPage(new NetworkError());
    expect(screen.getByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("an answer of a session that is over shows nothing", async () => {
    await inPage(new SessionOverError(), { notFound: true });
    expect(screen.getByRole("main")).toBeEmptyDOMElement();
  });
});
