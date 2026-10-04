// The API gives times as `YYYY-MM-DD HH:MM:SS`, in UTC.
const FORM = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})$/;

const NEVER = "Never";

function dateOf(value: string): Date | null {
  if (!FORM.test(value)) return null;
  const date = new Date(`${value.replace(" ", "T")}Z`);
  return Number.isNaN(date.getTime()) ? null : date;
}

/**
 * The time in the locale and the time zone of the browser; "Never" for no
 * time. A value of another form than the API's is returned as it is.
 */
export function formatTimestamp(value: string | null): string {
  if (value === null) return NEVER;
  const date = dateOf(value);
  if (date === null) return value;
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(
    date,
  );
}

/** A time of the API, with the exact UTC value in its `title`. */
export function Timestamp({ value }: { value: string | null }) {
  if (value === null || dateOf(value) === null) return <span>{formatTimestamp(value)}</span>;
  return (
    <time dateTime={`${value.replace(" ", "T")}Z`} title={`${value} UTC`}>
      {formatTimestamp(value)}
    </time>
  );
}
