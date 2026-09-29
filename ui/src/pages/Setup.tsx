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
  useFocusOnFailure,
} from "@/components/AuthForm";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

export const SETUP_DONE_NOTICE = "The admin account is created. Sign in to continue.";

/** The fields of the form that the API may find fault with. */
const FIELDS = ["name", "email", "password"] as const;
type FieldErrors = Partial<Record<(typeof FIELDS)[number] | "confirm", string>>;

/** Creates the first admin. The passwords are held by their fields only. */
export function Setup() {
  const { markSetUp, announce } = useSessionControl();
  const navigate = useNavigate();
  const setup = useSetup();
  const { mutateAsync, reset } = setup;
  const password = useRef<HTMLInputElement>(null);
  const confirm = useRef<HTMLInputElement>(null);
  const running = useRef(false);
  const form = useRef<HTMLFormElement>(null);
  const error = useRef<HTMLDivElement>(null);
  const failed = useFocusOnFailure(form, error);
  const [alreadyDone, setAlreadyDone] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [errors, setErrors] = useState<FieldErrors>({});

  function clearPasswords() {
    for (const field of [password.current, confirm.current]) {
      if (field !== null) field.value = "";
    }
  }

  async function create(data: FormData) {
    if (running.current) return;
    setMessage(null);
    if (textOf(data, "password") !== textOf(data, "confirm")) {
      setErrors({ confirm: PASSWORDS_DIFFER });
      failed();
      return;
    }
    setErrors({});
    running.current = true;
    let created = false;
    try {
      await mutateAsync({
        name: textOf(data, "name"),
        email: textOf(data, "email"),
        password: textOf(data, "password"),
      });
      created = true;
    } catch (reason) {
      if (reason instanceof ApiError && reason.code === "already_set_up") {
        setAlreadyDone(true);
      } else if (reason instanceof ApiError) {
        const known: FieldErrors = {};
        for (const field of FIELDS) {
          const text = reason.fields[field];
          if (text !== undefined) known[field] = text;
        }
        setErrors(known);
        // What the form has no field for is said by the message of the API.
        const others = Object.keys(reason.fields).length - Object.keys(known).length;
        if (others > 0 || Object.keys(known).length === 0) setMessage(reason.message);
      } else {
        setMessage(reason instanceof Error ? reason.message : "Something went wrong.");
      }
      failed();
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

  if (alreadyDone) {
    return (
      <AuthPage title="Set up the gateway">
        <Alert ref={error} tabIndex={-1}>
          <AlertTitle>Setup is already complete.</AlertTitle>
          <AlertDescription>
            <p>
              This gateway has its admin account. <Link to="/sign-in" onClick={markSetUp}>
                Sign in
              </Link>
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
      {message === null ? null : <FormError ref={error}>{message}</FormError>}
      <form ref={form} aria-label="Set up the gateway" className={formColumn} onSubmit={submit}>
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
