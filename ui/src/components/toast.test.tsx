// A toast does not outlive the session it was raised in. The session is the
// real one (`SessionProvider` inside `AppProviders`), against MSW.
import { act, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test, vi } from "vitest";
import { api } from "@/api/client";
import { useSession, useSessionControl, useSignOut } from "@/auth/session";
import { dismissAll, useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import * as fixtures from "@/test/fixtures";
import { PASSWORD, startGateway } from "@/test/gateway";
import { override } from "@/test/handlers";
import { renderWithApp, unauthenticated } from "@/test/render";

const SAVED = "Saved.";
const SESSION_ENDED = "Your session ended. Sign in again.";

/** A page that says who is signed in, raises a toast, and ends or begins a session. */
function Page() {
  const session = useSession();
  const signOut = useSignOut();
  const { begin, notice } = useSessionControl();
  const toast = useToast();

  function enter() {
    void api
      .post("/api/auth/login", { body: { email: "lena@example.test", password: PASSWORD } })
      .then((answer) => {
        void begin(answer.csrf_token);
      });
  }

  if (session.status === "loading") return <p>loading</p>;
  return (
    <main>
      {notice === null ? null : <p role="status">{notice}</p>}
      <p>{session.status === "signedIn" ? session.me.user.name : "Nobody is signed in"}</p>
      <Button
        type="button"
        onClick={() => {
          toast(SAVED);
        }}
      >
        Save
      </Button>
      <Button type="button" onClick={enter}>
        Enter
      </Button>
      <Button type="button" onClick={() => void signOut()}>
        Leave
      </Button>
    </main>
  );
}

/** A call that the gateway answers with 401, as it does when the session ended. */
async function aCallFindsTheSessionEnded(): Promise<void> {
  override("get", "/api/teams", unauthenticated);
  await act(async () => {
    await api.get("/api/teams").catch(() => undefined);
  });
}

async function mayaSaves(): Promise<void> {
  startGateway({ signedIn: true });
  await renderWithApp(<Page />);
  await screen.findByText("Maya Okafor");
  await userEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText(SAVED)).toBeInTheDocument();
}

async function lenaEnters(): Promise<void> {
  startGateway({ me: fixtures.me.lena });
  await userEvent.click(screen.getByRole("button", { name: "Enter" }));
  await screen.findByText("Lena Fischer");
}

/** Time for a toast that was brought back to show. */
async function aMoment(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });
}

// The toasts are kept by the module of sonner, not by a component.
afterEach(() => {
  dismissAll();
});

describe("a toast and the end of its session", () => {
  test("sign-out takes the toast away, and the next sign-in does not bring it back", async () => {
    await mayaSaves();
    await userEvent.click(screen.getByRole("button", { name: "Leave" }));
    await screen.findByText("Nobody is signed in");
    await aMoment();
    expect(screen.queryByText(SAVED)).toBeNull();
    expect(document.querySelector("[data-sonner-toast]")).toBeNull();

    await lenaEnters();
    await aMoment();
    expect(screen.queryByText(SAVED)).toBeNull();
    expect(document.querySelector("[data-sonner-toast]")).toBeNull();
  });

  test("a session that ended takes the toast away, and its own notice shows", async () => {
    await mayaSaves();
    await aCallFindsTheSessionEnded();
    await screen.findByText("Nobody is signed in");
    await aMoment();
    expect(screen.queryByText(SAVED)).toBeNull();
    expect(document.querySelector("[data-sonner-toast]")).toBeNull();
    // The notice of the session is no toast: it is still said.
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);

    await lenaEnters();
    await aMoment();
    expect(screen.queryByText(SAVED)).toBeNull();
    expect(document.querySelector("[data-sonner-toast]")).toBeNull();
  });

  test("a sign-in that replaces a live session takes the toast of the one before away", async () => {
    await mayaSaves();
    // Nobody signed out: the next user signs in over the session of the one before.
    await lenaEnters();
    await waitFor(() => {
      expect(screen.queryByText(SAVED)).toBeNull();
    });
    await waitFor(() => {
      expect(document.querySelector("[data-sonner-toast]")).toBeNull();
    });
  });

  test("a toast of the new session is shown", async () => {
    await mayaSaves();
    await userEvent.click(screen.getByRole("button", { name: "Leave" }));
    await screen.findByText("Nobody is signed in");
    await lenaEnters();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText(SAVED)).toBeInTheDocument();
    expect(document.querySelectorAll("[data-sonner-toast]")).toHaveLength(1);
  });
});

describe("a sign-in within the 5 seconds of a toast", () => {
  // The clock of the test: it goes on with the time, and the test moves it too.
  function clock() {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({
      advanceTimers: (ms) => {
        vi.advanceTimersByTime(ms);
      },
    });
    const started = Date.now();
    return {
      user,
      elapsed: () => Date.now() - started,
      pass: (ms: number) => {
        act(() => {
          vi.advanceTimersByTime(ms);
        });
      },
    };
  }

  afterEach(() => {
    vi.useRealTimers();
  });

  const ways: [string, (user: ReturnType<typeof userEvent.setup>) => Promise<void>][] = [
    [
      "a sign-out",
      async (user) => {
        await user.click(screen.getByRole("button", { name: "Leave" }));
        await screen.findByText("Nobody is signed in");
      },
    ],
    [
      "a session that ended",
      async () => {
        await aCallFindsTheSessionEnded();
        await screen.findByText("Nobody is signed in");
      },
    ],
    ["no end at all: the sign-in replaces the session", () => Promise.resolve()],
  ];

  test.each(ways)("after %s", async (_, end) => {
    startGateway({ signedIn: true });
    await renderWithApp(<Page />);
    await screen.findByText("Maya Okafor");
    const { user, elapsed, pass } = clock();
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText(SAVED)).toBeInTheDocument();

    await end(user);
    pass(1000);
    startGateway({ me: fixtures.me.lena });
    await user.click(screen.getByRole("button", { name: "Enter" }));
    await screen.findByText("Lena Fischer");
    // The toast would still have had time to show.
    expect(elapsed()).toBeLessThan(4000);
    pass(300);
    await waitFor(() => {
      expect(screen.queryByText(SAVED)).toBeNull();
    });
    expect(document.querySelector("[data-sonner-toast]")).toBeNull();
    // Nor does it come back while its 5 seconds last, or after.
    pass(500);
    expect(screen.queryByText(SAVED)).toBeNull();
    expect(elapsed()).toBeLessThan(5000);
    pass(5000);
    expect(screen.queryByText(SAVED)).toBeNull();
  });
});

describe("the pages of the console", () => {
  test("the sign-in page shows the notice of the session, and no toast of the user before", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(null, { route: "/keys" });
    const { result } = renderHook(() => useToast());
    act(() => {
      result.current(SAVED);
    });
    expect(await screen.findByText(SAVED)).toBeInTheDocument();

    await aCallFindsTheSessionEnded();
    await waitFor(() => {
      expect(app.router.state.location.pathname).toBe("/sign-in");
    });
    await screen.findByRole("heading", { name: "Sign in" });
    await aMoment();
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(screen.queryByText(SAVED)).toBeNull();
    expect(document.querySelector("[data-sonner-toast]")).toBeNull();
  });
});
