import { useId } from "react";
import { DownloadLink } from "@/components/DownloadLink";

export const POSTGRES_BACKUP = "Use pg_dump to back up a Postgres database.";

/**
 * A copy of the database to keep. The download is the browser's own, so a
 * large file is not held in memory; the session is checked first. A Postgres
 * database is not downloaded from here: the gateway refuses, and says so.
 * `database` is `undefined` while the settings are not known: nothing is
 * offered then.
 */
export function BackupSection({ database }: { database: "sqlite" | "postgres" | undefined }) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-3">
      <h2 id={headingId} className="text-lg font-medium">
        Backup
      </h2>
      {database === undefined ? null : (
        <p className="text-sm text-muted-foreground">
          Database: {database === "postgres" ? "PostgreSQL" : "SQLite"}
        </p>
      )}
      {database === "postgres" ? (
        <p className="max-w-prose text-sm">{POSTGRES_BACKUP}</p>
      ) : (
        <>
          <p className="max-w-prose text-sm">
            A consistent copy of the whole database, of one moment, taken while the gateway runs. It
            holds users, request logs, the audit log and the provider credentials as they are
            stored.
          </p>
          <p className="max-w-prose text-sm text-muted-foreground">
            The backup does not hold the master key, and is useless without it: the credentials in
            it can only be read with the key. Keep the master key safe, apart from the backups.
            Restore is a command-line procedure, in the README: stop the gateway and put the file
            in place.
          </p>
          {database === "sqlite" ? (
            <DownloadLink href="/api/backup">Download backup</DownloadLink>
          ) : null}
        </>
      )}
    </section>
  );
}
