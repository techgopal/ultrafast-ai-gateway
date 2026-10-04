// Dollars as people type them and micros as the gateway has them (millionths
// of a dollar). Pure; whole numbers only, so that no float decides a price.

const DOLLARS = /^(?=\.?\d)(\d*)(?:\.(\d{0,6}))?$/;

/**
 * The micros of an amount in dollars, up to 6 decimals ("2.5" is 2500000; ".5" and "5." are fine),
 * or `null` when the text is no such amount or is beyond what a number holds
 * exactly. No sign, no currency symbol, no exponent.
 */
export function dollarsToMicros(text: string): number | null {
  const found = DOLLARS.exec(text.trim());
  if (found === null) return null;
  const [, whole = "", fraction = ""] = found;
  const micros = Number(whole) * 1_000_000 + Number(fraction.padEnd(6, "0"));
  return Number.isSafeInteger(micros) ? micros : null;
}

/** The micros as dollars for an input: no trailing zeros ("2.5", "0.000001", "2"). */
export function microsToDollars(micros: number): string {
  const whole = Math.floor(micros / 1_000_000);
  const fraction = String(micros % 1_000_000)
    .padStart(6, "0")
    .replace(/0+$/, "");
  return fraction === "" ? String(whole) : `${String(whole)}.${fraction}`;
}

const grouped = new Intl.NumberFormat("en-US");

/**
 * The micros as money: at least cents, and as many more decimals (to 6) as
 * the amount has ("$2.50", "$0.000001"). Nothing is rounded: a price is what
 * it is.
 */
export function formatDollars(micros: number): string {
  const text = microsToDollars(micros);
  const [whole = "0", fraction = ""] = text.split(".");
  return `$${grouped.format(Number(whole))}.${fraction.padEnd(2, "0")}`;
}
