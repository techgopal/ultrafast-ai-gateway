import { describe, expect, test } from "vitest";
import {
  errorRate,
  formatDuration,
  formatMoney,
  formatTokens,
  outcomeLabel,
  perDay,
  sinceFor,
} from "@/lib/usage";

describe("money", () => {
  test("dollars with cents and thousands", () => {
    expect(formatMoney(1_234_560_000)).toBe("$1,234.56");
    expect(formatMoney(10_000)).toBe("$0.01");
    expect(formatMoney(12_340_000)).toBe("$12.34");
  });
  test("under a cent is said so, and exactly nothing is $0.00", () => {
    expect(formatMoney(0)).toBe("$0.00");
    expect(formatMoney(1)).toBe("<$0.01");
    expect(formatMoney(9_999)).toBe("<$0.01");
  });
});

describe("numbers", () => {
  test("tokens are grouped", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(1234567)).toBe("1,234,567");
  });
  test("the error rate", () => {
    expect(errorRate(0, 0)).toBe("0%");
    expect(errorRate(1, 8)).toBe("12.5%");
    expect(errorRate(1, 3)).toBe("33.3%");
    expect(errorRate(3, 3)).toBe("100%");
    expect(errorRate(1, 100000)).toBe("<0.1%");
  });
  test("durations", () => {
    expect(formatDuration(850)).toBe("850 ms");
    expect(formatDuration(1250)).toBe("1.25 s");
  });
});

describe("days", () => {
  const row = (group: string, requests: number) => ({
    group,
    label: group,
    requests,
    errors: 0,
    cancelled: 0,
    input_tokens: 0,
    output_tokens: 0,
    cost_micros: 0,
    unpriced_requests: 0,
  });
  test("a day with no calls is zero, in order, both ends counted", () => {
    const days = perDay("2026-02-27", "2026-03-02", [row("2026-03-01", 4), row("2026-02-27", 2)]);
    expect(days.map((d) => [d.group, d.requests])).toEqual([
      ["2026-02-27", 2],
      ["2026-02-28", 0],
      ["2026-03-01", 4],
      ["2026-03-02", 0],
    ]);
  });
});

describe("outcomes", () => {
  test("labels", () => {
    expect(
      ["ok", "retryable", "fatal", "circuit_open", "skipped", "cached", "new"].map(outcomeLabel),
    ).toEqual(["Answered", "Retried", "Failed", "Circuit open", "Skipped", "Cached", "new"]);
  });
});

describe("ranges", () => {
  test("a preset starts that long before the moment, in whole seconds, UTC", () => {
    const at = Date.UTC(2026, 8, 30, 12, 0, 0, 999);
    expect(sinceFor("1h", at)).toBe("2026-09-30T11:00:00Z");
    expect(sinceFor("24h", at)).toBe("2026-09-29T12:00:00Z");
    expect(sinceFor("7d", at)).toBe("2026-09-23T12:00:00Z");
    expect(sinceFor("30d", at)).toBe("2026-08-31T12:00:00Z");
  });
});
