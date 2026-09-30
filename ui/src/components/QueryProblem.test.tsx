import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { ApiError, NetworkError, SessionOverError } from "@/api/errors";
import { QueryProblem } from "@/components/QueryProblem";
import { errors, type GatewayError } from "@/test/errors";
import { NOT_AVAILABLE, NOT_FOUND } from "@/test/pages";
import { renderWithApp } from "@/test/render";

const PART_NOT_AVAILABLE = "Not available to your account.";

/** The `h1` of the screen, as their texts. */
function h1(): string[] {
  return screen.queryAllByRole("heading", { level: 1 }).map((heading) => heading.textContent);
}

function errorOf(fixture: GatewayError): ApiError {
  return new ApiError(fixture.status, fixture.body.error.code, fixture.body.error.message);
}

/** The problem inside the one `main` of a page, as the shell has it. The page is called "Users". */
function inPage(error: unknown, options: { notFound?: boolean; onRetry?: () => void } = {}) {
  return renderWithApp(
    <main>
      <QueryProblem
        title="Users"
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

  // One `h1` in every state: the problem is all the page shows, so it brings the heading.
  test.each([
    ["a 500", errorOf(errors.internal_error)],
    ["a 404 of a list", errorOf(errors.not_found)],
    ["a network error", new NetworkError()],
    ["what is no error of the API", new Error("whatever")],
  ])("%s is shown under the title of the page, which is the one h1", async (_, error) => {
    await inPage(error);
    expect(h1()).toEqual(["Users"]);
    expect(screen.getAllByRole("heading")).toHaveLength(1);
    // The heading comes first.
    const alert = screen.getByRole("alert");
    expect(
      screen.getByRole("heading").compareDocumentPosition(alert) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
  });

  test("not available takes the place of the page: its heading is the one h1, the title is not shown", async () => {
    await inPage(errorOf(errors.forbidden), { notFound: true });
    expect(h1()).toEqual(["Not available"]);
    expect(screen.queryByRole("heading", { name: "Users" })).toBeNull();
  });

  test("not found takes the place of the page: its heading is the one h1, the title is not shown", async () => {
    await inPage(errorOf(errors.not_found), { notFound: true });
    expect(h1()).toEqual([NOT_FOUND]);
    expect(screen.queryByRole("heading", { name: "Users" })).toBeNull();
  });
});

/** The problem of a part of a page: the page has its heading, the part its own. */
function inPart(error: unknown, onRetry: () => void = () => undefined) {
  return renderWithApp(
    <main>
      <h1>Account</h1>
      <section aria-label="Access tokens">
        <h2>Access tokens</h2>
        <QueryProblem part error={error} onRetry={onRetry} />
      </section>
    </main>,
  );
}

function headings(): string[] {
  return screen.getAllByRole("heading").map((heading) => heading.textContent);
}

describe("query problem of a part of a page", () => {
  test("a 403 is a note that the part is not available: no heading, no error, no Retry", async () => {
    await inPart(errorOf(errors.forbidden));
    const part = screen.getByRole("region", { name: "Access tokens" });
    expect(part).toHaveTextContent(PART_NOT_AVAILABLE);
    // The page and the part keep their headings: the note brings none, and is not the page's.
    expect(headings()).toEqual(["Account", "Access tokens"]);
    expect(screen.queryByText(NOT_AVAILABLE)).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(screen.queryByText(errors.forbidden.body.error.message)).toBeNull();
  });

  test("a 500 is an error with Retry, and Retry asks again", async () => {
    const retry = vi.fn();
    await inPart(errorOf(errors.internal_error), retry);
    expect(screen.getByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    expect(headings()).toEqual(["Account", "Access tokens"]);
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retry).toHaveBeenCalledTimes(1);
  });

  test("a 404 is an error with Retry: a part is no page that could be not found", async () => {
    await inPart(errorOf(errors.not_found));
    expect(screen.getByRole("alert")).toHaveTextContent(errors.not_found.body.error.message);
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expect(headings()).toEqual(["Account", "Access tokens"]);
  });

  test("a network error is an error with Retry", async () => {
    await inPart(new NetworkError());
    expect(screen.getByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("an answer of a session that is over shows nothing", async () => {
    await inPart(new SessionOverError());
    const part = screen.getByRole("region", { name: "Access tokens" });
    expect(part.textContent).toBe("Access tokens");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test.each([
    ["a 403", errorOf(errors.forbidden)],
    ["a 500", errorOf(errors.internal_error)],
  ])("%s of a part brings no h1: the page has its own", async (_, error) => {
    await inPart(error);
    expect(h1()).toEqual(["Account"]);
  });
});
