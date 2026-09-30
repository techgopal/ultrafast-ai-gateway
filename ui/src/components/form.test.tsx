import { useForm } from "@tanstack/react-form";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useRef, useState } from "react";
import { describe, expect, test, vi } from "vitest";
import { api } from "@/api/client";
import { Field } from "@/components/Field";
import { ApiError, ConsoleRefusal, NetworkError, SessionOverError } from "@/api/errors";
import { applyApiError, onField, submitOnce, useFormFailure } from "@/components/form";
import { FormError } from "@/components/FormError";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { errors, fieldMessages, validationFailed, type GatewayError } from "@/test/errors";
import { gate, startGateway } from "@/test/gateway";
import { ok, override, refuse } from "@/test/handlers";
import { counted, settle } from "@/test/pages";
import * as fixtures from "@/test/fixtures";
import { renderWithApp, unauthenticated } from "@/test/render";

function descriptionOf(control: HTMLElement): string {
  return (control.getAttribute("aria-describedby") ?? "")
    .split(" ")
    .filter((id) => id !== "")
    .map((id) => document.getElementById(id)?.textContent ?? `nothing has the id ${id}`)
    .join(" ");
}

describe("field", () => {
  test("field wiring", async () => {
    await renderWithApp(
      <form aria-label="New team">
        <Field label="Name" name="name" hint="Up to 60 characters" error="name is taken" required>
          <Input defaultValue="Platform" />
        </Field>
        <Field label="Note" name="note">
          <Textarea />
        </Field>
      </form>,
    );
    const name = screen.getByLabelText("Name");
    await userEvent.click(screen.getByText("Name"));
    expect(name).toHaveFocus();
    expect(name).toHaveAttribute("name", "name");
    expect(name).toBeRequired();
    expect(name).toHaveAttribute("aria-invalid", "true");
    // The error comes first in what describes the field, then the hint.
    expect(descriptionOf(name)).toBe("name is taken Up to 60 characters");
    expect(name).toHaveAccessibleDescription("name is taken Up to 60 characters");
    // The error is announced when it appears.
    expect(screen.getByRole("alert")).toHaveTextContent("name is taken");
    expect(screen.getByText("Up to 60 characters")).not.toHaveAttribute("role");

    const note = screen.getByLabelText("Note");
    expect(note.tagName).toBe("TEXTAREA");
    await userEvent.click(screen.getByText("Note"));
    expect(note).toHaveFocus();
    expect(note).not.toHaveAttribute("aria-invalid");
    expect(note).not.toHaveAttribute("aria-describedby");
    expect(note).not.toBeRequired();
  });

  test("a control that takes its wiring itself", async () => {
    await renderWithApp(
      <form aria-label="New key">
        <Field label="Owner" name="owner_id" error="owner must be an active user">
          {(wiring) => (
            <div>
              <select {...wiring}>
                <option value="1">Maya</option>
              </select>
            </div>
          )}
        </Field>
      </form>,
    );
    const owner = screen.getByLabelText("Owner");
    expect(owner.tagName).toBe("SELECT");
    expect(owner).toHaveAttribute("name", "owner_id");
    expect(owner).toHaveAttribute("aria-invalid", "true");
    expect(descriptionOf(owner)).toBe("owner must be an active user");
  });

  test("a group of controls is named by the label, which is no label of a control", async () => {
    await renderWithApp(
      <form aria-label="New provider">
        <Field
          group
          label="Kind"
          name="kind"
          hint="What the provider speaks"
          error="kind must be openai or anthropic"
        >
          {({ id, name, ...described }) => (
            <div role="radiogroup" id={id} {...described}>
              <input type="radio" name={name} aria-label="OpenAI-compatible" />
              <input type="radio" name={name} aria-label="Anthropic" />
            </div>
          )}
        </Field>
        <Field label="Name" name="name">
          <Input />
        </Field>
      </form>,
    );
    const group = screen.getByRole("radiogroup", { name: "Kind" });
    // Named by the visible text, which is no `<label>`: a label names one control.
    const name = document.getElementById(group.getAttribute("aria-labelledby") ?? "");
    expect(name).toHaveTextContent(/^Kind$/);
    expect(name?.tagName).not.toBe("LABEL");
    expect(document.querySelector(`label[for="${group.id}"]`)).toBeNull();
    expect(screen.getAllByText("Kind")).toHaveLength(1);
    // The rest of the wiring is as for a control.
    expect(group).toHaveAttribute("aria-invalid", "true");
    expect(descriptionOf(group)).toBe("kind must be openai or anthropic What the provider speaks");

    // A control is named by its label, as before, and gets no `aria-labelledby`.
    const control = screen.getByLabelText("Name");
    expect(control).not.toHaveAttribute("aria-labelledby");
    expect(document.querySelector(`label[for="${control.id}"]`)).toHaveTextContent("Name");
  });

  test("two fields of the same name do not share their ids", async () => {
    await renderWithApp(
      <main>
        <Field label="Name of the team" name="name">
          <Input />
        </Field>
        <Field label="Name of the key" name="name">
          <Input />
        </Field>
      </main>,
    );
    expect(screen.getByLabelText("Name of the team").id).not.toBe(
      screen.getByLabelText("Name of the key").id,
    );
  });
});

