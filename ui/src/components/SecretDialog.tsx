import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { alertDialogFit, dialogButton, dialogFit, useReturnFocus } from "@/components/dialog-fit";
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
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

export interface SecretOnce {
  /** The secret to show, or `null`. It is the `secret` of the `SecretDialog`. */
  secret: string | null;
  /** Shows the secret, and makes the mutation forget its answer. */
  show: (secret: string) => void;
  /** Forgets the secret. It is the `onClose` of the `SecretDialog`. */
  clear: () => void;
}

/**
 * Holds a secret that is shown once, in the state of the component that
 * calls it and nowhere else.
 *
 * `source` is the mutation whose answer holds the secret; it is required, so
 * that no page can show a secret and leave it in the mutation. A mutation keeps
 * its variables and its answer while a component observes it, so `show`
 * resets it at once: from then on the secret is in this state only. The
 * secret is forgotten by `clear`, and when the component unmounts.
 *
 * ```tsx
 * const create = useCreateKey();
 * const once = useSecretOnce(create);
 * const made = await create.mutateAsync(body);
 * once.show(made.secret);
 * <SecretDialog title="…" description="…" secret={once.secret} onClose={once.clear} />
 * ```
 */
export function useSecretOnce(source: { reset: () => void }): SecretOnce {
  const [secret, setSecret] = useState<string | null>(null);
  const { reset } = source;
  const show = useCallback(
    (value: string) => {
      setSecret(value);
      reset();
    },
    [reset],
  );
  const clear = useCallback(() => {
    setSecret(null);
  }, []);
  useEffect(() => clear, [clear]);
  return useMemo(() => ({ secret, show, clear }), [secret, show, clear]);
}

const COPIED = "Copied";
const COPY_BY_HAND = "Press Ctrl+C to copy";

interface SecretDialogProps {
  /** What the secret is, such as "Your new key". It names the dialog and the field. */
  title: string;
  /** For example "Copy this key now. It is not shown again." */
  description: string;
  /** The dialog is open while this is a secret. */
  secret: string | null;
  /** The user closed the dialog: the opener forgets the secret. */
  onClose: () => void;
  /**
   * What the opener says about the use of the secret, shown under it: an
   * example with a placeholder. It must not hold the secret.
   */
  children?: ReactNode;
}

/**
 * Shows a secret once: a new virtual key, a new access token, an invite link.
 *
 * The dialog never reads the answer of a mutation. The component that opens
 * it must
 * 1. copy the secret from the answer of the mutation into its own state,
 * 2. call `reset()` of the mutation at once, which otherwise keeps the answer,
 * 3. set the state to `null` in `onClose`.
 * `useSecretOnce` does the three. The secret is never given to a toast, a
 * log, the query cache, the router or the storage of the browser.
 *
 * Closing (the buttons, Escape, a click beside the dialog) asks first, with
 * Keep open as the default.
 */
export function SecretDialog({
  title,
  description,
  secret,
  onClose,
  children,
}: SecretDialogProps) {
  const open = secret !== null;
  const [asking, setAsking] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  const returnFocus = useReturnFocus(open);

  function close() {
    setAsking(false);
    setCopied(null);
    onClose();
  }

  function selectIt() {
    field.current?.focus();
    field.current?.select();
    setCopied(COPY_BY_HAND);
  }

  async function copy() {
    if (secret === null) return;
    // Not every browser has it, and not every page may use it.
    const clipboard: unknown = Reflect.get(navigator, "clipboard");
    if (!(clipboard instanceof Object) || !("writeText" in clipboard)) {
      selectIt();
      return;
    }
    try {
      await navigator.clipboard.writeText(secret);
      setCopied(COPIED);
    } catch {
      selectIt();
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setAsking(true);
      }}
    >
      <DialogContent className={dialogFit} onCloseAutoFocus={returnFocus}>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-2 sm:flex-row">
          <Input
            ref={field}
            readOnly
            aria-label={title}
            value={secret ?? ""}
            className="min-h-11 font-mono md:min-h-8"
            onFocus={(event) => {
              event.currentTarget.select();
            }}
          />
          <Button
            type="button"
            variant="outline"
            className={dialogButton}
            onClick={() => {
              void copy();
            }}
          >
            Copy
          </Button>
        </div>
        <p role="status" className="min-h-5 text-sm text-muted-foreground">
          {copied}
        </p>
        {children}
        <DialogFooter>
          <Button
            type="button"
            className={dialogButton}
            onClick={() => {
              setAsking(true);
            }}
          >
            Done
          </Button>
        </DialogFooter>
        <AlertDialog
          open={open && asking}
          onOpenChange={(next) => {
            if (!next) setAsking(false);
          }}
        >
          <AlertDialogContent className={alertDialogFit}>
            <AlertDialogHeader>
              <AlertDialogTitle>Close this dialog?</AlertDialogTitle>
              <AlertDialogDescription>
                Have you copied it? It cannot be shown again.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel className={dialogButton}>Keep open</AlertDialogCancel>
              <AlertDialogAction className={dialogButton} variant="destructive" onClick={close}>
                Close
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </DialogContent>
    </Dialog>
  );
}
