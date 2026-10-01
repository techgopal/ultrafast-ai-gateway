import { Link } from "@tanstack/react-router";
import { control } from "@/components/classes";
import { Button } from "@/components/ui/button";

/**
 * Says that there is nothing at this address. It has no `main` of its own:
 * inside the shell it is in the shell's, and `pages/NotFound` gives it one
 * where there is no shell.
 */
export function NotFoundContent() {
  return (
    <div className="flex flex-col items-center gap-4 py-8 text-center">
      <h1 className="text-2xl font-semibold">Page not found</h1>
      <p className="text-muted-foreground">There is nothing at this address.</p>
      <Button asChild className={control}>
        <Link to="/">Back to Overview</Link>
      </Button>
    </div>
  );
}
