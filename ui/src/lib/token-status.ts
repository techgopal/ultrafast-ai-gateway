// The API has no status for an access token: it gives when the token was
// revoked and when it expires. The console works the status out as the
// gateway does for a key (`key_status` in `crates/gateway/src/api/keys.rs`),
// and as it decides whether a token still works: a token works while it is
// not revoked and `expires_at > now`, both in UTC and compared as text.

interface TokenTimes {
  expires_at: string | null;
  revoked_at: string | null;
}

/** The time in UTC, in the form the gateway has: `YYYY-MM-DD HH:MM:SS`. */
function utc(time: Date): string {
  return time.toISOString().slice(0, 19).replace("T", " ");
}

/** `revoked`, `expired` or `active`: the first that applies. */
export function tokenStatus(token: TokenTimes, now: Date = new Date()): string {
  if (token.revoked_at !== null) return "revoked";
  if (token.expires_at !== null && token.expires_at <= utc(now)) return "expired";
  return "active";
}
