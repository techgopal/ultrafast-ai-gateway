import { Link, useNavigate } from "@tanstack/react-router";
import { useRef, useState, type SyntheticEvent } from "react";
import { ApiError } from "@/api/errors";
import { useSetup } from "@/api/queries";
import { useSessionControl } from "@/auth/session";
import {
  AuthPage,
  control,
  Field,
  formColumn,
  FormError,
  PASSWORD_POLICY,
  PASSWORDS_DIFFER,
  textOf,
} from "@/components/AuthForm";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

export const SETUP_DONE_NOTICE = "The admin account is created. Sign in to continue.";

type FieldErrors = Readonly<Record<string, string>>;

/** Creates the first admin. The passwords are held by their fields only. */
export function Setup() {
  const { needsSetup, markSetUp, announce } = useSessionControl();
  const navigate = useNavigate();
  const setup = useSetup();
  const { mutateAsync, reset } = setup;
  const password = useRef<HTMLInputElement>(null);
  const confirm = useRef<HTMLInputElement>(null);
  const running = useRef(false);
  const [message, setMessage] = useState<string | null>(null);
  const [errors, setErrors] = useState<FieldErrors>({});

  function clearPasswords() {
    for (const field of [password.current, confirm.current]) {
      if (field !== null) field.value = "";
    }
  }

  async function create(form: FormData) {
    if (running.current) return;
    setMessage(null);
    if (textOf(form, "password") !== textOf(form, "confirm")) {
      setErrors({ confirm: PASSWORDS_DIFFER });
      return;
    }
    setErrors({});
    running.current = true;
    let created = false;
    try {
      await mutateAsync({
        name: textOf(form, "name"),
        email: textOf(form, "email"),
        password: textOf(form, "password"),
      });
      created = true;
    } catch (error) {
      if (error instanceof ApiError && error.code === "already_set_up") {
        markSetUp();
      } else if (error instanceof ApiError && Object.keys(error.fields).length > 0) {
        setErrors(error.fields);
      } else {
        setMessage(error instanceof Error ? error.message : "Something went wrong.");
      }
    } finally {
      running.current = false;
      clearPasswords();
      // The mutation holds the request until it is reset.
      reset();
    }
    if (created) {
      markSetUp();
      announce(SETUP_DONE_NOTICE);
      await navigate({ to: "/sign-in" });
    }
  }

  function submit(event: SyntheticEvent<HTMLFormElement>) {
    event.preventDefault();
    void create(new FormData(event.currentTarget));
  }

  if (!needsSetup) {
    return (
      <AuthPage title="Set up the gateway">
        <Alert>
          <AlertTitle>Setup is already complete.</AlertTitle>
          <AlertDescription>
            <p>
              This gateway has its admin account. <Link to="/sign-in">Sign in</Link>
            </p>
          </AlertDescription>
        </Alert>
      </AuthPage>
    );
  }

  return (
    <AuthPage
      title="Set up the gateway"
      description="Create the first admin account of this gateway."
    >
      {message === null ? null : <FormError>{message}</FormError>}
      <form aria-label="Set up the gateway" className={formColumn} onSubmit={submit}>
        <Field label="Name" name="name" autoComplete="name" required error={errors.name} />
        <Field
          label="Email"
          name="email"
          type="email"
          autoComplete="username"
          required
          error={errors.email}
        />
        <Field
          ref={password}
          label="Password"
          name="password"
          type="password"
          autoComplete="new-password"
          required
          hint={PASSWORD_POLICY}
          error={errors.password}
        />
        <Field
          ref={confirm}
          label="Confirm password"
          name="confirm"
          type="password"
          autoComplete="new-password"
          required
          error={errors.confirm}
        />
        <Button type="submit" className={control} disabled={setup.isPending}>
          {setup.isPending ? "Creating the account" : "Create admin account"}
        </Button>
      </form>
    </AuthPage>
  );
}
