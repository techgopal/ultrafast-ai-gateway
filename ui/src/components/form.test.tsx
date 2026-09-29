import { useForm } from "@tanstack/react-form";
import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useRef } from "react";
import { describe, expect, test } from "vitest";
import { api } from "@/api/client";
import { NetworkError, SessionOverError } from "@/api/errors";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure } from "@/components/form";
import { FormError } from "@/components/FormError";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { errors, fieldMessages, validationFailed, type GatewayError } from "@/test/errors";
import { startGateway } from "@/test/gateway";
import { ok, override, refuse } from "@/test/handlers";
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
