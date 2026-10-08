import { useForm } from "@tanstack/react-form";
import { useRef } from "react";
import type { useCreateAlertChannel, useUpdateAlertChannel } from "@/api/queries";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, onField, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";

type Channel = components["schemas"]["ChannelView"];
type UpdateChannelRequest = components["schemas"]["UpdateChannelRequest"];

export const CHANNEL_DESCRIPTION =
  "The gateway posts each alert to the channel's URL, signed with a secret that only you see.";
export const URL_HINT =
  "Where to post. The gateway stores it encrypted and shows only its host.";
export const KEEP_URL_HINT = "Leave empty to keep the current URL. The gateway never shows it.";
export const NEEDS_URL_HINT = "This channel has no URL yet. Set one to be able to enable it.";

export const CHANNEL_KINDS = [
  ["webhook", "Webhook"],
  ["slack", "Slack"],
] as const;

export function channelKindText(kind: string): string {
  return CHANNEL_KINDS.find(([value]) => value === kind)?.[1] ?? kind;
}

interface FormProps {
  /** The channel that is changed; `null` for a new one. */
  channel: Channel | null;
  create: ReturnType<typeof useCreateAlertChannel>;
  update: ReturnType<typeof useUpdateAlertChannel>;
  /** The channel was made; the answer holds its signing secret, shown once. */
  onCreated: (secret: string) => void;
  /** The channel was saved; `changed` is false when there was nothing to send. */
  onUpdated: (changed: boolean) => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts from the channel as it is.
function ChannelForm({ channel, create, update, onCreated, onUpdated, onCancel }: FormProps) {
  const running = create.isPending || update.isPending;
  const form = useForm({
    defaultValues: {
      name: channel?.name ?? "",
      kind: channel?.kind ?? "webhook",
      url: "",
    },
    onSubmit: async ({ value }) => {
      try {
        if (channel === null) {
          const made = await create.mutateAsync({
            name: value.name,
            kind: value.kind,
            url: value.url.trim(),
          });
          // The gateway has the URL now: the form holds it no longer.
          form.setFieldValue("url", "");
          create.reset();
          onCreated(made.secret);
          return;
        }
        const changes: UpdateChannelRequest = {};
        if (value.name.trim() !== channel.name) changes.name = value.name;
        if (value.url.trim() !== "") changes.url = value.url.trim();
        if (Object.keys(changes).length === 0) {
          onUpdated(false);
          return;
        }
        await update.mutateAsync({ id: channel.id, body: changes });
        form.setFieldValue("url", "");
        update.reset();
        onUpdated(true);
      } catch (error) {
        // The mutation does not keep what it sent. The field does.
        create.reset();
        update.reset();
        applyApiError(form, onField(error, "alert_channel_exists", "name"));
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const onSubmit = useSubmit(form);

  return (
    <form
      ref={formRef}
      aria-label={channel === null ? "Add channel" : "Edit channel"}
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="name">
        {(field) => (
          <Field label="Name" name={field.name} required error={failure.fieldError(field.name)}>
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
      {channel === null ? (
        <form.Field name="kind">
          {(field) => (
            <Field group label="Kind" name={field.name} error={failure.fieldError(field.name)}>
              {({ id, name, ...described }) => (
                <RadioGroup
                  {...described}
                  id={id}
                  name={name}
                  value={field.state.value}
                  onValueChange={field.handleChange}
                >
                  {CHANNEL_KINDS.map(([value, label]) => (
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
      ) : null}
      <form.Field name="url">
        {(field) => (
          <Field
            label="URL"
            name={field.name}
            required={channel === null}
            hint={
              channel === null
                ? URL_HINT
                : channel.url_host === ""
                  ? NEEDS_URL_HINT
                  : KEEP_URL_HINT
            }
            error={failure.fieldError(field.name)}
          >
            <Input
              inputMode="url"
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
      <FormDialogFooter
        running={running}
        submit={channel === null ? "Add channel" : "Save channel"}
        submitting={channel === null ? "Adding the channel" : "Saving the channel"}
        onCancel={onCancel}
      />
    </form>
  );
}

export function ChannelDialog({ open, ...form }: FormProps & { open: boolean }) {
  return (
    <FormDialog
      open={open}
      running={form.create.isPending || form.update.isPending}
      title={form.channel === null ? "Add channel" : "Edit channel"}
      description={CHANNEL_DESCRIPTION}
      onCancel={form.onCancel}
    >
      <ChannelForm {...form} />
    </FormDialog>
  );
}
