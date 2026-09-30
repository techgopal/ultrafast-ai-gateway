import { useEffect, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { messageOfError } from "@/api/errors";
import { alertDialogFit, dialogButton, useReturnFocus } from "@/components/dialog-fit";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

interface ConfirmDialogProps {
  open: boolean;
  /** Called with `false` when the dialog closes: on cancel, and after `onConfirm` succeeded. */
  onOpenChange: (open: boolean) => void;
  /** The question. It is the accessible name of the dialog. */
  title: string;
  /** What happens when the user confirms. It is the description of the dialog. */
  body: ReactNode;
  confirmLabel: string;
  /** `danger` for what cannot be undone. Default `primary`. */
  tone?: "danger" | "primary";
  /**
   * Does it. While it runs, the buttons are disabled and the dialog stays.
   * When it rejects, the dialog stays open and shows the message of the
   * error, which takes the focus; of an answer of a session that is over it shows nothing, and the
   * clean-up of the session closes the dialog.
   */
  onConfirm: () => Promise<unknown>;
}

/** Asks before something is done that the user may not want. */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  body,
  confirmLabel,
  tone = "primary",
  onConfirm,
}: ConfirmDialogProps) {
  const [running, setRunning] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const busy = useRef(false);
  const alert = useRef<HTMLParagraphElement>(null);
  const returnFocus = useReturnFocus(open);

  // The button that was pressed was disabled while the call ran, and so lost
  // the focus: after a failure it goes to what went wrong.
  useEffect(() => {
    if (message !== null) alert.current?.focus();
  }, [message]);

  function change(next: boolean) {
    if (busy.current) return;
    if (!next) setMessage(null);
    onOpenChange(next);
  }

  async function run() {
    if (busy.current) return;
    busy.current = true;
    setRunning(true);
    setMessage(null);
    let done = false;
    try {
      await onConfirm();
      done = true;
    } catch (error) {
      setMessage(messageOfError(error));
    } finally {
      busy.current = false;
      setRunning(false);
    }
    if (done) onOpenChange(false);
  }

  function confirm(event: MouseEvent) {
    // The dialog closes when the call succeeded, not when the button is pressed.
    event.preventDefault();
    void run();
  }

  return (
    <AlertDialog open={open} onOpenChange={change}>
      <AlertDialogContent className={alertDialogFit} onCloseAutoFocus={returnFocus}>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{body}</AlertDialogDescription>
        </AlertDialogHeader>
        {message === null ? null : (
          <p
            ref={alert}
            role="alert"
            tabIndex={-1}
            className="rounded-sm text-sm text-destructive outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {message}
          </p>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel className={dialogButton} disabled={running}>
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            className={dialogButton}
            variant={tone === "danger" ? "destructive" : "default"}
            disabled={running}
            onClick={confirm}
          >
            {confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
