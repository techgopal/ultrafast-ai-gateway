import { describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import { tokenStatus } from "./token-status";

const NOW = new Date("2026-09-30T12:00:00Z");

function token(expires_at: string | null, revoked_at: string | null = null) {
  return { expires_at, revoked_at };
}

describe("the status of an access token", () => {
  test("the tokens of the fixtures", () => {
    const { active, neverUsed, revoked, expired } = fixtures.tokens;
    expect([active, neverUsed, revoked, expired].map((one) => tokenStatus(one, NOW))).toEqual([
      "active",
      "active",
      "revoked",
      "expired",
    ]);
  });

  test("without an expiry it is active", () => {
    expect(tokenStatus(token(null), NOW)).toBe("active");
  });

  test("revoked comes first, as for a key", () => {
    expect(tokenStatus(token(null, "2026-09-05 09:00:00"), NOW)).toBe("revoked");
    // Revoked and past its time: revoked.
    expect(tokenStatus(token("2026-09-01 00:00:00", "2026-09-05 09:00:00"), NOW)).toBe("revoked");
    // Revoked before its time came: revoked.
    expect(tokenStatus(token("2027-01-01 00:00:00", "2026-09-05 09:00:00"), NOW)).toBe("revoked");
  });

  test("it is expired from the second of its expiry on, in UTC", () => {
    expect(tokenStatus(token("2026-09-30 12:00:01"), NOW)).toBe("active");
    // The gateway takes a token while `expires_at > now`.
    expect(tokenStatus(token("2026-09-30 12:00:00"), NOW)).toBe("expired");
    expect(tokenStatus(token("2026-09-30 11:59:59"), NOW)).toBe("expired");
  });

  test("the time is that of UTC, whatever the time zone of the browser", () => {
    // 23:30 UTC is the next day east of Greenwich: the token of that UTC day is still active.
    const late = new Date("2026-10-01T23:30:00Z");
    expect(tokenStatus(token("2026-10-01 23:59:59"), late)).toBe("active");
    expect(tokenStatus(token("2026-10-01 23:29:59"), late)).toBe("expired");
  });

  test("without a time given, it is now", () => {
    expect(tokenStatus(token("2000-01-01 00:00:00"))).toBe("expired");
    expect(tokenStatus(token("2999-01-01 00:00:00"))).toBe("active");
  });
});
