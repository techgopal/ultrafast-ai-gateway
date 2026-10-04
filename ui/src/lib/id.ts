/**
 * The id a text stands for, when it is one: a positive integer, written
 * without a sign or a leading zero, that is a safe integer. The text comes
 * from the address or from a field; for anything else the API is not asked.
 */
export function idOf(text: string): number | null {
  if (!/^[1-9]\d{0,15}$/.test(text)) return null;
  const id = Number(text);
  return Number.isSafeInteger(id) ? id : null;
}
