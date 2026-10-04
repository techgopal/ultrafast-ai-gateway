import { useForm } from "@tanstack/react-form";
import { useId, useRef } from "react";
import { useUpdateSettings } from "@/api/queries";
import { ConsoleRefusal } from "@/api/errors";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormError } from "@/components/FormError";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

type Settings = components["schemas"]["SettingsView"];

export const SESSION_DONE = "Settings saved.";
export const SESSION_RULE = "Enter a whole number from 1 to 720.";
export const SESSION_HINT =
  "From 1 to 720 hours. Applies to sign-ins from now on; sessions that exist keep theirs.";

function SessionForm({ hours }: { hours: number }) {
  const update = useUpdateSettings();
  const toast = useToast();
  const { mutateAsync, reset } = update;
  const form = useForm({
    defaultValues: { session_hours: String(hours) },
    onSubmit: async ({ value }) => {
      try {
        const typed = value.session_hours.trim();
        const count = /^\d+$/.test(typed) ? Number(typed) : 0;
        if (count < 1 || count > 720) {
          throw new ConsoleRefusal(SESSION_RULE, "session_hours");
        }
        await mutateAsync({ session_hours: count });
        reset();
        toast(SESSION_DONE);
      } catch (error) {
        reset();
        applyApiError(form, error);
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
      aria-label="Sign-in"
      noValidate
      className="flex max-w-md flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="session_hours">
        {(field) => (
          <Field
            label="Session lifetime (hours)"
            name={field.name}
            hint={SESSION_HINT}
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                inputMode="numeric"
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <div>
        <Button type="submit" className={control} disabled={update.isPending}>
          {update.isPending ? "Saving" : "Save sign-in settings"}
        </Button>
      </div>
    </form>
  );
}

/** What the gateway was started with and what is built in: shown, not editable. */
function ReadOnly({ settings }: { settings: Settings }) {
  const { window_minutes: minutes, max_per_email: email, max_per_address: address } =
    settings.login_limits;
  return (
    <div className="flex max-w-prose flex-col gap-4">
      <div className="flex flex-col gap-1">
        <h3 className="text-sm font-medium">Trusted proxies</h3>
        {settings.trusted_proxies.length === 0 ? (
          <p className="text-sm">None. Forwarding headers are ignored.</p>
        ) : (
          <ul className="flex flex-col gap-1 font-mono text-sm">
            {settings.trusted_proxies.map((network) => (
              <li key={network}>{network}</li>
            ))}
          </ul>
        )}
        <p className="text-sm text-muted-foreground">
          Networks whose forwarding headers are believed, to find the address of the client. Set with
          the --trusted-proxy flag when the gateway starts; it cannot be changed here.
        </p>
      </div>
      <div className="flex flex-col gap-1">
        <h3 className="text-sm font-medium">Sign-in limits</h3>
        <p className="text-sm">
          {`A sign-in is refused after ${String(email)} failed attempts for one email, or ${String(address)} from one address, within ${String(minutes)} minutes.`}
        </p>
        <p className="text-sm text-muted-foreground">Built in; they cannot be changed here.</p>
      </div>
    </div>
  );
}

/** How long a sign-in lasts, and what guards signing in. */
export function SignInSection({ settings }: { settings: Settings }) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-4">
      <h2 id={headingId} className="text-lg font-medium">
        Sign-in
      </h2>
      <SessionForm hours={settings.session_hours} />
      <ReadOnly settings={settings} />
    </section>
  );
}
