// The parts the sign-in, setup and invite pages share.
import {
  useCallback,
  useEffect,
  useId,
  useState,
  type ComponentProps,
  type ReactNode,
  type Ref,
  type RefObject,
} from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export const PASSWORD_POLICY = "12 characters or more";
export const PASSWORDS_DIFFER = "The passwords do not match.";

interface AuthPageProps {
  title: string;
  description?: string;
  children: ReactNode;
}

/** A page for somebody who is not signed in: one card in the middle. */
export function AuthPage({ title, description, children }: AuthPageProps) {
  return (
    <main className="flex min-h-svh flex-col items-center justify-center bg-background p-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>
            <h1>{title}</h1>
          </CardTitle>
          {description === undefined ? null : <CardDescription>{description}</CardDescription>}
        </CardHeader>
        <CardContent className="flex flex-col gap-4">{children}</CardContent>
      </Card>
    </main>
  );
}

/** The class of a form of these pages: its controls are one column at every width. */
export const formColumn = "flex flex-col gap-4";
/** The class of a control: as wide as the form, and high enough to touch. */
export const control = "min-h-11 w-full";

type InputProps = Omit<
  ComponentProps<typeof Input>,
  "id" | "className" | "aria-describedby" | "aria-invalid"
>;

interface FieldProps extends InputProps {
  label: string;
  name: string;
  /** Shown under the field at all times. */
  hint?: string;
  /** What is wrong with the value. */
  error?: string | undefined;
}

/** A labelled field. Its error and its hint are tied to it with `aria-describedby`. */
export function Field({ label, hint, error, ...input }: FieldProps) {
  const id = useId();
  const errorId = `${id}-error`;
  const hintId = `${id}-hint`;
  const describedBy = [error === undefined ? null : errorId, hint === undefined ? null : hintId]
    .filter((part) => part !== null)
    .join(" ");
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        className={control}
        aria-invalid={error === undefined ? undefined : true}
        aria-describedby={describedBy === "" ? undefined : describedBy}
        {...input}
      />
      {error === undefined ? null : (
        <p id={errorId} role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {hint === undefined ? null : (
        <p id={hintId} className="text-sm text-muted-foreground">
          {hint}
        </p>
      )}
    </div>
  );
}

/**
 * Why the request failed. It is announced when it appears, and it can take
 * the focus (see `useFocusOnFailure`).
 */
export function FormError({ ref, children }: { ref?: Ref<HTMLDivElement>; children: ReactNode }) {
  return (
    <Alert
      ref={ref}
      tabIndex={-1}
      variant="destructive"
      className="outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <AlertDescription>{children}</AlertDescription>
    </Alert>
  );
}

/** What happened before the user came to this page. */
export function Notice({ children }: { children: ReactNode }) {
  return (
    <p role="status" className="rounded-lg border bg-muted px-3 py-2 text-sm text-foreground">
      {children}
    </p>
  );
}

/** The text of a field of a submitted form. */
export function textOf(form: FormData, name: string): string {
  const value = form.get(name);
  return typeof value === "string" ? value : "";
}

/**
 * After a submit that failed, the focus goes to the first field with an error,
 * or to the message of the form when no field has one. Returns what the form
 * calls when a submit failed, after it set its errors.
 */
export function useFocusOnFailure(
  form: RefObject<HTMLFormElement | null>,
  message: RefObject<HTMLDivElement | null>,
): () => void {
  const [failures, setFailures] = useState(0);
  useEffect(() => {
    if (failures === 0) return;
    const field = form.current?.querySelector<HTMLElement>('[aria-invalid="true"]');
    (field ?? message.current)?.focus();
  }, [failures, form, message]);
  return useCallback(() => {
    setFailures((count) => count + 1);
  }, []);
}
