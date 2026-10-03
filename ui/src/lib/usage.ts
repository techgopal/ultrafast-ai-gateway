// How usage and logs are shown: money, tokens, rates, days. Pure; the
// numbers are those of the API, none is made up.
import type { components } from "@/api/schema";

export type UsageRow = components["schemas"]["UsageRow"];

const dollars = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" });
const grouped = new Intl.NumberFormat("en-US");

/**
 * Millionths of a dollar as `$1,234.56`. Under a cent it is `<$0.01`; exactly
 * nothing is `$0.00`.
 */
export function formatMoney(micros: number): string {
  if (micros === 0) return "$0.00";
  if (micros < 10_000) return "<$0.01";
  return dollars.format(micros / 1_000_000);
}

export function formatTokens(count: number): string {
  return grouped.format(count);
}

/** Errors as a share of requests, to a tenth of a percent. */
export function errorRate(errors: number, requests: number): string {
  if (requests === 0 || errors === 0) return "0%";
  const percent = (errors / requests) * 100;
  if (percent < 0.1) return "<0.1%";
  return `${Number(percent.toFixed(1))}%`;
}

export function formatDuration(ms: number): string {
  return ms < 1000 ? `${ms} ms` : `${Number((ms / 1000).toFixed(2))} s`;
}

const OUTCOMES: Readonly<Record<string, string>> = {
  ok: "Answered",
  retryable: "Retried",
  fatal: "Failed",
  circuit_open: "Circuit open",
  skipped: "Skipped",
  cached: "Cached",
};

/** What an attempt came to; an outcome the console does not know is shown as it is. */
export function outcomeLabel(outcome: string): string {
  return OUTCOMES[outcome] ?? outcome;
}

const DAY_MS = 86_400_000;

function dayOf(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

/**
 * One row for every day from `from` to `to`, both counted, in order: a day
 * the API has no row for had no calls and is zero.
 */
export function perDay(from: string, to: string, rows: readonly UsageRow[]): UsageRow[] {
  const byDay = new Map(rows.map((row) => [row.group, row]));
  const out: UsageRow[] = [];
  const end = Date.parse(`${to}T00:00:00Z`);
  for (let at = Date.parse(`${from}T00:00:00Z`); at <= end; at += DAY_MS) {
    const day = dayOf(at);
    out.push(
      byDay.get(day) ?? {
        group: day,
        label: day,
        requests: 0,
        errors: 0,
        cancelled: 0,
        input_tokens: 0,
        output_tokens: 0,
        cost_micros: 0,
        unpriced_requests: 0,
      },
    );
  }
  return out;
}

export type Preset = "1h" | "24h" | "7d" | "30d";

const SPAN: Readonly<Record<Preset, number>> = {
  "1h": 3_600_000,
  "24h": DAY_MS,
  "7d": 7 * DAY_MS,
  "30d": 30 * DAY_MS,
};

/** The start of a preset range that ends at `now` (ms): RFC 3339, whole seconds, UTC. */
export function sinceFor(preset: Preset, now: number): string {
  return new Date(now - SPAN[preset]).toISOString().slice(0, 19) + "Z";
}
