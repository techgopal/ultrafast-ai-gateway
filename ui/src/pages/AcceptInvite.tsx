import { Link, useNavigate, useRouter } from "@tanstack/react-router";
import { useEffect, useRef, useState, type SyntheticEvent } from "react";
import { ApiError } from "@/api/errors";
import { useAcceptInvite } from "@/api/queries";
import { useSession, useSessionControl, useSignOut } from "@/auth/session";
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
import { Button } from "@/components/ui/button";

export const INVITE_ACCEPTED_NOTICE = "Your password is set. Sign in to continue.";
const INVALID_INVITE = "This invite link is not valid or has expired. Ask an admin for a new one.";

// Signing out mounts the pages anew. The token crosses that in here, in
// memory, from the click on "Sign out and continue" to the next mount of the
// page. At every other time this is `null`.
let carried: string | null = null;

function tokenIn(search: string): string | null {
  const token = new URLSearchParams(search).get("token");
  return token === null || token === "" ? null : token;
}

/**
 * Sets the password of an invited user. The token of the link is read once
 * and then taken out of the address; it lives in the state of this page and
 * nowhere else. Without it the link has to be opened again.
 */
export function AcceptInvite() {
  const router = useRouter();
  const navigate = useNavigate();
  const session = useSession();
  const { announce } = useSessionControl();
  const signOut = useSignOut();
  const accept = useAcceptInvite();
  const { mutateAsync, reset } = accept;
  const [token] = useState(() => tokenIn(router.state.location.searchStr) ?? carried);
  const password = useRef<HTMLInputElement>(null);
  const confirm = useRef<HTMLInputElement>(null);
  const running = useRef(false);
  const [message, setMessage] = useState<string | null>(null);
  const [passwordError, setPasswordError] = useState<string | undefined>(undefined);
  const [confirmError, setConfirmError] = useState<string | undefined>(undefined);

  useEffect(() => {
    carried = null;
    const { pathname, searchStr, hash } = router.state.location;
    const search = new URLSearchParams(searchStr);
    if (!search.has("token")) return;
    search.delete("token");
    const rest = search.toString();
    // Replaces the entry of the history: with a browser, `history.replaceState`.
    router.history.replace(
      pathname + (rest === "" ? "" : `?${rest}`) + (hash === "" ? "" : `#${hash}`),
    );
  }, [router]);

  async function setPassword(form: FormData) {
    if (running.current || token === null) return;
    setMessage(null);
    setPasswordError(undefined);
    if (textOf(form, "password") !== textOf(form, "confirm")) {
      setConfirmError(PASSWORDS_DIFFER);
      return;
    }
    setConfirmError(undefined);
    running.current = true;
    let accepted = false;
    try {
      await mutateAsync({ token, password: textOf(form, "password") });
      accepted = true;
    } catch (error) {
      if (error instanceof ApiError && error.status === 404) {
        setMessage(INVALID_INVITE);
      } else if (error instanceof ApiError && error.fields.password !== undefined) {
        setPasswordError(error.fields.password);
      } else {
        setMessage(error instanceof Error ? error.message : "Something went wrong.");
      }
    } finally {
      running.current = false;
      for (const field of [password.current, confirm.current]) {
        if (field !== null) field.value = "";
      }
      // The mutation holds the token and the password until it is reset.
      reset();
    }
    if (accepted) {
      announce(INVITE_ACCEPTED_NOTICE);
      await navigate({ to: "/sign-in" });
    }
  }

  function submit(event: SyntheticEvent<HTMLFormElement>) {
    event.preventDefault();
    void setPassword(new FormData(event.currentTarget));
  }

  if (token === null) {
    return (
      <AuthPage title="Accept your invite">
        <p className="text-sm text-muted-foreground">
          Open the link of your invite again. This page cannot be reloaded.
        </p>
      </AuthPage>
    );
  }

  if (session.status === "signedIn") {
    return (
      <AuthPage title="Accept your invite">
        <p className="text-sm">
          You are signed in as {session.me.user.email}. To accept the invite, you are signed out
          first.
        </p>
        <div className="flex flex-col gap-2">
          <Button
            type="button"
            className={control}
            onClick={() => {
              carried = token;
              void signOut();
            }}
          >
            Sign out and continue
          </Button>
          <Button asChild variant="outline" className={control}>
            <Link to="/">Stay signed in</Link>
          </Button>
        </div>
      </AuthPage>
    );
  }

  return (
    <AuthPage title="Accept your invite" description="Choose the password of your account.">
      {message === null ? null : <FormError>{message}</FormError>}
      <form aria-label="Accept your invite" className={formColumn} onSubmit={submit}>
        <Field
          ref={password}
          label="Password"
          name="password"
          type="password"
          autoComplete="new-password"
          required
          hint={PASSWORD_POLICY}
          error={passwordError}
        />
        <Field
          ref={confirm}
          label="Confirm password"
          name="confirm"
          type="password"
          autoComplete="new-password"
          required
          error={confirmError}
        />
        <Button type="submit" className={control} disabled={accept.isPending}>
          {accept.isPending ? "Setting the password" : "Set password"}
        </Button>
      </form>
    </AuthPage>
  );
}
