import { describe, expect, test } from "vitest";
import { ConsoleRefusal } from "@/api/errors";
import { CHOOSE_A_DATE, dayIn, endOfDay, expiryOf, EXPIRY_CHOICES, NO_EXPIRY, today } from "./expiry";

describe("the end of a day", () => {
  test("is 23:59:59 of the day as it is written, in the form the gateway takes", () => {
    expect(endOfDay("2027-01-31")).toBe("2027-01-31 23:59:59");
    expect(endOfDay("2028-02-29")).toBe("2028-02-29 23:59:59");
    // crates/gateway/src/api/mod.rs, future_timestamp: a UTC time in the form YYYY-MM-DD HH:MM:SS.
    expect(endOfDay("2027-01-31")).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/);
  });

  test.each([
    "",
    "2027-1-31",
    "31-01-2027",
    "2027-02-30",
    "2027-02-29",
    "2027-13-01",
    "2027-00-10",
    "12027-01-01",
    "2027-01-31T00:00",
    " 2027-01-31",
    "2027-01-31 ",
  ])("%j is no day", (text) => {
    expect(endOfDay(text)).toBeNull();
  });
});

describe("today and the days after it", () => {
  // Late in the UTC day it is the next day east of Greenwich, early in it the
  // day before west of it. The day is the one of UTC, wherever the viewer is.
  test.each(["2026-10-01T00:00:00Z", "2026-10-01T12:00:00Z", "2026-10-01T23:59:59Z"])(
    "are those of the calendar of UTC: at %s",
    (at) => {
      const now = new Date(at);
      expect(today(now)).toBe("2026-10-01");
      expect(dayIn(0, now)).toBe("2026-10-01");
      expect(dayIn(30, now)).toBe("2026-10-31");
      expect(dayIn(90, now)).toBe("2026-12-30");
    },
  );

  test("go over the end of a month, a year and a leap day", () => {
    expect(dayIn(30, new Date("2026-12-15T12:00:00Z"))).toBe("2027-01-14");
    expect(dayIn(30, new Date("2028-02-01T00:00:00Z"))).toBe("2028-03-02");
    expect(dayIn(30, new Date("2027-02-01T00:00:00Z"))).toBe("2027-03-03");
  });

  test("the key that expires in 30 days works for at least 30 days, and less than 31", () => {
    for (const at of ["2026-10-01T00:00:00Z", "2026-10-01T23:59:58Z"]) {
      const now = new Date(at);
      const end = endOfDay(dayIn(30, now));
      if (end === null) throw new Error("no end");
      const days = (Date.parse(`${end.replace(" ", "T")}Z`) - now.getTime()) / 86_400_000;
      expect(days).toBeGreaterThanOrEqual(30);
      expect(days).toBeLessThan(31);
    }
  });

  test("without a time given, the time is now", () => {
    expect(today()).toBe(new Date().toISOString().slice(0, 10));
  });
});

describe("the expiry a form holds, as the gateway takes it", () => {
  /** Late in the UTC day. */
  const now = new Date("2026-10-01T23:30:00Z");

  test("the choices, in the order they are offered; a new form has none", () => {
    expect(EXPIRY_CHOICES.map(([value, label]) => [value, label])).toEqual([
      ["never", "Never"],
      ["30", "In 30 days"],
      ["90", "In 90 days"],
      ["date", "On a date"],
    ]);
    expect(NO_EXPIRY).toEqual({ choice: "never", day: "" });
  });

  test("never is nothing to send", () => {
    expect(expiryOf(NO_EXPIRY, now)).toBeUndefined();
    // A day that was typed before another choice was made is not sent.
    expect(expiryOf({ choice: "never", day: "2027-01-31" }, now)).toBeUndefined();
  });

  test("30 and 90 days are counted from today in UTC, to the end of that day", () => {
    expect(expiryOf({ choice: "30", day: "" }, now)).toBe("2026-10-31 23:59:59");
    expect(expiryOf({ choice: "90", day: "" }, now)).toBe("2026-12-30 23:59:59");
  });

  test("a date is the end of that day", () => {
    expect(expiryOf({ choice: "date", day: "2027-01-31" }, now)).toBe("2027-01-31 23:59:59");
  });

  test.each(["", "2027-02-30", "soon"])(
    "on a date without a day (%j) the console refuses, for the field of the expiry",
    (day) => {
      let refusal: unknown;
      try {
        expiryOf({ choice: "date", day }, now);
      } catch (error) {
        refusal = error;
      }
      expect(refusal).toBeInstanceOf(ConsoleRefusal);
      expect(refusal).toMatchObject({ message: "Choose a date.", field: "expires_at" });
      expect(CHOOSE_A_DATE).toBe("Choose a date.");
    },
  );

  test("a choice the form does not offer is no expiry", () => {
    expect(expiryOf({ choice: "365", day: "" }, now)).toBeUndefined();
  });
});
