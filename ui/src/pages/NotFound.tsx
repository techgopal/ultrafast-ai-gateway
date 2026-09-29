import { NotFoundContent } from "@/components/NotFoundContent";

/** The whole screen, for where there is no shell. Inside the shell: `NotFoundContent`. */
export function NotFound() {
  return (
    <main className="flex min-h-svh flex-col items-center justify-center p-6">
      <NotFoundContent />
    </main>
  );
}
