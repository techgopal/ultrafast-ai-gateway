import { useRef, useState, type SyntheticEvent } from "react";
import { ApiError } from "@/api/errors";
import { useLogin } from "@/api/queries";
import { useSessionControl } from "@/auth/session";
import {
  AuthPage,
  control,
  Field,
  formColumn,
  FormError,
  Notice,
  textOf,
  useFocusOnFailure,
} from "@/components/AuthForm";
import { Button } from "@/components/ui/button";

function messageOf(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 401) return "Email or password is incorrect.";
    if (error.status === 429) return "Too many attempts. Try again in a few minutes.";
  }
  return error instanceof Error ? error.message : "Something went wrong.";
}

/**
 * The password is held by its field only, never by the state of the page.
 * Where the user goes after the sign-in is decided by the route.
 */
export function SignIn() {
  const { begin, notice } = useSessionControl();
  const login = useLogin();
  const { mutateAsync, reset } = login;
  const password = useRef<HTMLInputElement>(null);
  const running = useRef(false);
  const form = useRef<HTMLFormElement>(null);
  const error = useRef<HTMLDivElement>(null);
  const failed = useFocusOnFailure(form, error);
  const [message, setMessage] = useState<string | null>(null);

  async function signIn(data: FormData) {
    if (running.current) return;
    running.current = true;
    setMessage(null);
    try {
      const session = await mutateAsync({
        email: textOf(data, "email"),
        password: textOf(data, "password"),
      });
      begin(session.csrf_token);
    } catch (reason) {
      setMessage(messageOf(reason));
      failed();
    } finally {
      running.current = false;
      if (password.current !== null) password.current.value = "";
      // The mutation holds the request and the answer until it is reset.
      reset();
    }
  }

  function submit(event: SyntheticEvent<HTMLFormElement>) {
    event.preventDefault();
    void signIn(new FormData(event.currentTarget));
  }

  return (
    <AuthPage title="Sign in">
      {notice === null ? null : <Notice>{notice}</Notice>}
      {message === null ? null : <FormError ref={error}>{message}</FormError>}
      <form ref={form} aria-label="Sign in" className={formColumn} onSubmit={submit}>
        <Field label="Email" name="email" type="email" autoComplete="username" required />
        <Field
          ref={password}
          label="Password"
          name="password"
          type="password"
          autoComplete="current-password"
          required
        />
        <Button type="submit" className={control} disabled={login.isPending}>
          {login.isPending ? "Signing in" : "Sign in"}
        </Button>
      </form>
    </AuthPage>
  );
}
