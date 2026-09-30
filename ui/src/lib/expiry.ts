// When a key stops working. The gateway takes the time in UTC, in the form
// `YYYY-MM-DD HH:MM:SS`; the console lets the user choose a day and sends the
// end of it. Days are those of the calendar of UTC, as the time that is sent.

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
