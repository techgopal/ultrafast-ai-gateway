import type { ReactNode } from "react";
import { dialogButton, dialogFit, useReturnFocus } from "@/components/dialog-fit";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface FormDialogProps {
  open: boolean;
  /** The request of the form runs: `isPending` of its mutation. For that time the dialog stays. */
  running: boolean;
  /**
   * The user leaves the dialog and its form was not sent: by Escape, by a
   * click beside the dialog, by its X. The opener closes the dialog and
   * resets the mutation. It is also what the Cancel of the form calls.
   */
  onCancel: () => void;
  /** What the dialog does. It is its accessible name. */
  title: string;
  /** It is the description of the dialog. */
  description: ReactNode;
  /**
   * The form. It is mounted while the dialog is open, so every opening starts
   * with a new form. Its `onSubmit` is `submitOnce(form)` of `components/form`,
   * and its last child is a `FormDialogFooter`.
   */
  children: ReactNode;
}

/**
 * The dialog of every form of the console.
 *
 * While the request of its form runs the dialog stays, as a dialog that asks
 * does while its call runs: Escape and a click beside it do nothing, and it
 * has no X. A dialog that was closed then could be opened again and send the
 * same once more, and the answer of the first request would close it over
 * what was typed. After a request that failed it can be left again.
 *
 * When it closes, the focus goes back to what opened it.
 *
 * A secret that is shown once has a dialog of its own, `SecretDialog`.
 */
export function FormDialog({
  open,
  running,
  onCancel,
  title,
  description,
  children,
}: FormDialogProps) {
  const returnFocus = useReturnFocus(open);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !running) onCancel();
      }}
    >
      <DialogContent
        className={dialogFit}
        showCloseButton={!running}
        onCloseAutoFocus={returnFocus}
      >
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        {children}
      </DialogContent>
    </Dialog>
  );
}

interface FormDialogFooterProps {
  /** The request of the form runs: neither button can be pressed. */
  running: boolean;
  /** What the submit button says. */
  submit: string;
  /** What it says while the request runs. */
  submitting: string;
  /** There is nothing to send yet: the submit button cannot be pressed. */
  disabled?: boolean;
  /** The `onCancel` of the dialog. */
  onCancel: () => void;
}

/** The two buttons of the form of a `FormDialog`: Cancel, and the one that sends. */
export function FormDialogFooter({
  running,
  submit,
  submitting,
  disabled = false,
  onCancel,
}: FormDialogFooterProps) {
  return (
    <DialogFooter>
      <Button
        type="button"
        variant="outline"
        className={dialogButton}
        disabled={running}
        onClick={onCancel}
      >
        Cancel
      </Button>
      <Button type="submit" className={dialogButton} disabled={running || disabled}>
        {running ? submitting : submit}
      </Button>
    </DialogFooter>
  );
}