/** A form of TanStack Form that sends an invite with the real client. */
function InviteForm({ onSent }: { onSent?: () => void }) {
  const form = useForm({
    defaultValues: { name: "", email: "" },
    onSubmit: async ({ value }) => {
      try {
        await api.post("/api/users", { body: { ...value, role: "member" } });
        onSent?.();
      } catch (error) {
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  return (
    <main>
      <form
        ref={formRef}
        aria-label="Invite a user"
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          void form.handleSubmit();
        }}
      >
        <FormError ref={errorRef} messages={failure.messages} />
        <form.Field name="name">
          {(field) => (
            <Field label="Name" name={field.name} error={failure.fieldError(field.name)}>
              <Input
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )}
        </form.Field>
        <form.Field name="email">
          {(field) => (
            <Field label="Email" name={field.name} error={failure.fieldError(field.name)}>
              <Input
                type="email"
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )}
        </form.Field>
        <Button type="submit">Send invite</Button>
      </form>
    </main>
  );
}

/**
 * A form that holds one field of the API in a value with two parts, as the
 * expiry of a key is a choice and a day. It sends with the real client.
 */
function ExpiryForm() {
  const form = useForm({
    defaultValues: { name: "", expires_at: { choice: "date", day: "" } },
    onSubmit: async ({ value }) => {
      try {
        await api.post("/api/keys", {
          body: { name: value.name, expires_at: `${value.expires_at.day} 23:59:59` },
        });
      } catch (error) {
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  return (
    <main>
      <form
        ref={formRef}
        aria-label="Create key"
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          void form.handleSubmit();
        }}
      >
        <FormError ref={errorRef} messages={failure.messages} />
        <form.Field name="name">
          {(field) => (
            <Field label="Name" name={field.name} error={failure.fieldError(field.name)}>
              <Input
                value={field.state.value}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )}
        </form.Field>
        <form.Field name="expires_at">
          {(field) => (
            <Field label="Expires" name={field.name} error={failure.fieldError(field.name)}>
              {(wiring) => (
                <div>
                  <select
                    {...wiring}
                    value={field.state.value.choice}
                    onChange={(event) => {
                      field.handleChange({ ...field.state.value, choice: event.target.value });
                    }}
                  >
                    <option value="never">Never</option>
                    <option value="date">On a date</option>
                  </select>
                  <Input
                    aria-label="Day"
                    value={field.state.value.day}
                    onChange={(event) => {
                      field.handleChange({ ...field.state.value, day: event.target.value });
                    }}
                  />
                </div>
              )}
            </Field>
          )}
        </form.Field>
        <Button type="submit">Create key</Button>
      </form>
    </main>
  );
}

async function fill(name = "Sam Carter", email = "sam@example.test"): Promise<void> {
  await userEvent.type(screen.getByLabelText("Name"), name);
  await userEvent.type(screen.getByLabelText("Email"), email);
  await userEvent.click(screen.getByRole("button", { name: "Send invite" }));
}

function answerWith(error: GatewayError): void {
  override("post", "/api/users", () => refuse(error));
}

describe("applyApiError", () => {
  test("api field errors land on fields", async () => {
    // `other` is a field the gateway does not have: the form has no field for it.
    answerWith(validationFailed({ name: fieldMessages.name, other: "x" }));
    await renderWithApp(<InviteForm />);
    await fill();
    const name = screen.getByLabelText("Name");
    await waitFor(() => {
      expect(descriptionOf(name)).toBe("name must be 1 to 100 characters");
    });
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Email")).not.toHaveAttribute("aria-invalid");
    const alerts = screen.getAllByRole("alert");
    expect(alerts.map((alert) => alert.textContent)).toEqual([
      "other: x",
      "name must be 1 to 100 characters",
    ]);
    // The error of the form is at the top of the form.
    const form = screen.getByRole("form", { name: "Invite a user" });
    expect(form.firstElementChild).toContainElement(alerts[0] ?? null);
    // The first field with an error has the focus.
    await waitFor(() => {
      expect(name).toHaveFocus();
    });
  });

  test("every field with an error shows its own, and the first has the focus", async () => {
    answerWith(errors.validation_failed);
    await renderWithApp(<InviteForm />);
    await fill();
    const name = screen.getByLabelText("Name");
    const email = screen.getByLabelText("Email");
    await waitFor(() => {
      expect(descriptionOf(email)).toBe("email is not valid");
    });
    expect(descriptionOf(name)).toBe("name must be 1 to 100 characters");
    expect(screen.getAllByRole("alert")).toHaveLength(2);
    await waitFor(() => {
      expect(name).toHaveFocus();
    });
  });

  test("an error without fields is shown by the form, which takes the focus", async () => {
    answerWith(errors.user_exists);
    await renderWithApp(<InviteForm />);
    await fill();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("A user with this email already exists.");
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(document.querySelector('[aria-invalid="true"]')).toBeNull();
    await waitFor(() => {
      expect(alert).toHaveFocus();
    });
  });

  test("form keeps values after an error", async () => {
    answerWith(validationFailed({ email: fieldMessages.email }));
    await renderWithApp(<InviteForm />);
    await fill("Sam Carter", "sam@example");
    await screen.findByRole("alert");
    expect(screen.getByLabelText("Name")).toHaveValue("Sam Carter");
    expect(screen.getByLabelText("Email")).toHaveValue("sam@example");

    answerWith(errors.internal_error);
    await userEvent.click(screen.getByRole("button", { name: "Send invite" }));
    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("Something went wrong.");
    });
    expect(screen.getByLabelText("Name")).toHaveValue("Sam Carter");
    expect(screen.getByLabelText("Email")).toHaveValue("sam@example");
  });

  test("the error of a field goes when the field is changed, and the form can be sent again", async () => {
    answerWith(validationFailed({ email: fieldMessages.email }));
    const sent: string[] = [];
    await renderWithApp(
      <InviteForm
        onSent={() => {
          sent.push("sent");
        }}
      />,
    );
    await fill("Sam Carter", "sam@example");
    const email = screen.getByLabelText("Email");
    await waitFor(() => {
      expect(email).toHaveAttribute("aria-invalid", "true");
    });
    await userEvent.type(email, ".test");
    expect(email).not.toHaveAttribute("aria-invalid");
    expect(screen.queryByRole("alert")).toBeNull();

    override("post", "/api/users", () =>
      ok("post", "/api/users", 201, {
        user: fixtures.users.sam,
        invite_link: fixtures.newInviteLink,
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Send invite" }));
    await waitFor(() => {
      expect(sent).toEqual(["sent"]);
    });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("the error of a field that was changed stays away when the old value is typed again", async () => {
    answerWith(validationFailed({ email: fieldMessages.email, name: fieldMessages.name }));
    await renderWithApp(<InviteForm />);
    await fill("Sam Carter", "sam@example");
    const email = screen.getByLabelText("Email");
    const name = screen.getByLabelText("Name");
    await waitFor(() => {
      expect(email).toHaveAttribute("aria-invalid", "true");
    });
    await userEvent.type(email, "x");
    expect(email).toHaveValue("sam@examplex");
    expect(email).not.toHaveAttribute("aria-invalid");
    await userEvent.type(email, "{Backspace}");
    // The value is the one the gateway refused, and the user knows: it was changed since.
    expect(email).toHaveValue("sam@example");
    expect(email).not.toHaveAttribute("aria-invalid");
    expect(descriptionOf(email)).toBe("");
    // The field that was not changed keeps its error.
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(screen.getAllByRole("alert").map((alert) => alert.textContent)).toEqual([
      fieldMessages.name,
    ]);
    // Changing a field moves no focus.
    expect(email).toHaveFocus();
  });

  test("the error of a field whose value has parts goes when a part is changed, and stays while another field is changed", async () => {
    override("post", "/api/keys", () =>
      refuse(validationFailed({ expires_at: fieldMessages.expiresAtPast })),
    );
    await renderWithApp(<ExpiryForm />);
    const expires = screen.getByLabelText("Expires");
    const day = screen.getByLabelText("Day");
    const send = screen.getByRole("button", { name: "Create key" });
    async function refused(): Promise<void> {
      await userEvent.click(send);
      await waitFor(() => {
        expect(descriptionOf(expires)).toBe(fieldMessages.expiresAtPast);
      });
      expect(expires).toHaveAttribute("aria-invalid", "true");
    }

    await userEvent.type(day, "2001-01-01");
    await refused();
    // Another field of the form: the error is not about it.
    await userEvent.type(screen.getByLabelText("Name"), "laptop");
    expect(descriptionOf(expires)).toBe(fieldMessages.expiresAtPast);

    // One part of the value.
    await userEvent.type(day, "0");
    expect(expires).not.toHaveAttribute("aria-invalid");
    expect(screen.queryByRole("alert")).toBeNull();

    // The other part.
    await refused();
    await userEvent.selectOptions(expires, "never");
    expect(expires).not.toHaveAttribute("aria-invalid");
    expect(screen.queryByRole("alert")).toBeNull();
    // And it stays away when the part is set back.
    await userEvent.selectOptions(expires, "date");
    expect(expires).not.toHaveAttribute("aria-invalid");
  });

  test("the errors of an attempt go when the next one starts", async () => {
    answerWith(validationFailed({ email: fieldMessages.email, other: "x" }));
    const sent: string[] = [];
    await renderWithApp(
      <InviteForm
        onSent={() => {
          sent.push("sent");
        }}
      />,
    );
    await fill();
    await waitFor(() => {
      expect(screen.getAllByRole("alert")).toHaveLength(2);
    });
    // Nothing was changed, and the form can still be sent.
    override("post", "/api/users", () =>
      ok("post", "/api/users", 201, {
        user: fixtures.users.sam,
        invite_link: fixtures.newInviteLink,
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Send invite" }));
    await waitFor(() => {
      expect(sent).toEqual(["sent"]);
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByLabelText("Email")).not.toHaveAttribute("aria-invalid");
  });

  test("a gateway that cannot be reached is said by the form", async () => {
    function Page() {
      const form = useForm({ defaultValues: { name: "" } });
      const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
      return (
        <form ref={formRef} aria-label="Test">
          <FormError ref={errorRef} messages={failure.messages} />
          <Button
            type="button"
            onClick={() => {
              applyApiError(form, new NetworkError());
            }}
          >
            Network
          </Button>
          <Button
            type="button"
            onClick={() => {
              applyApiError(form, new TypeError("internal-detail-of-the-code"));
            }}
          >
            Code
          </Button>
        </form>
      );
    }
    await renderWithApp(<Page />);
    await userEvent.click(screen.getByRole("button", { name: "Network" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    await userEvent.click(screen.getByRole("button", { name: "Code" }));
    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("Something went wrong.");
    });
    expect(document.body.innerHTML).not.toContain("internal-detail-of-the-code");
  });

  test("an answer of a session that is over puts nothing on the form", async () => {
    function Page() {
      const form = useForm({ defaultValues: { name: "Platform" } });
      const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
      return (
        <form ref={formRef} aria-label="Test">
          <FormError ref={errorRef} messages={failure.messages} />
          <form.Field name="name">
            {(field) => (
              <Field label="Name" name={field.name} error={failure.fieldError(field.name)}>
                <Input value={field.state.value} readOnly />
              </Field>
            )}
          </form.Field>
          <Button
            type="button"
            onClick={() => {
              applyApiError(form, new SessionOverError());
            }}
          >
            Apply
          </Button>
        </form>
      );
    }
    await renderWithApp(<Page />);
    const button = screen.getByRole("button", { name: "Apply" });
    await userEvent.click(button);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByLabelText("Name")).not.toHaveAttribute("aria-invalid");
    expect(screen.getByLabelText("Name")).toHaveValue("Platform");
    expect(screen.getByRole("form")).not.toHaveTextContent("session");
    // The focus stays where it was.
    expect(button).toHaveFocus();
  });

  test("a submit whose answer came after the session ended shows nothing", async () => {
    startGateway({ signedIn: true });
    let open: () => void = () => undefined;
    const opened = new Promise<void>((resolve) => {
      open = resolve;
    });
    override("post", "/api/users", async () => {
      await opened;
      return refuse(errors.user_exists);
    });
    await renderWithApp(<InviteForm />);
    await fill();
    override("get", "/api/teams", unauthenticated);
    await act(async () => {
      await api.get("/api/teams").catch(() => undefined);
    });
    await act(async () => {
      open();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText("A user with this email already exists.")).toBeNull();
  });

  test.each(["light", "dark"] as const)("the form shows its texts in the %s theme", async (theme) => {
    answerWith(validationFailed({ name: fieldMessages.name, other: "x" }));
    await renderWithApp(<InviteForm />, { theme });
    await fill();
    expect(await screen.findByText("name must be 1 to 100 characters")).toBeInTheDocument();
    expect(screen.getByText("other: x")).toBeInTheDocument();
    expect(screen.getByText("Name")).toBeInTheDocument();
  });
});

/** A form that is sent with `submitOnce`, by the real client. */
function TeamForm() {
  const form = useForm({
    defaultValues: { name: "" },
    onSubmit: async ({ value }) => {
      try {
        await api.post("/api/teams", { body: value });
      } catch (error) {
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  return (
    <main>
      <form ref={formRef} aria-label="New team" noValidate onSubmit={submitOnce(form)}>
        <FormError ref={errorRef} messages={failure.messages} />
        <form.Field name="name">
          {(field) => (
            <Field label="Name" name={field.name} error={failure.fieldError(field.name)}>
              <Input
                value={field.state.value}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )}
        </form.Field>
        <Button type="submit">Create team</Button>
      </form>
    </main>
  );
}

describe("submitOnce", () => {
  /** The form, filled, and its request, whose answer is held. */
  async function filled(answer: () => Response) {
    const door = gate();
    const posts = counted("post", "/api/teams", async () => {
      await door.opened;
      return answer();
    });
    await renderWithApp(<TeamForm />);
    await userEvent.type(screen.getByLabelText("Name"), "Growth");
    const open = () => {
      act(() => {
        door.open();
      });
    };
    return { posts, open, form: screen.getByRole("form", { name: "New team" }) };
  }

  const created = () => ok("post", "/api/teams", 201, fixtures.teams.growth);

  test("two submits in one tick are one request", async () => {
    const { posts, open, form } = await filled(created);
    act(() => {
      fireEvent.submit(form);
      fireEvent.submit(form);
    });
    await waitFor(() => {
      expect(posts.calls).toBe(1);
    });
    await settle();
    expect(posts.calls).toBe(1);
    expect(posts.bodies).toEqual([{ name: "Growth" }]);
    open();
  });

  test("a form that is being sent is not sent again: not by Enter, not by the button, not by a submit of its own", async () => {
    const { posts, open, form } = await filled(created);
    await userEvent.click(screen.getByRole("button", { name: "Create team" }));
    await waitFor(() => {
      expect(posts.calls).toBe(1);
    });
    await userEvent.type(screen.getByLabelText("Name"), "{Enter}{Enter}");
    await userEvent.click(screen.getByRole("button", { name: "Create team" }));
    fireEvent.submit(form);
    await settle();
    expect(posts.calls).toBe(1);
    open();
  });

  test("when the request has its answer the form can be sent again: after a refusal, and after a success", async () => {
    const { posts, open } = await filled(() => refuse(errors.forbidden));
    await userEvent.type(screen.getByLabelText("Name"), "{Enter}");
    open();
    expect(await screen.findByRole("alert")).toHaveTextContent(errors.forbidden.body.error.message);
    expect(posts.calls).toBe(1);

    const again = counted("post", "/api/teams", created);
    await userEvent.type(screen.getByLabelText("Name"), "{Enter}");
    await waitFor(() => {
      expect(again.calls).toBe(1);
    });
    await waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
    await userEvent.type(screen.getByLabelText("Name"), "{Enter}");
    await waitFor(() => {
      expect(again.calls).toBe(2);
    });
  });

  test("the browser does not send the form itself, also not the submit that is refused", async () => {
    const { open, form } = await filled(created);
    // `fireEvent` answers whether the event was left to the browser.
    let first = true;
    let second = true;
    act(() => {
      first = fireEvent.submit(form);
      second = fireEvent.submit(form);
    });
    expect([first, second]).toEqual([false, false]);
    open();
  });
});

describe("form error", () => {
  test("two equal messages are both shown", async () => {
    const said = vi.spyOn(console, "error").mockImplementation(() => undefined);
    try {
      await renderWithApp(
        <FormError messages={["must not be empty", "must not be empty", "another"]} />,
      );
      const lines = [...screen.getByRole("alert").querySelectorAll("p")];
      expect(lines.map((line) => line.textContent)).toEqual([
        "must not be empty",
        "must not be empty",
        "another",
      ]);
      // React says nothing about the keys of the lines.
      expect(said).not.toHaveBeenCalled();
    } finally {
      said.mockRestore();
    }
  });

  test("a line that goes leaves the other ones", async () => {
    function Page() {
      const [messages, setMessages] = useState(["must not be empty", "must not be empty"]);
      return (
        <main>
          <FormError messages={messages} />
          <Button
            type="button"
            onClick={() => {
              setMessages(["must not be empty"]);
            }}
          >
            One
          </Button>
        </main>
      );
    }
    await renderWithApp(<Page />);
    expect(screen.getByRole("alert").querySelectorAll("p")).toHaveLength(2);
    await userEvent.click(screen.getByRole("button", { name: "One" }));
    expect(screen.getByRole("alert").querySelectorAll("p")).toHaveLength(1);
  });
});

describe("onField", () => {
  const taken = errors.user_exists;
  const error = new ApiError(taken.status, taken.body.error.code, taken.body.error.message);

  test("a refusal of the code is said by the field", () => {
    const said = onField(error, "user_exists", "email");
    expect(said).toBeInstanceOf(ApiError);
    expect(said).toMatchObject({
      status: 409,
      code: "user_exists",
      message: taken.body.error.message,
      fields: { email: taken.body.error.message },
    });
  });

  test("with a text the field says it in the words of the console, and the error stays the answer of the gateway", () => {
    const gone = errors.not_found;
    const answer = new ApiError(gone.status, gone.body.error.code, gone.body.error.message);
    const said = onField(answer, "not_found", "user_id", "No user with that ID.");
    expect(said).toBeInstanceOf(ApiError);
    expect(said).not.toBeInstanceOf(ConsoleRefusal);
    expect(said).toMatchObject({
      status: 404,
      code: "not_found",
      message: gone.body.error.message,
      fields: { user_id: "No user with that ID." },
    });
    // An error of another code is not given the text either.
    expect(onField(error, "not_found", "user_id", "No user with that ID.")).toBe(error);
  });

  test("every other error is returned as it is", () => {
    expect(onField(error, "team_exists", "name")).toBe(error);
    const network = new NetworkError();
    expect(onField(network, "user_exists", "email")).toBe(network);
    const over = new SessionOverError();
    expect(onField(over, "user_exists", "email")).toBe(over);
    const refusal = new ConsoleRefusal("Choose a user.", "user_id");
    expect(onField(refusal, "user_exists", "email")).toBe(refusal);
  });
});

describe("what the console itself refuses", () => {
  const CHOOSE = "Choose a user.";
  const UNUSABLE = "The gateway returned an invite link that cannot be used.";

  test("it is no answer of the gateway: no status, no code", () => {
    const about = new ConsoleRefusal(CHOOSE, "user_id");
    expect(about).toBeInstanceOf(Error);
    expect(about).not.toBeInstanceOf(ApiError);
    expect(about.name).toBe("ConsoleRefusal");
    expect(about.message).toBe(CHOOSE);
    expect(about.field).toBe("user_id");
    expect(Reflect.has(about, "status")).toBe(false);
    expect(Reflect.has(about, "code")).toBe(false);
    expect(Reflect.has(about, "fields")).toBe(false);
    expect(new ConsoleRefusal(UNUSABLE).field).toBeUndefined();
  });

  function Page() {
    const form = useForm({ defaultValues: { user_id: "" } });
    const formRef = useRef<HTMLFormElement>(null);
    const errorRef = useRef<HTMLDivElement>(null);
    const failure = useFormFailure(form, formRef, errorRef);
    const apply = (error: unknown) => () => {
      applyApiError(form, error);
    };
    return (
      <form ref={formRef} aria-label="Test">
        <FormError ref={errorRef} messages={failure.messages} />
        <form.Field name="user_id">
          {(field) => (
            <Field label="User ID" name={field.name} error={failure.fieldError(field.name)}>
              <Input
                value={field.state.value}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )}
        </form.Field>
        <Button type="button" onClick={apply(new ConsoleRefusal(CHOOSE, "user_id"))}>
          About the field
        </Button>
        <Button type="button" onClick={apply(new ConsoleRefusal(UNUSABLE))}>
          About no field
        </Button>
        <Button type="button" onClick={apply(new ConsoleRefusal("is not known", "team_id"))}>
          About a field the form does not have
        </Button>
      </form>
    );
  }

  test("with a field it is the error of that field, which takes the focus", async () => {
    await renderWithApp(<Page />);
    const field = screen.getByLabelText("User ID");
    await userEvent.click(screen.getByRole("button", { name: "About the field" }));
    await waitFor(() => {
      expect(field).toHaveAttribute("aria-invalid", "true");
    });
    expect(descriptionOf(field)).toBe(CHOOSE);
    // The field says it, and the form says nothing beside it.
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(field).toHaveFocus();
    // As every error of a field, it goes when the field is changed.
    await userEvent.type(field, "7");
    expect(field).not.toHaveAttribute("aria-invalid");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("without a field it is the error of the form, which takes the focus", async () => {
    await renderWithApp(<Page />);
    await userEvent.click(screen.getByRole("button", { name: "About no field" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(UNUSABLE);
    expect(screen.getByLabelText("User ID")).not.toHaveAttribute("aria-invalid");
    await waitFor(() => {
      expect(alert).toHaveFocus();
    });
  });

  test("about a field the form does not have it is said by the form, with the name of the field", async () => {
    await renderWithApp(<Page />);
    await userEvent.click(
      screen.getByRole("button", { name: "About a field the form does not have" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("team_id: is not known");
    expect(screen.getByLabelText("User ID")).not.toHaveAttribute("aria-invalid");
  });
});
