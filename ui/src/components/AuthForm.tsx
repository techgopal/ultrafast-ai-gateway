// The parts the sign-in, setup and invite pages share. The error of the form
// and the focus after a failed submit are the ones every form of the console has.
import { Brand } from "@/components/Brand";
import type { ComponentProps, ReactNode } from "react";
import { Field as SharedField } from "@/components/Field";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";

export { useFocusOnFailure } from "@/components/form";
export { FormError } from "@/components/FormError";

export const PASSWORD_POLICY = "12 characters or more";
export const PASSWORDS_DIFFER = "The passwords do not match.";
/** For a 429 of the gateway, wherever a password is asked for. */
export const TOO_MANY_ATTEMPTS = "Too many attempts. Try again in a few minutes.";

interface AuthPageProps {
  title: string;
  description?: string;
  children: ReactNode;
}

/** A page for somebody who is not signed in: one card in the middle. */
export function AuthPage({ title, description, children }: AuthPageProps) {
  return (
    <main className="flex min-h-svh flex-col items-center justify-center gap-6 bg-background p-4">
      <Brand className="text-lg" />
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
  "id" | "name" | "className" | "aria-describedby" | "aria-invalid"
>;

interface FieldProps extends InputProps {
  label: string;
  name: string;
  /** Shown under the field at all times. */
  hint?: string;
  /** What is wrong with the value. */
  error?: string | undefined;
}

/** A labelled text field of these pages: the shared field around an input. */
export function Field({ label, name, hint, error, required, ...input }: FieldProps) {
  return (
    <SharedField label={label} name={name} hint={hint} error={error} required={required}>
      <Input className={control} {...input} />
    </SharedField>
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
