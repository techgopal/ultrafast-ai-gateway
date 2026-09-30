// Class names that more than one file uses, each in one place.

/** A control that is high enough to touch on a narrow screen. */
export const control = "min-h-11 md:min-h-8";

/**
 * In the table a cell is one line. A name, an email or an address that is
 * longer than most wraps in its cell, so that it does not make the table much
 * wider than the page; what is shorter stays on its line. On a card the text
 * wraps anyway. The cell says how wide it may get: `md:max-w-64`, `md:max-w-80`.
 */
export const longText = "md:block md:w-max md:whitespace-normal";

/**
 * What a select shows of a long choice is one line, cut at its end: a long
 * email does not make the select, and with it the dialog, wider than the screen.
 */
export const cutLongChoice =
  "*:data-[slot=select-value]:block *:data-[slot=select-value]:min-w-0 *:data-[slot=select-value]:truncate";

/** The list of a select is no wider than the screen; a long choice wraps. */
export const selectList = "max-w-[calc(100vw-2rem)]";
