// When a key or an access token stops working. The gateway takes the time in
// UTC, in the form `YYYY-MM-DD HH:MM:SS`; the console lets the user choose a
// day and sends the end of it. Days are those of the calendar of UTC, as the
// time that is sent.
import { ConsoleRefusal } from "@/api/errors";

const DAY = /^(\d{4})-(\d{2})-(\d{2})$/;
const A_DAY = 86_400_000;

/** The day of the time, by the calendar of UTC, as `YYYY-MM-DD`. */
export function today(now: Date = new Date()): string {
  return now.toISOString().slice(0, 10);
}

/** The day that is `days` days after today, by the calendar of UTC. */
export function dayIn(days: number, now: Date = new Date()): string {
  return today(new Date(now.getTime() + days * A_DAY));
}

/**
 * The end of the day, as the gateway takes a time: `YYYY-MM-DD 23:59:59`.
 * The day is sent as it is written: it is not moved into another time zone.
 * `null` for a text that is no day of the calendar.
 */
export function endOfDay(day: string): string | null {
  if (!DAY.test(day)) return null;
  const date = new Date(`${day}T00:00:00Z`);
  // A day the calendar does not have, such as 30 February, is another day or none.
  if (Number.isNaN(date.getTime()) || today(date) !== day) return null;
  return `${day} 23:59:59`;
}

export const CHOOSE_A_DATE = "Choose a date.";

/** When it expires, as a form offers it: the value of each choice, and what it says. */
export const EXPIRY_CHOICES = [
  ["never", "Never"],
  ["30", "In 30 days"],
  ["90", "In 90 days"],
  ["date", "On a date"],
] as const;

const DAYS: Record<string, number> = { "30": 30, "90": 90 };

/**
 * When it expires, as a form holds it. It is one value of the form, under the
 * name the gateway has for it: what is said about `expires_at`, by the
 * gateway or by the console, is said about the choice and the day together,
 * and goes when either of them is changed.
 */
export interface Expiry {
  /** One of `EXPIRY_CHOICES`. */
  choice: string;
  /** The day, when it expires on a date. */
  day: string;
}

/** What a new form holds: it never expires. */
export const NO_EXPIRY: Expiry = { choice: "never", day: "" };

/**
 * When it stops working, as the gateway takes it; nothing for never. "On a
 * date" without a day of the calendar is refused by the console itself, for
 * the field `expires_at`.
 */
export function expiryOf({ choice, day }: Expiry, now: Date = new Date()): string | undefined {
  if (choice === "date") {
    const end = endOfDay(day);
    if (end === null) throw new ConsoleRefusal(CHOOSE_A_DATE, "expires_at");
    return end;
  }
  const days = DAYS[choice];
  return days === undefined ? undefined : (endOfDay(dayIn(days, now)) ?? undefined);
}
