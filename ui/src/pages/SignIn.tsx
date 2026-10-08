import { useNavigate } from "@tanstack/react-router";
import { useEffect, useRef, useState, type SyntheticEvent } from "react";
import { ApiError, messageOfError } from "@/api/errors";
import { useLogin, useSignInMethods } from "@/api/queries";
import { safePath } from "@/auth/guards";
import { useSessionControl } from "@/auth/session";
import {
  AuthPage,
  control,
  Field,
  formColumn,
  FormError,
  Notice,
  textOf,
  TOO_MANY_ATTEMPTS,
  useFocusOnFailure,
} from "@/components/AuthForm";
import { Button } from "@/components/ui/button";

export const SESSION_NOT_KEPT =
  "Signed in, but this browser did not keep the session. Open the console over HTTPS, or start the gateway with --insecure-cookies on a trusted network.";

const SSO_INTERRUPTED = "Sign-in took too long or was interrupted. Try again.";
const SSO_NOT_SET_UP = "Single sign-on is not set up correctly. Ask an admin.";

/**
 * What the gateway means by `sso_error`, in words. Only the codes it sends
 * are known; anything else in the address is said in general words and is
 * never shown as it came.
 */
export const SSO_MESSAGES: Readonly<Record<string, string>> = {
  state: SSO_INTERRUPTED,
  expired: SSO_INTERRUPTED,
  idp: "Your identity provider refused the sign-in.",
  token: SSO_NOT_SET_UP,
  config: SSO_NOT_SET_UP,
  not_allowed: "Your account is not allowed to sign in here. Ask an admin to invite you.",
  disabled: "Your account is disabled.",
  rate_limited: "Too many sign-in attempts. Try again in a few minutes.",
};
export const SSO_UNKNOWN = "Single sign-on did not work. Try again.";

function ssoMessage(code: string | undefined): string | null {
  if (code === undefined) return null;
  return Object.hasOwn(SSO_MESSAGES, code) ? (SSO_MESSAGES[code] ?? SSO_UNKNOWN) : SSO_UNKNOWN;
}

function messageOf(error: unknown): string | null {
  if (error instanceof ApiError) {
    if (error.status === 401) return "Email or password is incorrect.";
    if (error.status === 429) return TOO_MANY_ATTEMPTS;
  }
  return messageOfError(error);
}

/**
 * The password is held by its field only, never by the state of the page.
 * Where the user goes after the sign-in is decided by the route.
 */
export function SignIn({ next, ssoError }: { next?: string | undefined; ssoError?: string | undefined }) {
  const { begin, notice } = useSessionControl();
  const login = useLogin();
  const { mutateAsync, reset } = login;
  const password = useRef<HTMLInputElement>(null);
  const running = useRef(false);
  const form = useRef<HTMLFormElement>(null);
  const error = useRef<HTMLDivElement>(null);
  const failed = useFocusOnFailure(form, error);
  const [message, setMessage] = useState<string | null>(() => ssoMessage(ssoError));
  const navigate = useNavigate();
  // The code is said once and then dropped from the address, so that a reload
  // or a failed password does not bring the same message back. `next` stays.
  useEffect(() => {
    if (ssoError === undefined) return;
    void navigate({
      to: "/sign-in",
      search: next === undefined ? {} : { next },
      replace: true,
    });
  }, [ssoError, next, navigate]);
  const methods = useSignInMethods();
  const sso = methods?.oidc ?? null;
  // A full-page navigation: the gateway sends the browser to the provider and
  // the provider sends it back, to the page the visitor came for.
  const start = `/api/auth/oidc/start?return_to=${encodeURIComponent(safePath(next) ?? "/")}`;

  async function signIn(data: FormData) {
    if (running.current) return;
    running.current = true;
    setMessage(null);
    try {
      const session = await mutateAsync({
        email: textOf(data, "email"),
        password: textOf(data, "password"),
      });
      // The session cookie is `Secure` unless the gateway was started with
      // `--insecure-cookies`; over plain HTTP the browser drops it.
      if (!(await begin(session.csrf_token))) {
        setMessage(SESSION_NOT_KEPT);
        failed();
      }
    } catch (reason) {
      const said = messageOf(reason);
      if (said !== null) {
        setMessage(said);
        failed();
      }
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
      {sso === null ? null : (
        <>
          <Button asChild variant="outline" className={control}>
            <a href={start}>{`Sign in with ${sso.label}`}</a>
          </Button>
          <p className="text-center text-sm text-muted-foreground">or</p>
        </>
      )}
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
