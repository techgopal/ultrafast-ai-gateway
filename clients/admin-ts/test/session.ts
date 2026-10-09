// Signs in once with a password (session cookie and CSRF token) to mint an
// access token. Tests then use only the token, through the SDK.
import { randomBytes } from "node:crypto";
import type { Account, Gateway } from "./gateway.js";

export async function mintToken(gateway: Gateway, account: Account, name = "sdk-test"): Promise<string> {
  const login = await fetch(`${gateway.origin}/api/auth/login`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(account),
  });
  if (!login.ok) throw new Error(`sign-in failed with ${login.status}`);
  const cookie = login.headers
    .getSetCookie()
    .map((line) => line.split(";")[0] ?? "")
    .join("; ");
  const { csrf_token: csrf } = (await login.json()) as { csrf_token: string };
  const created = await fetch(`${gateway.origin}/api/tokens`, {
    method: "POST",
    headers: { "content-type": "application/json", cookie, "x-csrf-token": csrf },
    body: JSON.stringify({ name }),
  });
  if (created.status !== 201) throw new Error(`minting a token failed with ${created.status}`);
  const { secret } = (await created.json()) as { secret: string };
  return secret;
}

/** Invites a member through the SDK's token, accepts the invite, and returns a token of that member. */
export async function memberToken(
  gateway: Gateway,
  adminToken: string,
  email: string,
): Promise<string> {
  const invited = await fetch(`${gateway.origin}/api/users`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${adminToken}` },
    body: JSON.stringify({ email, name: "Member", role: "member" }),
  });
  if (invited.status !== 201) throw new Error(`inviting failed with ${invited.status}`);
  const { invite_link: link } = (await invited.json()) as { invite_link: string };
  // The link is `<origin>/accept-invite#token=<token>`.
  const token = link.split("#token=")[1] ?? "";
  const password = `pw-${randomBytes(12).toString("hex")}`;
  const accepted = await fetch(`${gateway.origin}/api/auth/accept-invite`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ token, password }),
  });
  if (accepted.status !== 204) throw new Error(`accepting the invite failed with ${accepted.status}`);
  return mintToken(gateway, { email, password }, "member-test");
}
