import type { QueryClient } from "@tanstack/react-query";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { api } from "@/api/client";
import { ConsoleRefusal, NetworkError, SessionOverError } from "@/api/errors";
import { useCreateKey, useDeleteUser } from "@/api/queries";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { SecretDialog, useSecretOnce } from "@/components/SecretDialog";
import { Button } from "@/components/ui/button";
import { errors } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { noContent, ok, override, refuse } from "@/test/handlers";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

const SECRET = fixtures.newKeySecret;
const NO_MUTATION = { reset: () => undefined };
const ASK = "Have you copied it? It cannot be shown again.";

function Users({ onConfirm }: { onConfirm?: () => Promise<unknown> }) {
  const [open, setOpen] = useState(false);
  const remove = useDeleteUser();
  return (
    <main>
      <Button
        type="button"
        onClick={() => {
          setOpen(true);
        }}
      >
        Delete user
      </Button>
      <ConfirmDialog
        open={open}
        onOpenChange={setOpen}
        title="Delete this user?"
        body="Their keys stop working."
        confirmLabel="Delete"
        tone="danger"
        onConfirm={onConfirm ?? (() => remove.mutateAsync({ id: fixtures.users.tomas.id }))}
      />
    </main>
  );
}

async function openConfirm(): Promise<HTMLElement> {
  await userEvent.click(screen.getByRole("button", { name: "Delete user" }));
  return screen.getByRole("alertdialog", { name: "Delete this user?" });
}

