/**
 * For a page that the account may not see. It is not an error. It has no
 * `main` of its own: inside the shell it is in the shell's.
 */
export function NotAvailableContent() {
  return (
    <div className="flex flex-col gap-2">
      <h1 className="text-2xl font-semibold">Not available</h1>
      <p className="text-muted-foreground">This page is not available to your account.</p>
    </div>
  );
}
