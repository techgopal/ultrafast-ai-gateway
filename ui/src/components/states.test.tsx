import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, test, vi } from "vitest";
import { api } from "@/api/client";
import { ApiError, ConsoleRefusal, NetworkError, SessionOverError } from "@/api/errors";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState, messageOfError } from "@/components/ErrorState";
import { StatusBadge } from "@/components/StatusBadge";
import { formatTimestamp, Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { errors } from "@/test/errors";
import { override, refuse } from "@/test/handlers";
import { renderWithApp } from "@/test/render";

async function failureOf(call: Promise<unknown>): Promise<unknown> {
  return call.then(
    () => {
      throw new Error("The call did not fail.");
    },
    (reason: unknown) => reason,
  );
}

describe("error state", () => {
  test("error state", async () => {
    override("get", "/api/keys", () => refuse(errors.internal_error));
    const error = await failureOf(api.get("/api/keys"));
    expect(error).toBeInstanceOf(ApiError);
    const retry = vi.fn();
    const first = await renderWithApp(<ErrorState error={error} onRetry={retry} />);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Something went wrong.");
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retry).toHaveBeenCalledTimes(1);
    first.unmount();

    await renderWithApp(<ErrorState error={new NetworkError()} onRetry={retry} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("without a handler there is no Retry", async () => {
    await renderWithApp(<ErrorState error={new ApiError(404, "not_found", "Not found.")} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Not found.");
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  test.each([
    ["an error of the code", new TypeError("internal-detail at line 12")],
    ["something that is not an error", { stack: "internal-detail", body: "<html>" }],
    ["a text", "internal-detail"],
  ])("it never shows %s", async (_, error) => {
    await renderWithApp(<ErrorState error={error} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Something went wrong.");
    expect(document.body.innerHTML).not.toContain("internal-detail");
    expect(document.body.innerHTML).not.toContain("html&gt;");
  });

  test("it shows no stack of an error of the API", async () => {
    const error = new ApiError(409, "last_admin", "At least one active admin is required.");
    await renderWithApp(<ErrorState error={error} />);
    expect(screen.getByRole("alert")).toHaveTextContent("At least one active admin is required.");
    expect(document.body.innerHTML).not.toContain("ApiError");
    expect(document.body.innerHTML).not.toContain(".ts");
  });

  test("what the console itself refuses is told by its message", () => {
    expect(messageOfError(new ConsoleRefusal("Choose a user.", "user_id"))).toBe("Choose a user.");
    expect(messageOfError(new ConsoleRefusal("The link cannot be used."))).toBe(
      "The link cannot be used.",
    );
    // The others, as before.
    expect(messageOfError(new ApiError(409, "last_admin", "One admin is required."))).toBe(
      "One admin is required.",
    );
    expect(messageOfError(new NetworkError())).toBe("Could not reach the gateway.");
    expect(messageOfError(new SessionOverError())).toBeNull();
    expect(messageOfError(new Error("internal-detail"))).toBe("Something went wrong.");
  });

  test("an answer of a session that is over shows nothing", async () => {
    const retry = vi.fn();
    const app = await renderWithApp(
      <main>
        <ErrorState error={new SessionOverError()} onRetry={retry} />
      </main>,
    );
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
    expect(app.container.querySelector("main")).toBeEmptyDOMElement();
  });

  test.each(["light", "dark"] as const)("states show their text in the %s theme", async (theme) => {
    await renderWithApp(
      <main>
        <ErrorState error={new NetworkError()} onRetry={() => undefined} />
        <EmptyState
          title="No teams yet"
          description="Create the first team."
          action={<Button type="button">New team</Button>}
        />
        <StatusBadge status="active" />
        <Timestamp value={null} />
      </main>,
      { theme },
    );
    expect(document.documentElement.classList.contains("dark")).toBe(theme === "dark");
    expect(screen.getByText("Could not reach the gateway.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "No teams yet" })).toBeInTheDocument();
    expect(screen.getByText("Create the first team.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New team" })).toBeInTheDocument();
    expect(screen.getByText("active")).toBeInTheDocument();
    expect(screen.getByText("Never")).toBeInTheDocument();
  });
});

describe("timestamps", () => {
  const localized = new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(Date.UTC(2026, 8, 28, 14, 10, 0)));

  test("timestamps", async () => {
    expect(formatTimestamp("2026-09-28 14:10:00")).toBe(localized);
    expect(formatTimestamp(null)).toBe("Never");
    await renderWithApp(
      <main>
        <Timestamp value="2026-09-28 14:10:00" />
        <Timestamp value={null} />
      </main>,
    );
    const time = screen.getByText(localized);
    expect(time).toHaveAttribute("title", "2026-09-28 14:10:00 UTC");
    expect(time.tagName).toBe("TIME");
    expect(time).toHaveAttribute("datetime", "2026-09-28T14:10:00Z");
    const never = screen.getByText("Never");
    expect(never).not.toHaveAttribute("title");
  });

  test("the time is read as UTC, whatever the time zone of the browser", () => {
    const shown = formatTimestamp("2026-01-01 00:00:00");
    const expected = new Intl.DateTimeFormat(undefined, {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date("2026-01-01T00:00:00Z"));
    expect(shown).toBe(expected);
  });

  test.each(["soon", "2026-13-45 99:00:00", "2026-09-28T14:10:00Z", ""])(
    "a value of another form is shown as it is: %j",
    async (value) => {
      expect(formatTimestamp(value)).toBe(value);
      await renderWithApp(
        <main>
          <Timestamp value={value} />
        </main>,
      );
      expect(screen.queryByText("Never")).toBeNull();
      expect(screen.queryByText("Invalid Date")).toBeNull();
      expect(document.querySelector("time")).toBeNull();
    },
  );
});

describe("status badge", () => {
  test("a value the console does not know is shown as it is, and as nothing else", async () => {
    await renderWithApp(
      <main>
        <StatusBadge status="quarantined" />
      </main>,
    );
    const badge = screen.getByText("quarantined");
    expect(badge).toHaveAttribute("data-variant", "outline");
    expect(screen.getByRole("main")).toHaveTextContent(/^quarantined$/);
    expect(screen.queryByText("active")).toBeNull();
  });
});

describe("toasts", () => {
  // jsdom has no pointer capture, which the toast asks for when it is pressed.
  beforeAll(() => {
    for (const name of ["setPointerCapture", "releasePointerCapture"]) {
      if (Reflect.has(HTMLElement.prototype, name)) continue;
      Object.defineProperty(HTMLElement.prototype, name, {
        configurable: true,
        writable: true,
        value: () => undefined,
      });
    }
  });

  function Page() {
    const toast = useToast();
    return (
      <main>
        <Button
          type="button"
          onClick={() => {
            toast("Team created");
          }}
        >
          Create
        </Button>
        <Button
          type="button"
          onClick={() => {
            toast("It did not work", "error");
          }}
        >
          Fail
        </Button>
      </main>
    );
  }

  test("a toast is announced and can be dismissed", async () => {
    await renderWithApp(<Page />);
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    const text = await screen.findByText("Team created");
    const region = text.closest("[aria-live]");
    expect(region).toHaveAttribute("aria-live", "polite");
    const toast = text.closest<HTMLElement>("[data-sonner-toast]");
    if (toast === null) throw new Error("no toast");
    expect(toast).toHaveAttribute("data-type", "success");
    await userEvent.click(within(toast).getByRole("button", { name: "Close toast" }));
    await waitFor(() => {
      expect(screen.queryByText("Team created")).toBeNull();
    });
  });

  test("the tone error is a toast of that kind", async () => {
    await renderWithApp(<Page />);
    await userEvent.click(screen.getByRole("button", { name: "Fail" }));
    const text = await screen.findByText("It did not work");
    expect(text.closest("[data-sonner-toast]")).toHaveAttribute("data-type", "error");
  });

  test("a toast disappears after 5 seconds", async () => {
    await renderWithApp(<Page />);
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      await userEvent.click(screen.getByRole("button", { name: "Create" }), {
        advanceTimers: (ms) => {
          vi.advanceTimersByTime(ms);
        },
      });
      expect(await screen.findByText("Team created")).toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(4900);
      });
      expect(screen.getByText("Team created")).toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(1000);
      });
      await waitFor(() => {
        expect(screen.queryByText("Team created")).toBeNull();
      });
    } finally {
      vi.useRealTimers();
    }
  });
});
