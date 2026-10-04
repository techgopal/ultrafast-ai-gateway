import { useId } from "react";
import { control } from "@/components/classes";
import { Button } from "@/components/ui/button";

/** A copy of the database to keep. The download is the browser's own, so a large file is not held in memory. */
export function BackupSection() {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3">
      <h2 id={headingId} className="text-lg font-medium">
        Backup
      </h2>
      <p className="max-w-prose text-sm">
        A consistent copy of the whole database, of one moment, taken while the gateway runs. It
        holds users, request logs, the audit log and the provider credentials as they are stored.
      </p>
      <p className="max-w-prose text-sm text-muted-foreground">
        The backup does not hold the master key, and is useless without it: the credentials in it
        can only be read with the key. Keep the master key safe, apart from the backups. Restore is
        a command-line procedure, in the README: stop the gateway and put the file in place.
      </p>
      <div>
        <Button asChild className={control}>
          <a href="/api/backup" download>
            Download backup
          </a>
        </Button>
      </div>
    </section>
  );
}
