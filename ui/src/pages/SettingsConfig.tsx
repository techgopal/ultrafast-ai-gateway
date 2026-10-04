import { useId, useRef, useState } from "react";
import { useImportConfig } from "@/api/queries";
import type { ImportReport } from "@/api/client";
import { ConsoleRefusal, messageOfError } from "@/api/errors";
import { control } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DownloadLink } from "@/components/DownloadLink";
import { DataTable, type Column } from "@/components/DataTable";
import { useToast } from "@/components/toast";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

/** The largest file the gateway reads. */
const MAX_BYTES = 8 * 1024 * 1024;

export const NOT_JSON = "This file is not JSON.";
export const TOO_LARGE = "This file is larger than 8 MiB.";
export const IMPORTED = "Configuration imported.";

type Change = { action: "Create" | "Update"; kind: string; name: string; changes: string[] };

const changeColumns: Column<Change>[] = [
  { id: "action", header: "Action", cell: (row) => row.action },
  { id: "kind", header: "Kind", cell: (row) => row.kind },
  { id: "name", header: "Name", cell: (row) => <span className="break-all">{row.name}</span> },
  { id: "changes", header: "Changes", cell: (row) => row.changes.join(", ") },
];

const problemColumns: Column<{ at: string; message: string }>[] = [
  { id: "at", header: "Where", cell: (row) => <span className="font-mono break-all">{row.at}</span> },
  { id: "message", header: "Problem", cell: (row) => <span className="break-words">{row.message}</span> },
];

function count(number: number, one: string, many: string): string {
  return `${String(number)} ${number === 1 ? one : many}`;
}

/** What the gateway said of the file. */
function Report({ report }: { report: ImportReport }) {
  const changes: Change[] = [
    ...report.created.map((item) => ({ action: "Create" as const, ...item })),
    ...report.updated.map((item) => ({ action: "Update" as const, ...item })),
  ];
  const warnings =
    report.warnings.length === 0 ? null : (
      <div className="flex flex-col gap-1">
        <h3 className="text-sm font-medium">Warnings</h3>
        <ul className="flex flex-col gap-1 text-sm">
          {report.warnings.map((warning, index) => (
            <li key={index} className="break-words">{`${warning.at}: ${warning.message}`}</li>
          ))}
        </ul>
      </div>
    );
  if (report.errors.length > 0) {
    return (
      <div className="flex flex-col gap-3">
        <p className="text-sm font-medium">The file has errors. Nothing was written.</p>
        <DataTable
          caption="Problems in the file"
          columns={problemColumns}
          rows={report.errors}
          getRowId={(row) => `${row.at} ${row.message}`}
          empty={<p>No problems.</p>}
        />
        {warnings}
      </div>
    );
  }
  if (changes.length === 0) {
    return (
      <div className="flex flex-col gap-3">
        <p className="text-sm">
          {report.unchanged === 1
            ? "Nothing to change: the 1 thing in the file is already so."
            : `Nothing to change: all ${String(report.unchanged)} things are already so.`}
        </p>
        {warnings}
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm">
        Nothing has been written yet. This is what applying the file would do.
      </p>
      <DataTable
        caption="What the import would do"
        columns={changeColumns}
        rows={changes}
        getRowId={(row) => `${row.action} ${row.kind} ${row.name}`}
        empty={<p>Nothing.</p>}
      />
      <p className="text-sm text-muted-foreground">{`${String(report.unchanged)} unchanged.`}</p>
      {warnings}
    </div>
  );
}

type Checked = { file: unknown; report: ImportReport };

function readText(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      resolve(typeof reader.result === "string" ? reader.result : "");
    };
    reader.onerror = () => {
      reject(new ConsoleRefusal("This file could not be read."));
    };
    reader.readAsText(file);
  });
}

/** Moves the configuration between gateways: a file out, a file in, checked before it is applied. */
export function ConfigSection() {
  const headingId = useId();
  const input = useRef<HTMLInputElement>(null);
  const run = useImportConfig();
  const toast = useToast();
  const { mutateAsync, reset } = run;
  const [checked, setChecked] = useState<Checked | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [asking, setAsking] = useState(false);

  async function choose(file: File | undefined) {
    // Choosing again starts over, whatever was shown.
    setChecked(null);
    setProblem(null);
    if (file === undefined) return;
    try {
      if (file.size > MAX_BYTES) throw new ConsoleRefusal(TOO_LARGE);
      const text = await readText(file);
      let parsed: unknown;
      try {
        parsed = JSON.parse(text);
      } catch {
        throw new ConsoleRefusal(NOT_JSON);
      }
      setChecking(true);
      const report = await mutateAsync({ file: parsed, dryRun: true });
      setChecked({ file: parsed, report });
    } catch (error) {
      setProblem(messageOfError(error));
    } finally {
      setChecking(false);
      reset();
      // The same file can be chosen again.
      if (input.current !== null) input.current.value = "";
    }
  }

  async function apply() {
    if (checked === null) return;
    try {
      const report = await mutateAsync({ file: checked.file, dryRun: false });
      if (report.errors.length > 0) {
        // The gateway is not as it was when the file was checked: nothing was written.
        setChecked({ file: checked.file, report });
        return;
      }
      setChecked(null);
      toast(IMPORTED);
    } finally {
      reset();
    }
  }

  const report = checked?.report;
  const canApply =
    report !== undefined && report.errors.length === 0 && report.created.length + report.updated.length > 0;

  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-4">
      <h2 id={headingId} className="text-lg font-medium">
        Configuration
      </h2>
      <div className="flex flex-col gap-3">
        <h3 className="text-sm font-medium">Export</h3>
        <p className="max-w-prose text-sm">
          The setup as one file: providers (without their credentials), models with who may call
          them, teams, routes, limits, budgets and settings. No credential, key, token, password or
          log is in the file.
        </p>
        <DownloadLink href="/api/config/export" variant="outline">
          Download configuration
        </DownloadLink>
      </div>
      <div className="flex flex-col gap-3">
        <h3 className="text-sm font-medium">Import</h3>
        <p className="max-w-prose text-sm text-muted-foreground">
          Choose a file to see what it would do before anything is written. What is missing is
          created and what exists, by name, is updated. Nothing is deleted. A new provider has no
          credential until you set one.
        </p>
        <div className="flex max-w-md flex-col gap-2">
          <Label htmlFor={`${headingId}-file`}>Configuration file</Label>
          <Input
            id={`${headingId}-file`}
            ref={input}
            type="file"
            accept="application/json,.json"
            className={`${control} cursor-pointer`}
            disabled={checking}
            onChange={(event) => {
              void choose(event.target.files?.[0]);
            }}
          />
        </div>
        {checking ? (
          <p role="status" className="text-sm text-muted-foreground">
            Checking the file
          </p>
        ) : null}
        {problem === null ? null : (
          <Alert variant="destructive">
            <AlertDescription>
              <p>{problem}</p>
            </AlertDescription>
          </Alert>
        )}
        {report === undefined ? null : <Report report={report} />}
        {canApply ? (
          <div>
            <Button
              type="button"
              className={control}
              onClick={() => {
                setAsking(true);
              }}
            >
              Apply import
            </Button>
          </div>
        ) : null}
      </div>
      <ConfirmDialog
        open={asking && report !== undefined}
        onOpenChange={setAsking}
        title="Apply this import?"
        body={
          report === undefined
            ? ""
            : `It creates ${count(report.created.length, "thing", "things")} and updates ${String(report.updated.length)}. Nothing is deleted. It takes effect at once.`
        }
        confirmLabel="Apply"
        onConfirm={apply}
      />
    </section>
  );
}
