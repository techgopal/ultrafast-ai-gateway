import { Link } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";

export function NotFound() {
  return (
    <main className="flex min-h-svh flex-col items-center justify-center gap-4 p-6 text-center">
      <h1 className="text-2xl font-semibold">Page not found</h1>
      <p className="text-muted-foreground">
        There is nothing at this address.
      </p>
      <Button asChild className="min-h-11 md:min-h-8">
        <Link to="/">Back to Overview</Link>
      </Button>
    </main>
  );
}
