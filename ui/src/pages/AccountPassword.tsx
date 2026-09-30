import { useForm } from "@tanstack/react-form";
import { useEffect, useRef, useState } from "react";
import { ApiError, ConsoleRefusal, messageOfError } from "@/api/errors";
import { useChangePassword } from "@/api/queries";
import { useSessionControl } from "@/auth/session";
import { PASSWORD_POLICY, PASSWORDS_DIFFER, TOO_MANY_ATTEMPTS } from "@/components/AuthForm";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { applyApiError, onField, onStatus, useFormFailure, useSubmit } from "@/components/form";
import { FormError } from "@/components/FormError";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export const PASSWORD_CHANGED =
  "Password changed. Your other sessions and all your access tokens were ended.";
export const WRONG_CURRENT_PASSWORD = "Current password is incorrect.";

const PASSWORD_FIELDS = ["current_password", "new_password", "confirm_password"] as const;

const NO_PASSWORDS = { current_password: "", new_password: "", confirm_password: "" };

/**
 * A 401 of the password call that is not about the password: the session is
 * over. The client takes no 401 of this call for the end of a session, since
 * a wrong current password is a 401 too; the two differ by their code.
 */
function saysTheSessionIsOver(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401 && error.code !== "invalid_credentials";
}

/**
 * What the gateway refused, in the words of this form. A wrong current
 * password is said by its field. Too many attempts are about no field: the
 * gateway refuses before it looks at the password, and the limit is the one
 * of the sign-in. It is said as on the sign-in page: by the status, in the
 * same text, at the top of the form, where it stays until the form is sent
 * again.
 */
function inTheWordsOfTheForm(error: unknown): unknown {
  const wrong = onField(error, "invalid_credentials", "current_password", WRONG_CURRENT_PASSWORD);
  return onStatus(wrong, 429, TOO_MANY_ATTEMPTS);
}

/**
 * Changes the password of who is signed in. The session this is done in goes
 * on; the gateway ends the other sessions and revokes all access tokens.
 *
 * The passwords are held by the form while they are typed, and by nothing
 * after an answer: the three fields are emptied after a success and after a
 * refusal, and the mutation is reset. An empty form is never sent, so what
 * was refused cannot be sent again by a second press.
 *
 * That matters for the limit of the gateway, which is the one of the
 * sign-in: an attempt with a wrong current password counts against it, and
 * an empty current password is a wrong one. These do not count: a new
 * password the gateway does not take (a 422, which comes after the current
 * password was found right), a refusal for too many attempts (a 429), and
 * a 401 of a call without a session.
 */
export function PasswordForm({ email }: { email: string }) {
  const change = useChangePassword();
  const { mutateAsync, reset } = change;
  const { end } = useSessionControl();
  const toast = useToast();
  const [refusals, setRefusals] = useState(0);
  const [changes, setChanges] = useState(0);
  const form = useForm({
    defaultValues: NO_PASSWORDS,
    onSubmit: async ({ value }) => {
      if (value.new_password !== value.confirm_password) {
        // Nothing is sent, so nothing is emptied: the user corrects what they typed.
        applyApiError(form, new ConsoleRefusal(PASSWORDS_DIFFER, "confirm_password"));
        return;
      }
      let refused = false;
      let refusal: unknown;
      try {
        await mutateAsync({
          current_password: value.current_password,
          new_password: value.new_password,
        });
      } catch (error) {
        refused = true;
        refusal = error;
      }
      for (const name of PASSWORD_FIELDS) form.setFieldValue(name, "");
      reset();
      if (!refused) {
        toast(PASSWORD_CHANGED);
        setChanges((count) => count + 1);
        return;
      }
      if (saysTheSessionIsOver(refusal)) {
        end("expired");
        return;
      }
      // An answer of a session that is over says nothing, and moves no focus.
      if (messageOfError(refusal) === null) return;
      // After the fields were emptied: the error of a field is about the
      // field as it is now, and goes when something is typed into it.
      applyApiError(form, inTheWordsOfTheForm(refusal));
      setRefusals((count) => count + 1);
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const currentRef = useRef<HTMLInputElement>(null);
  const submitRef = useRef<HTMLButtonElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const submit = useSubmit(form);

  // All three fields are empty after a refusal: the user starts again at the
  // first, whichever field the refusal was about. This runs after the focus
  // that `useFormFailure` sets.
  useEffect(() => {
    if (refusals > 0) currentRef.current?.focus();
  }, [refusals]);

  // The button that was pressed was disabled while the call ran, and so lost
  // the focus: after a success it has it again. A focus that is somewhere,
  // as in the field from which the form was sent, stays where it is.
  useEffect(() => {
    if (changes > 0 && document.activeElement === document.body) submitRef.current?.focus();
  }, [changes]);

  return (
    // The browser checks that the fields are filled: there is no `noValidate`.
    <form
      ref={formRef}
      aria-label="Change password"
      className="flex max-w-md flex-col gap-4"
      onSubmit={(event) => {
        // A form with an empty field is not sent, however it was submitted.
        if (PASSWORD_FIELDS.some((name) => form.state.values[name] === "")) {
          event.preventDefault();
          return;
        }
        submit(event);
      }}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      {/* Whose password it is, for the password manager of the browser. It is not shown and not sent. */}
      <input type="text" name="username" autoComplete="username" value={email} readOnly hidden />
      <form.Field name="current_password">
        {(field) => (
          <Field
            label="Current password"
            name={field.name}
            required
            error={failure.fieldError(field.name)}
          >
            <Input
              ref={currentRef}
              type="password"
              autoComplete="current-password"
              className={`${control} w-full`}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <form.Field name="new_password">
        {(field) => (
          <Field
            label="New password"
            name={field.name}
            required
            hint={PASSWORD_POLICY}
            error={failure.fieldError(field.name)}
          >
            <Input
              type="password"
              autoComplete="new-password"
              className={`${control} w-full`}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <form.Field name="confirm_password">
        {(field) => (
          <Field
            label="Confirm new password"
            name={field.name}
            required
            error={failure.fieldError(field.name)}
          >
            <Input
              type="password"
              autoComplete="new-password"
              className={`${control} w-full`}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      <Button
        ref={submitRef}
        type="submit"
        className={`${control} w-fit`}
        disabled={change.isPending}
      >
        {change.isPending ? "Changing the password" : "Change password"}
      </Button>
    </form>
  );
}
