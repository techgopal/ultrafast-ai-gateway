import { useCallback } from "react";
import { toast } from "sonner";

export type ToastTone = "ok" | "error";

/** How long a toast stays, in milliseconds. */
export const TOAST_DURATION = 5000;

/**
 * Says that something was done. The toast is announced to screen readers,
 * goes after 5 seconds and can be dismissed.
 *
 * Toasts are for successes. What a mutation failed with is shown by the form
 * or the dialog that made the call. A message never holds a secret.
 */
export function useToast(): (message: string, tone?: ToastTone) => void {
  return useCallback((message, tone = "ok") => {
    const show = tone === "error" ? toast.error : toast.success;
    show(message, { duration: TOAST_DURATION, closeButton: true, dismissible: true });
  }, []);
}

/**
 * Takes every toast away, at once. The toasts are kept by the module of
 * sonner, not by a component, and a `Toaster` that mounts shows every toast
 * that is still active: without this a toast of one session would show in
 * the next. The clean-up of the session calls it wherever a session ends or
 * is replaced.
 */
export function dismissAll(): void {
  toast.dismiss();
}
