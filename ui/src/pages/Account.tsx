import { useSession } from "@/auth/session";
import { PageHeader } from "@/components/PageHeader";
import { Part } from "@/components/Part";
import { PasswordForm } from "@/pages/AccountPassword";
import { Profile } from "@/pages/AccountProfile";
import { Tokens } from "@/pages/AccountTokens";

/** The account of who is signed in: their profile, their password, their access tokens. */
export function Account() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  return (
    <>
      <PageHeader title="Account" />
      <Profile me={session.me} />
      {session.me.user.has_password ? (
        <Part
          title="Password"
          description="Changing it ends your other sessions and revokes all your access tokens."
        >
          <PasswordForm email={session.me.user.email} />
        </Part>
      ) : (
        <Part title="Password">
          <p className="text-sm text-muted-foreground">
            You sign in with single sign-on. Ask an admin for a password link if you want a
            password too.
          </p>
        </Part>
      )}
      <Tokens />
    </>
  );
}
