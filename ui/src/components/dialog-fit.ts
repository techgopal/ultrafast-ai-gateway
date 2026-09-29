// What the dialogs of the console share: how they fit a narrow screen, and
// where the focus goes when they close.
import { useCallback, useLayoutEffect, useRef } from "react";

/**
 * Below 768 px a dialog is as wide as the screen less its margin. At every
 * width it is no higher than the screen, and what does not fit scrolls inside.
 */
export const dialogFit =
  "max-h-[calc(100svh-2rem)] overflow-y-auto max-w-[calc(100%-2rem)] sm:max-w-[calc(100%-2rem)] md:max-w-sm";

/** The same for the alert dialog, whose widths depend on its size. */
export const alertDialogFit =
  "max-h-[calc(100svh-2rem)] overflow-y-auto data-[size=default]:max-w-[calc(100%-2rem)] data-[size=default]:sm:max-w-[calc(100%-2rem)] data-[size=default]:md:max-w-sm";

/** A button of a dialog: high enough to touch on a narrow screen. */
export const dialogButton = "min-h-11 md:min-h-8";

/**
 * A dialog that is opened by its `open` prop has no trigger to give the focus
 * back to. This remembers what had the focus when the dialog opened; the
 * returned handler, for `onCloseAutoFocus`, gives the focus back to it.
 */
export function useReturnFocus(open: boolean): (event: Event) => void {
  const opener = useRef<HTMLElement | null>(null);
  // Before the dialog takes the focus, which it does in an effect.
  useLayoutEffect(() => {
    if (!open) return;
    const active = document.activeElement;
    opener.current = active instanceof HTMLElement && active !== document.body ? active : null;
  }, [open]);
  return useCallback((event: Event) => {
    const target = opener.current;
    opener.current = null;
    if (target === null || !target.isConnected) return;
    event.preventDefault();
    target.focus();
  }, []);
}
