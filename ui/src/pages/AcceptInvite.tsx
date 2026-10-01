import { Link, useNavigate } from "@tanstack/react-router";
import { useRef, useState, type SyntheticEvent } from "react";
import { ApiError, messageOfError } from "@/api/errors";
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
  useFocusOnFailure,
} from "@/components/AuthForm";
import { Button } from "@/components/ui/button";

export const INVITE_ACCEPTED_NOTICE = "Your password is set. Sign in to continue.";
const INVALID_INVITE = "This invite link is not valid or has expired. Ask an admin for a new one.";

/**
 * Sets the password of an invited user. The token of the link is held by the
 * session for as long as this goes on, and the address never shows it here.
 * Without it the link has to be opened again.
 */
export function AcceptInvite() {
  const navigate = useNavigate();
  const session = useSession();
  const { announce, invite: token, dropInvite } = useSessionControl();
  const [leaving, setLeaving] = useState(false);
  const signOut = useSignOut();
  const accept = useAcceptInvite();
  const { mutateAsync, reset } = accept;
  const password = useRef<HTMLInputElement>(null);
  const confirm = useRef<HTMLInputElement>(null);
  const running = useRef(false);
  const form = useRef<HTMLFormElement>(null);
  const error = useRef<HTMLDivElement>(null);
  const failed = useFocusOnFailure(form, error);
  const [message, setMessage] = useState<string | null>(null);
  const [passwordError, setPasswordError] = useState<string | undefined>(undefined);
  const [confirmError, setConfirmError] = useState<string | undefined>(undefined);

  async function setPassword(data: FormData) {
    if (running.current || token === null) return;
    setMessage(null);
    setPasswordError(undefined);
    if (textOf(data, "password") !== textOf(data, "confirm")) {
      setConfirmError(PASSWORDS_DIFFER);
      failed();
      return;
    }
    setConfirmError(undefined);
    running.current = true;
    let accepted = false;
    try {
      await mutateAsync({ token, password: textOf(data, "password") });
      accepted = true;
    } catch (reason) {
      const error = reason;
      failed();
      if (error instanceof ApiError && error.status === 404) {
        // The token is of no use any more.
        dropInvite();
        setMessage(INVALID_INVITE);
      } else if (error instanceof ApiError && error.fields.password !== undefined) {
        setPasswordError(error.fields.password);
      } else {
        setMessage(messageOfError(error));
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
      dropInvite();
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
        {message === null ? null : <FormError ref={error}>{message}</FormError>}
        <p className="text-sm text-muted-foreground">
          Open your invite link again. This page cannot be reloaded.
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
            disabled={leaving}
            onClick={() => {
              setLeaving(true);
              // Signing out mounts this page anew; the session holds the token.
              void signOut();
            }}
          >
            {leaving ? "Signing out" : "Sign out and continue"}
          </Button>
          {leaving ? (
            <Button type="button" variant="outline" className={control} disabled>
              Stay signed in
            </Button>
          ) : (
            <Button asChild variant="outline" className={control}>
              <Link to="/" onClick={dropInvite}>
                Stay signed in
              </Link>
            </Button>
          )}
        </div>
      </AuthPage>
    );
  }

  return (
    <AuthPage title="Accept your invite" description="Choose the password of your account.">
      {message === null ? null : <FormError ref={error}>{message}</FormError>}
      <form ref={form} aria-label="Accept your invite" className={formColumn} onSubmit={submit}>
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
