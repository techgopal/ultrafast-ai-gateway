export const SECRET_SHOWN_ONCE = "Copy this signing secret now. It is not shown again.";
export const ROTATE_CONSEQUENCE =
  "The old secret stops working at once. Update the guardrail's service with the new one.";

/** What goes with a signing secret shown once: where it is used. */
export function SecretNote() {
  return (
    <p className="text-sm text-muted-foreground">
      The guardrail&apos;s service checks the <code className="font-mono">x-uf-signature</code>{" "}
      header with it. See How external guardrails work on the guardrails page.
    </p>
  );
}
