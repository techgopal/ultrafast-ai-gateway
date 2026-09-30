import { useForm } from "@tanstack/react-form";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useRef, useState } from "react";
import { describe, expect, test } from "vitest";
import { useCreateTeam } from "@/api/queries";
import { Field } from "@/components/Field";
import { applyApiError, submitOnce, useFormFailure } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { errors } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate } from "@/test/gateway";
import { ok } from "@/test/handlers";
import {
  besideTheDialog,
  counted,
  expectOneRequestWhileTheDialogStays,
  expectTheDialogStays,
  held,
  sendTwiceAtOnce,
  settle,
} from "@/test/pages";
import { renderWithApp } from "@/test/render";

const DESCRIPTION = "Members are added on the page of the team.";

interface TeamFormProps {
  create: ReturnType<typeof useCreateTeam>;
  /** Whether there is something to send. */
  ready: boolean;
  onDone: () => void;
  onCancel: () => void;
}

// A form as the pages write it: mounted while its dialog is open.
function TeamForm({ create, ready, onDone, onCancel }: TeamFormProps) {
  const { mutateAsync } = create;
  const form = useForm({
    defaultValues: { name: "" },
    onSubmit: async ({ value }) => {
      try {
        await mutateAsync(value);
        onDone();
      } catch (error) {
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  return (
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
      <FormDialogFooter
        running={create.isPending}
        submit="Create team"
        submitting="Creating the team"
        disabled={!ready}
        onCancel={onCancel}
      />
    </form>
  );
}

/** A page with one dialog. It says how often the dialog was left, and how often it was done. */
function Teams({ ready = true }: { ready?: boolean }) {
  const create = useCreateTeam();
  const [open, setOpen] = useState(false);
  const [left, setLeft] = useState(0);
  const [done, setDone] = useState(0);
  function close() {
    setOpen(false);
    create.reset();
  }
  return (
    <main>
      <Button
        type="button"
        onClick={() => {
          setOpen(true);
        }}
      >
        New team
      </Button>
      <output aria-label="Left">{left}</output>
      <output aria-label="Done">{done}</output>
      <FormDialog
        open={open}
        running={create.isPending}
        title="New team"
        description={DESCRIPTION}
        onCancel={() => {
          close();
          setLeft((count) => count + 1);
        }}
      >
        <TeamForm
          create={create}
          ready={ready}
          onCancel={() => {
            close();
            setLeft((count) => count + 1);
          }}
          onDone={() => {
            close();
            setDone((count) => count + 1);
          }}
        />
      </FormDialog>
    </main>
  );
}

function opener(): HTMLElement {
  return screen.getByRole("button", { name: "New team", hidden: true });
}

async function openDialog(): Promise<HTMLElement> {
  await userEvent.click(opener());
  return screen.findByRole("dialog", { name: "New team" });
}

function count(name: "Left" | "Done"): string {
  return screen.getByRole("status", { name, hidden: true }).textContent;
}

async function closed(): Promise<void> {
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

/** The ways out of a dialog that send nothing. */
const WAYS_OUT = {
  Cancel: (dialog: HTMLElement) =>
    userEvent.click(within(dialog).getByRole("button", { name: "Cancel" })),
  "the X": (dialog: HTMLElement) =>
    userEvent.click(within(dialog).getByRole("button", { name: "Close" })),
  Escape: () => userEvent.keyboard("{Escape}"),
  "a click beside the dialog": () => userEvent.click(besideTheDialog()),
} as const;
const waysOut = Object.entries(WAYS_OUT);

describe("form dialog", () => {
  test("it is named by its title and described by its description, and holds the focus", async () => {
    await renderWithApp(<Teams />);
    const dialog = await openDialog();
    expect(dialog).toHaveAccessibleDescription(DESCRIPTION);
    expect(within(dialog).getByRole("heading", { name: "New team" })).toBeInTheDocument();
    await waitFor(() => {
      expect(dialog).toContainElement(document.activeElement as HTMLElement);
    });
    // The focus does not leave the dialog: after the last control comes the first.
    const controls = within(dialog).getAllByRole("button");
    const last = controls[controls.length - 1];
    if (last === undefined) throw new Error("the dialog has no controls");
    last.focus();
    await userEvent.tab();
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
    expect(last).not.toHaveFocus();
  });

  test.each(waysOut)("%s leaves it, sends nothing, and the focus is back on what opened it", async (_, leave) => {
    const posts = counted("post", "/api/teams", () =>
      ok("post", "/api/teams", 201, fixtures.teams.growth),
    );
    await renderWithApp(<Teams />);
    const dialog = await openDialog();
    await userEvent.type(within(dialog).getByLabelText("Name"), "Growth");
    await leave(dialog);
    await closed();
    expect(count("Left")).toBe("1");
    expect(count("Done")).toBe("0");
    expect(posts.calls).toBe(0);
    await waitFor(() => {
      expect(opener()).toHaveFocus();
    });
  });

  test("every opening starts with a new form", async () => {
    await renderWithApp(<Teams />);
    const dialog = await openDialog();
    await userEvent.type(within(dialog).getByLabelText("Name"), "Growth");
    await WAYS_OUT.Cancel(dialog);
    await closed();
    const again = await openDialog();
    expect(within(again).getByLabelText("Name")).toHaveValue("");
  });

  test("while its request runs it stays: no Escape, no click beside it, no Cancel, no X, no second submit; one request", async () => {
    const door = gate();
    const posts = counted("post", "/api/teams", async () => {
      await door.opened;
      return ok("post", "/api/teams", 201, fixtures.teams.growth);
    });
    await renderWithApp(<Teams />);
    const dialog = await openDialog();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, "Growth");
    // Two submits before anything of the first is on the screen.
    sendTwiceAtOnce(dialog);
    expect(await within(dialog).findByRole("button", { name: "Creating the team" })).toBeDisabled();
    await expectTheDialogStays(dialog, name);
    expect(name).toHaveValue("Growth");
    expect(posts.calls).toBe(1);
    expect(count("Left")).toBe("0");

    act(() => {
      door.open();
    });
    await closed();
    await settle();
    expect(posts.calls).toBe(1);
    expect([count("Left"), count("Done")]).toEqual(["0", "1"]);
  });

  test("after a refusal it can be left again, and nothing was sent twice", async () => {
    const request = held("post", "/api/teams");
    const app = await renderWithApp(<Teams />);
    const dialog = await openDialog();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, "Growth");
    await expectOneRequestWhileTheDialogStays(dialog, name, "Creating the team", request);
    expect([count("Left"), count("Done")]).toEqual(["1", "0"]);
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
  });

  test.each(waysOut)("after a refusal %s leaves it", async (_, leave) => {
    const request = held("post", "/api/teams");
    await renderWithApp(<Teams />);
    const dialog = await openDialog();
    await userEvent.type(within(dialog).getByLabelText("Name"), "Growth");
    await userEvent.click(within(dialog).getByRole("button", { name: "Create team" }));
    await within(dialog).findByRole("button", { name: "Creating the team" });
    request.answer();
    await within(dialog).findByText(errors.forbidden.body.error.message);
    await waitFor(() => {
      expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
    });
    await leave(dialog);
    await closed();
    expect(count("Left")).toBe("1");
    expect(request.calls).toBe(1);
  });

  test("after a refusal the form can be sent again, with what it held", async () => {
    const request = held("post", "/api/teams");
    await renderWithApp(<Teams />);
    const dialog = await openDialog();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, "Growth");
    await userEvent.type(name, "{Enter}");
    await within(dialog).findByRole("button", { name: "Creating the team" });
    request.answer();
    await within(dialog).findByText(errors.forbidden.body.error.message);
    expect(name).toHaveValue("Growth");

    const posts = counted("post", "/api/teams", () =>
      ok("post", "/api/teams", 201, fixtures.teams.growth),
    );
    await userEvent.type(name, "{Enter}");
    await closed();
    expect(posts.bodies).toEqual([{ name: "Growth" }]);
    expect([count("Left"), count("Done")]).toEqual(["0", "1"]);
  });

  test("the footer: Cancel, and a submit button that is off while there is nothing to send", async () => {
    await renderWithApp(<Teams ready={false} />);
    const dialog = await openDialog();
    const buttons = within(dialog)
      .getAllByRole("button")
      .filter((button) => button.textContent !== "Close");
    expect(buttons.map((button) => button.textContent)).toEqual(["Cancel", "Create team"]);
    const [cancel, submit] = buttons;
    expect(cancel).toHaveAttribute("type", "button");
    expect(cancel).toBeEnabled();
    expect(submit).toHaveAttribute("type", "submit");
    expect(submit).toBeDisabled();
  });

  test.each([390, 1280])("at width %s it fits the screen, and its controls are large enough to touch", async (width) => {
    await renderWithApp(<Teams />, { width });
    const dialog = await openDialog();
    // As every dialog: no wider and no higher than the screen less its margin.
    expect(dialog).toHaveClass("max-w-[calc(100%-2rem)]", "md:max-w-sm");
    expect(dialog).toHaveClass("max-h-[calc(100svh-2rem)]", "overflow-y-auto");
    for (const name of ["Cancel", "Create team"]) {
      expect(within(dialog).getByRole("button", { name })).toHaveClass("min-h-11", "md:min-h-8");
    }
  });
});
