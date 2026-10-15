import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import { ConsoleRefusal } from "@/api/errors";
import type { useCreateMcpServer, useUpdateMcpServer } from "@/api/queries";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, onField, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { AUTH_KINDS, updateOf, type ServerValues } from "@/lib/mcp";

type Server = components["schemas"]["McpServerView"];
type CreateRequest = components["schemas"]["CreateMcpServerRequest"];

export const ENTER_A_NAME = "Enter the server's name.";
export const ENTER_AN_ADDRESS = "Enter the address of the server.";
export const ENTER_A_SECRET = "Enter the secret.";
export const ENTER_A_HEADER = "Enter the name of the header.";
export const NAME_HINT = "Letters a to z, digits and dashes. It is part of the endpoint and cannot be changed.";
export const SECRET_KEPT = "Leave empty to keep the secret that is stored.";

interface Values extends ServerValues {
  name: string;
}

function refuse(message: string, field: string): ConsoleRefusal {
  return new ConsoleRefusal(message, field);
}

function createOf(values: Values): CreateRequest {
  const name = values.name.trim();
  const url = values.url.trim();
  if (name === "") throw refuse(ENTER_A_NAME, "name");
  if (url === "") throw refuse(ENTER_AN_ADDRESS, "url");
  const body: CreateRequest = {
    name,
    url,
    description: values.description,
    auth_kind: values.auth_kind,
  };
  if (values.auth_kind === "header") {
    if (values.auth_header.trim() === "") throw refuse(ENTER_A_HEADER, "auth_header");
    body.auth_header = values.auth_header.trim();
  }
  if (values.auth_kind !== "none") {
    if (values.secret === "") throw refuse(ENTER_A_SECRET, "secret");
    body.secret = values.secret;
  }
  return body;
}

interface FormProps {
  /** The server being edited; none when one is added. */
  server: Server | null;
  create: ReturnType<typeof useCreateMcpServer>;
  update: ReturnType<typeof useUpdateMcpServer>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts from the server as it is.
function ServerForm({ server, create, update, onDone, onCancel }: FormProps) {
  const adding = server === null;
  const form = useForm({
    defaultValues: {
      name: server?.name ?? "",
      url: server?.url ?? "",
      description: server?.description ?? "",
      auth_kind: server?.auth_kind ?? "none",
      auth_header: server?.auth_header ?? "",
      secret: "",
    } satisfies Values,
    onSubmit: async ({ value }) => {
      try {
        if (server === null) {
          await create.mutateAsync(createOf(value));
        } else {
          const body = updateOf(server, value);
          if (Object.keys(body).length > 0) await update.mutateAsync({ id: server.id, body });
        }
        onDone();
      } catch (error) {
        applyApiError(form, onField(error, "mcp_server_exists", "name"));
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const onSubmit = useSubmit(form);
  const running = create.isPending || update.isPending;

  return (
    <form
      ref={formRef}
      aria-label={adding ? "Add MCP server" : "Edit MCP server"}
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      {adding ? (
        <form.Field name="name">
          {(field) => (
            <Field label="Name" name={field.name} required hint={NAME_HINT} error={failure.fieldError(field.name)}>
              <Input
                autoComplete="off"
                autoCapitalize="none"
                spellCheck={false}
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )}
        </form.Field>
      ) : null}
      <form.Field name="url">
        {(field) => (
          <Field
            label="Address"
            name={field.name}
            required
            hint="The streamable HTTP endpoint of the server."
            error={failure.fieldError(field.name)}
          >
            <Input
              type="url"
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              className={control}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <form.Field name="description">
        {(field) => (
          <Field label="Description" name={field.name} error={failure.fieldError(field.name)}>
            <Input
              autoComplete="off"
              className={control}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <form.Field name="auth_kind">
        {(field) => (
          <Field group label="Authentication" name={field.name} error={failure.fieldError(field.name)}>
            {({ id, name, ...described }) => (
              <RadioGroup {...described} id={id} name={name} value={field.state.value} onValueChange={field.handleChange}>
                {AUTH_KINDS.map(([value, label]) => (
                  <Label key={value} htmlFor={`${id}-${value}`} className={control}>
                    <RadioGroupItem id={`${id}-${value}`} value={value} />
                    {label}
                  </Label>
                ))}
              </RadioGroup>
            )}
          </Field>
        )}
      </form.Field>
      <form.Subscribe selector={(state) => state.values.auth_kind}>
        {(kind) => (
          <>
            {kind === "header" ? (
              <form.Field name="auth_header">
                {(field) => (
                  <Field label="Header name" name={field.name} required error={failure.fieldError(field.name)}>
                    <Input
                      autoComplete="off"
                      autoCapitalize="none"
                      spellCheck={false}
                      className={control}
                      value={field.state.value}
                      onBlur={field.handleBlur}
                      onChange={(event) => {
                        field.handleChange(event.target.value);
                      }}
                    />
                  </Field>
                )}
              </form.Field>
            ) : null}
            {kind === "none" ? null : (
              <form.Field name="secret">
                {(field) => (
                  <Field
                    label="Secret"
                    name={field.name}
                    required={adding || server.has_secret !== true}
                    hint={adding || server.has_secret !== true ? undefined : SECRET_KEPT}
                    error={failure.fieldError(field.name)}
                  >
                    <Input
                      type="password"
                      autoComplete="off"
                      className={control}
                      value={field.state.value}
                      onBlur={field.handleBlur}
                      onChange={(event) => {
                        field.handleChange(event.target.value);
                      }}
                    />
                  </Field>
                )}
              </form.Field>
            )}
          </>
        )}
      </form.Subscribe>
      <FormDialogFooter
        running={running}
        submit={adding ? "Add server" : "Save server"}
        submitting={adding ? "Adding the server" : "Saving the server"}
        onCancel={onCancel}
      />
    </form>
  );
}

export function ServerDialog({
  open,
  adding,
  ...form
}: FormProps & { open: boolean; adding: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.create.isPending || form.update.isPending}
      title={adding ? "Add MCP server" : "Edit MCP server"}
      description={
        adding
          ? "The tools of a new server start off, and nobody has access to them."
          : "Changing the address to another host needs the secret again."
      }
      onCancel={form.onCancel}
    >
      <ServerForm {...form} server={adding ? null : form.server} />
    </FormDialog>
  );
}