describe("confirm dialog", () => {
  test("dialog focus", async () => {
    await renderWithApp(<Users />);
    const opener = screen.getByRole("button", { name: "Delete user" });
    const dialog = await openConfirm();
    expect(dialog).toHaveAccessibleDescription("Their keys stop working.");
    await waitFor(() => {
      expect(dialog).toContainElement(document.activeElement as HTMLElement);
    });
    const controls = within(dialog).getAllByRole("button");
    const first = controls[0];
    const last = controls[controls.length - 1];
    if (first === undefined || last === undefined) throw new Error("the dialog has no controls");
    expect(controls.length).toBeGreaterThan(1);
    last.focus();
    await userEvent.tab();
    expect(first).toHaveFocus();
    await userEvent.tab({ shift: true });
    expect(last).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(opener).toHaveFocus();
  });

  test("cancel closes and calls nothing", async () => {
    const confirm = vi.fn(() => Promise.resolve());
    await renderWithApp(<Users onConfirm={confirm} />);
    const dialog = await openConfirm();
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(confirm).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Delete user" })).toHaveFocus();
  });

  test("confirm closes on success", async () => {
    let deleted = 0;
    override("delete", "/api/users/{id}", () => {
      deleted += 1;
      return noContent();
    });
    await renderWithApp(<Users />);
    const dialog = await openConfirm();
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(deleted).toBe(1);
  });

  test("confirm stays open on api error", async () => {
    override("delete", "/api/users/{id}", () => refuse(errors.last_admin));
    await renderWithApp(<Users />);
    const dialog = await openConfirm();
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("At least one active admin is required.");
    // The focus goes to what went wrong; Tab from there reaches the buttons.
    expect(alert).toHaveAttribute("tabindex", "-1");
    await waitFor(() => {
      expect(alert).toHaveFocus();
    });
    expect(screen.getByRole("alertdialog", { name: "Delete this user?" })).toBeInTheDocument();
    // It can be tried again, and cancelled.
    expect(within(dialog).getByRole("button", { name: "Delete" })).toBeEnabled();
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    // The message of the attempt before is not shown by the next dialog.
    await openConfirm();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("confirm shows the network message when the gateway cannot be reached", async () => {
    await renderWithApp(<Users onConfirm={() => Promise.reject(new NetworkError())} />);
    const dialog = await openConfirm();
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "Could not reach the gateway.",
    );
  });

  test("confirm shows what the console itself refuses", async () => {
    const refusing = () => Promise.reject(new ConsoleRefusal("The link cannot be used."));
    await renderWithApp(<Users onConfirm={refusing} />);
    const dialog = await openConfirm();
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("The link cannot be used.");
    expect(alert).not.toHaveTextContent("Something went wrong.");
    expect(dialog).toBeInTheDocument();
  });

  test("confirm shows nothing of an error that is not of the API", async () => {
    const failing = () => Promise.reject(new TypeError("internal-detail-of-the-code"));
    await renderWithApp(<Users onConfirm={failing} />);
    const dialog = await openConfirm();
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Something went wrong.");
    expect(document.body.innerHTML).not.toContain("internal-detail-of-the-code");
  });

  test("confirm buttons disabled while running", async () => {
    const door = gate();
    let calls = 0;
    override("delete", "/api/users/{id}", async () => {
      calls += 1;
      await door.opened;
      return noContent();
    });
    await renderWithApp(<Users />);
    const dialog = await openConfirm();
    const confirm = within(dialog).getByRole("button", { name: "Delete" });
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    await userEvent.click(confirm);
    await waitFor(() => {
      expect(confirm).toBeDisabled();
    });
    expect(cancel).toBeDisabled();
    // Escape does not close it while the call runs.
    await userEvent.keyboard("{Escape}");
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    expect(calls).toBe(1);
    await act(async () => {
      door.open();
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
  });

  test("confirm shows nothing for an answer of a session that is over", async () => {
    await renderWithApp(<Users onConfirm={() => Promise.reject(new SessionOverError())} />);
    const dialog = await openConfirm();
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(dialog).not.toHaveTextContent("session");
    // It is not the dialog that closes itself.
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
  });

  test("the end of the session closes the dialog, and its late answer shows nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("delete", "/api/users/{id}", async () => {
      await door.opened;
      return refuse(errors.last_admin);
    });
    await renderWithApp(<Users />);
    const dialog = await openConfirm();
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await aCallFindsTheSessionEnded();
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    await act(async () => {
      door.open();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByText("At least one active admin is required.")).toBeNull();
  });

  test.each(["danger", "primary"] as const)("the tone %s is said by the button", async (tone) => {
    function Page() {
      return (
        <ConfirmDialog
          open
          onOpenChange={() => undefined}
          title="Revoke this key?"
          body="It stops working at once."
          confirmLabel="Revoke"
          tone={tone}
          onConfirm={() => Promise.resolve()}
        />
      );
    }
    await renderWithApp(<Page />);
    expect(screen.getByRole("button", { name: "Revoke" })).toHaveAttribute(
      "data-variant",
      tone === "danger" ? "destructive" : "default",
    );
  });
});

/** A call that the gateway answers with 401, as it does when the session ended. */
async function aCallFindsTheSessionEnded(): Promise<void> {
  override("get", "/api/teams", unauthenticated);
  await act(async () => {
    await api.get("/api/teams").catch(() => undefined);
  });
}

/** A page that makes a key with the real mutation and shows it once. */
function Keys({ onMade }: { onMade?: (leftInMutation: unknown) => void }) {
  const create = useCreateKey();
  const once = useSecretOnce(create);
  const [failed, setFailed] = useState(false);
  return (
    <main>
      <Button
        type="button"
        onClick={() => {
          create
            .mutateAsync({ name: "CI" })
            .then((made) => {
              once.show(made.secret);
            })
            .catch(() => {
              setFailed(true);
            });
        }}
      >
        Create key
      </Button>
      <Button
        type="button"
        onClick={() => {
          onMade?.(create.data);
        }}
      >
        Look at the mutation
      </Button>
      {failed ? <p>It failed</p> : null}
      <SecretDialog
        title="Your new key"
        description="Copy this key now. It is not shown again."
        secret={once.secret}
        onClose={once.clear}
      />
    </main>
  );
}

async function makeKey(): Promise<HTMLElement> {
  await userEvent.click(screen.getByRole("button", { name: "Create key" }));
  return screen.findByRole("dialog", { name: "Your new key" });
}

/** The page as text, with what the fields hold. */
function shown(): string {
  const values = [...document.querySelectorAll("input, textarea")].map((field) =>
    field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement ? field.value : "",
  );
  return document.documentElement.outerHTML + JSON.stringify(values);
}

function cached(client: QueryClient): string {
  const queries = client
    .getQueryCache()
    .getAll()
    .map((query) => ({ key: query.queryKey, state: query.state }));
  const mutations = client
    .getMutationCache()
    .getAll()
    .map((mutation) => ({
      key: mutation.options.mutationKey,
      state: mutation.state,
    }));
  return JSON.stringify({ queries, mutations });
}

function stored(): string {
  return JSON.stringify([
    Object.entries(window.localStorage),
    Object.entries(window.sessionStorage),
    document.cookie,
    window.history.state,
    window.location.href,
  ]);
}

function routed(app: AppRenderResult): string {
  return JSON.stringify(app.router.state);
}

/** `text` is nowhere: by default the secret of the fixtures. */
function expectNoSecret(app: AppRenderResult, text = SECRET): void {
  expect(document.body.innerHTML).not.toContain(text);
  expect(shown()).not.toContain(text);
  expect(cached(app.queryClient)).not.toContain(text);
  expect(routed(app)).not.toContain(text);
  expect(JSON.stringify(window.history.state)).not.toContain(text);
  expect(stored()).not.toContain(text);
}

const clipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");

function setClipboard(value: unknown): void {
  Object.defineProperty(navigator, "clipboard", { configurable: true, value });
}

afterEach(() => {
  if (clipboard === undefined) Reflect.deleteProperty(navigator, "clipboard");
  else Object.defineProperty(navigator, "clipboard", clipboard);
});

describe("secret dialog", () => {
  test("the scans find a secret that is there", async () => {
    const app = await renderWithApp(<p>{SECRET}</p>);
    expect(() => {
      expectNoSecret(app);
    }).toThrow();
    app.queryClient.setQueryData(["a-test"], { secret: SECRET });
    expect(cached(app.queryClient)).toContain(SECRET);
  });

  test("the scan finds a marker in the mutation cache", async () => {
    const marker = "marker-7f3a";
    const app = await renderWithApp(<p>A page</p>);
    expectNoSecret(app, marker);
    const mutation = app.queryClient
      .getMutationCache()
      .build(app.queryClient, { mutationFn: () => Promise.resolve({ value: marker }) });
    await mutation.execute(undefined);
    // It is the mutation cache that holds it, not the query cache.
    const queries = app.queryClient.getQueryCache().getAll();
    expect(JSON.stringify(queries.map((query) => query.state))).not.toContain(marker);
    expect(cached(app.queryClient)).toContain(marker);
    expect(() => {
      expectNoSecret(app, marker);
    }).toThrow();
  });

  test("the scan finds a marker in the variables of a mutation", async () => {
    const marker = "marker-2c9e";
    const app = await renderWithApp(<p>A page</p>);
    const mutation = app.queryClient
      .getMutationCache()
      .build(app.queryClient, { mutationFn: () => Promise.resolve(null) });
    await mutation.execute({ value: marker });
    expect(cached(app.queryClient)).toContain(marker);
    expect(() => {
      expectNoSecret(app, marker);
    }).toThrow();
  });

  test("the scan finds a marker in router state", async () => {
    const marker = "marker-5b1d";
    const app = await renderWithApp(<p>A page</p>);
    expectNoSecret(app, marker);
    // No page of the console keeps state in the router, so it has no such field.
    await act(async () => {
      await app.router.navigate({
        to: "/",
        state: (before) => Object.assign({}, before, { note: marker }),
      });
    });
    expect(routed(app)).toContain(marker);
    expect(() => {
      expectNoSecret(app, marker);
    }).toThrow();
  });

  test("the scan finds a marker in the address of the router", async () => {
    const marker = "marker-8e4f";
    const app = await renderWithApp(<p>A page</p>);
    await act(async () => {
      await app.router.navigate({ to: "/", search: { note: marker } });
    });
    expect(routed(app)).toContain(marker);
    expect(() => {
      expectNoSecret(app, marker);
    }).toThrow();
  });

  test.each(["localStorage", "sessionStorage"] as const)(
    "the scan finds a marker in browser storage: %s",
    async (name) => {
      const marker = "marker-a06c";
      const app = await renderWithApp(<p>A page</p>);
      expectNoSecret(app, marker);
      window[name].setItem("a-test", marker);
      try {
        expect(stored()).toContain(marker);
        expect(() => {
          expectNoSecret(app, marker);
        }).toThrow();
      } finally {
        window[name].removeItem("a-test");
      }
      expectNoSecret(app, marker);
    },
  );

  test("the scan finds a marker in the state of the history of the browser", async () => {
    const marker = "marker-d713";
    const app = await renderWithApp(<p>A page</p>);
    const before: unknown = window.history.state;
    window.history.replaceState({ note: marker }, "");
    try {
      expect(stored()).toContain(marker);
      expect(() => {
        expectNoSecret(app, marker);
      }).toThrow();
    } finally {
      window.history.replaceState(before, "");
    }
    expectNoSecret(app, marker);
  });

  test("secret is shown and copied", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(() => Promise.resolve());
    setClipboard({ writeText });
    await renderWithApp(<Keys />);
    await user.click(screen.getByRole("button", { name: "Create key" }));
    const dialog = await screen.findByRole("dialog", { name: "Your new key" });
    expect(dialog).toHaveAccessibleDescription("Copy this key now. It is not shown again.");
    const field = within(dialog).getByRole("textbox", { name: "Your new key" });
    expect(field).toHaveValue(SECRET);
    expect(field).toHaveAttribute("readonly");
    expect(field).toHaveClass("font-mono");
    await user.click(within(dialog).getByRole("button", { name: "Copy" }));
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(writeText).toHaveBeenCalledWith(SECRET);
    expect(await within(dialog).findByRole("status")).toHaveTextContent("Copied");
  });

  test.each([
    ["no clipboard API", undefined],
    ["a clipboard that refuses", { writeText: () => Promise.reject(new Error("not allowed")) }],
  ])("secret copy fallback: %s", async (_, clipboardOfTheTest) => {
    const user = userEvent.setup();
    setClipboard(clipboardOfTheTest);
    await renderWithApp(<Keys />);
    await user.click(screen.getByRole("button", { name: "Create key" }));
    const dialog = await screen.findByRole("dialog", { name: "Your new key" });
    const field = within(dialog).getByRole<HTMLInputElement>("textbox", { name: "Your new key" });
    await user.click(within(dialog).getByRole("button", { name: "Copy" }));
    expect(await within(dialog).findByRole("status")).toHaveTextContent("Press Ctrl+C to copy");
    expect(field).toHaveFocus();
    expect(field.selectionStart).toBe(0);
    expect(field.selectionEnd).toBe(SECRET.length);
    expect(within(dialog).queryByText("Copied")).toBeNull();
  });

  test("secret close asks first", async () => {
    const app = await renderWithApp(<Keys />);
    const dialog = await makeKey();
    await userEvent.keyboard("{Escape}");
    const question = await screen.findByRole("alertdialog");
    expect(question).toHaveTextContent(ASK);
    // Keep open is the default: it has the focus.
    const keep = within(question).getByRole("button", { name: "Keep open" });
    await waitFor(() => {
      expect(keep).toHaveFocus();
    });
    await userEvent.click(keep);
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(screen.getByRole("dialog", { name: "Your new key" })).toBeInTheDocument();
    expect(within(dialog).getByRole("textbox", { name: "Your new key" })).toHaveValue(SECRET);

    // Escape on the question is Keep open as well.
    await userEvent.keyboard("{Escape}");
    await screen.findByRole("alertdialog");
    await userEvent.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(screen.getByRole("textbox", { name: "Your new key" })).toHaveValue(SECRET);

    await userEvent.keyboard("{Escape}");
    const again = await screen.findByRole("alertdialog");
    await userEvent.click(within(again).getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expectNoSecret(app);
  });

  test.each(["Close", "Done"])("the button %s of the dialog asks first too", async (name) => {
    await renderWithApp(<Keys />);
    const dialog = await makeKey();
    await userEvent.click(within(dialog).getByRole("button", { name }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(ASK);
    expect(screen.getByRole("textbox", { name: "Your new key", hidden: true })).toHaveValue(
      SECRET,
    );
  });

  test("secret is gone after close", async () => {
    let inMutation: unknown = "not looked at";
    const app = await renderWithApp(
      <Keys
        onMade={(left) => {
          inMutation = left;
        }}
      />,
    );
    await makeKey();
    expect(document.body.innerHTML).toContain(SECRET);
    // While it is shown it is in the state of the page only.
    expect(cached(app.queryClient)).not.toContain(SECRET);
    expect(JSON.stringify(app.router.state)).not.toContain(SECRET);
    expect(stored()).not.toContain(SECRET);

    await userEvent.keyboard("{Escape}");
    const question = await screen.findByRole("alertdialog");
    await userEvent.click(within(question).getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expectNoSecret(app);
    // The focus is back on what opened the dialog.
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Create key" })).toHaveFocus();
    });
    await userEvent.click(screen.getByRole("button", { name: "Look at the mutation" }));
    expect(inMutation).toBeUndefined();
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
  });

  test("secret is gone after the session ended while it was shown", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(<Keys />);
    await makeKey();
    expect(document.body.innerHTML).toContain(SECRET);

    await aCallFindsTheSessionEnded();

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expectNoSecret(app);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  });

  test("the session ending while the question is shown closes both", async () => {
    startGateway({ signedIn: true });
    const app = await renderWithApp(<Keys />);
    await makeKey();
    await userEvent.keyboard("{Escape}");
    await screen.findByRole("alertdialog");
    await aCallFindsTheSessionEnded();
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expectNoSecret(app);
  });

  test("a key whose answer came for a session that is over is not shown", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("post", "/api/keys", async () => {
      await door.opened;
      return ok("post", "/api/keys", 201, { key: fixtures.keys.active, secret: SECRET });
    });
    const app = await renderWithApp(<Keys />);
    await userEvent.click(screen.getByRole("button", { name: "Create key" }));
    await aCallFindsTheSessionEnded();
    await act(async () => {
      door.open();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expectNoSecret(app);
  });

  test("what the opener says about the use of the secret shows under it, and goes with it", async () => {
    const NOTE = "Send it as a bearer token.";
    function Page() {
      const once = useSecretOnce(NO_MUTATION);
      return (
        <main>
          <Button
            type="button"
            onClick={() => {
              once.show(SECRET);
            }}
          >
            Show
          </Button>
          <SecretDialog
            title="Your new key"
            description="Copy this key now. It is not shown again."
            secret={once.secret}
            onClose={once.clear}
          >
            <p>{NOTE}</p>
          </SecretDialog>
        </main>
      );
    }
    await renderWithApp(<Page />);
    expect(screen.queryByText(NOTE)).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Show" }));
    const dialog = await screen.findByRole("dialog", { name: "Your new key" });
    const note = within(dialog).getByText(NOTE);
    // After the secret, and before the button that closes the dialog.
    const field = within(dialog).getByLabelText("Your new key");
    const done = within(dialog).getByRole("button", { name: "Done" });
    expect(field.compareDocumentPosition(note) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(note.compareDocumentPosition(done) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);

    await userEvent.click(done);
    const question = await screen.findByRole("alertdialog");
    await userEvent.click(within(question).getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(screen.queryByText(NOTE)).toBeNull();
  });

  test("the hook does not compile without the mutation", () => {
    function Page() {
      // @ts-expect-error The mutation is required: without it the secret would stay in its answer.
      const once = useSecretOnce();
      return <p>{once.secret}</p>;
    }
    // The check is the one of the compiler (`pnpm typecheck`); the page is never rendered.
    expect(Page).toBeTypeOf("function");
  });

  test("the secret of the hook goes when its page goes", async () => {
    const seen: (string | null)[] = [];
    function Page() {
      // No mutation made this secret: there is nothing to reset.
      const once = useSecretOnce(NO_MUTATION);
      seen.push(once.secret);
      return (
        <Button
          type="button"
          onClick={() => {
            once.show(SECRET);
          }}
        >
          Show
        </Button>
      );
    }
    const app = await renderWithApp(<Page />);
    await userEvent.click(screen.getByRole("button", { name: "Show" }));
    expect(seen.at(-1)).toBe(SECRET);
    app.unmount();
    expectNoSecret(app);
  });
});

describe("narrow screens and themes", () => {
  test.each([390, 1280])("dialogs fit narrow screens: width %s", async (width) => {
    await renderWithApp(<Keys />, { width });
    const dialog = await makeKey();
    // No wider than the screen less its margin, at every width below 768.
    expect(dialog).toHaveClass("w-full", "max-w-[calc(100%-2rem)]", "md:max-w-sm");
    expect(dialog.className).not.toMatch(/(^|\s)sm:max-w-sm(\s|$)/);
    // No higher than the screen, and what does not fit scrolls inside.
    expect(dialog).toHaveClass("max-h-[calc(100svh-2rem)]", "overflow-y-auto");
    await userEvent.keyboard("{Escape}");
    const question = await screen.findByRole("alertdialog");
    expect(question).toHaveClass("max-h-[calc(100svh-2rem)]", "overflow-y-auto");
    expect(question.className).toContain("max-w-[calc(100%-2rem)]");
    // Every button can be touched.
    for (const button of within(question).getAllByRole("button")) {
      expect(button).toHaveClass("min-h-11");
    }
  });

  test.each(["light", "dark"] as const)("the dialogs show their text in the %s theme", async (theme) => {
    await renderWithApp(<Keys />, { theme });
    expect(document.documentElement.classList.contains("dark")).toBe(theme === "dark");
    const dialog = await makeKey();
    expect(dialog).toHaveTextContent("Your new key");
    expect(dialog).toHaveTextContent("Copy this key now. It is not shown again.");
    await userEvent.keyboard("{Escape}");
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(ASK);
  });
});

describe("confirm dialog: the focus after a failure", () => {
  test("every failure moves the focus to its message, also the second one", async () => {
    override("delete", "/api/users/{id}", () => refuse(errors.last_admin));
    await renderWithApp(<Users />);
    const dialog = await openConfirm();
    const confirm = within(dialog).getByRole("button", { name: "Delete" });
    await userEvent.click(confirm);
    await waitFor(() => {
      expect(within(dialog).getByRole("alert")).toHaveFocus();
    });
    await userEvent.tab();
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
    expect(within(dialog).getByRole("alert")).not.toHaveFocus();

    override("delete", "/api/users/{id}", () => refuse(errors.cannot_delete_self));
    await userEvent.click(confirm);
    await waitFor(() => {
      expect(within(dialog).getByRole("alert")).toHaveTextContent(
        "You cannot delete your own account.",
      );
    });
    await waitFor(() => {
      expect(within(dialog).getByRole("alert")).toHaveFocus();
    });
  });
});
