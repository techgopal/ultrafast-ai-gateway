import { Link, useNavigate } from "@tanstack/react-router";
import { useForm } from "@tanstack/react-form";
import { useMemo, useRef, useState } from "react";
import { messageOfError } from "@/api/errors";
import { useAddMcpFromCatalog, useMcpCatalog, useRefreshMcpCatalog } from "@/api/queries";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { applyApiError, onField, useFormFailure, useSubmit } from "@/components/form";
import { FormDialog, FormDialogFooter } from "@/components/FormDialog";
import { FormError } from "@/components/FormError";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { useToast } from "@/components/toast";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  catalogFailure,
  catalogRequest,
  entryBlock,
  envField,
  searchCatalog,
  sourceText,
  type CatalogValues,
} from "@/lib/mcp";

type Entry = components["schemas"]["Entry"];
type View = components["schemas"]["McpCatalogView"];

export const DONE = {
  add: "Server added from the catalog. Its tools start off.",
  refresh: "Catalog refreshed.",
} as const;
export const NAME_HINT = "Letters a to z, digits and dashes. It is part of the endpoint and cannot be changed.";

interface FormProps {
  entry: Entry;
  add: ReturnType<typeof useAddMcpFromCatalog>;
  onDone: () => void;
  onCancel: () => void;
}

// Mounted while the dialog is open: every opening starts from the entry.
function AddForm({ entry, add, onDone, onCancel }: FormProps) {
  const defaults: CatalogValues = {
    name: entry.id,
    ...Object.fromEntries(entry.env.map((item) => [envField(item.name), ""])),
  };
  const form = useForm({
    defaultValues: defaults,
    onSubmit: async ({ value }) => {
      try {
        await add.mutateAsync(catalogRequest(entry, value));
        onDone();
      } catch (error) {
        applyApiError(form, catalogFailure(onField(error, "mcp_server_exists", "name")));
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const onSubmit = useSubmit(form);

  return (
    <form
      ref={formRef}
      aria-label={`Add ${entry.name}`}
      noValidate
      className="flex flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="name">
        {(field) => (
          <Field label="Name" name={field.name} required hint={NAME_HINT} error={failure.fieldError(field.name)}>
            <Input
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              className={control}
              value={field.state.value}
              onBlur={field.handleBlur}
              onChange={(event) => {
                field.handleChange(event.target.value);
              }}
            />
          </Field>
        )}
      </form.Field>
      {entry.env.map((item) => (
        <form.Field key={item.name} name={envField(item.name)}>
          {(field) => (
            <Field
              label={item.name}
              name={field.name}
              required={item.secret}
              hint={item.help === "" ? undefined : item.help}
              error={failure.fieldError(field.name)}
            >
              <Input
                type={item.secret ? "password" : "text"}
                autoComplete="off"
                autoCapitalize="none"
                spellCheck={false}
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            </Field>
          )}
        </form.Field>
      ))}
      <FormDialogFooter
        running={add.isPending}
        submit="Add server"
        submitting="Adding the server"
        onCancel={onCancel}
      />
    </form>
  );
}

function EntryCard({
  entry,
  view,
  onAdd,
}: {
  entry: Entry;
  view: View;
  onAdd: (entry: Entry) => void;
}) {
  const block = entryBlock(entry, view);
  return (
    <li className="flex flex-col gap-2 rounded-lg border bg-card p-4">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-base font-medium break-words">{entry.name}</h2>
        <Badge variant="outline">{entry.transport === "stdio" ? "Runs on the gateway host" : "Remote"}</Badge>
      </div>
      <p className="text-sm">{entry.description}</p>
      <ul aria-label={`Tags of ${entry.name}`} className="flex flex-wrap gap-1">
        {entry.tags.map((tag) => (
          <li key={tag}>
            <Badge variant="secondary">{tag}</Badge>
          </li>
        ))}
      </ul>
      <p className="font-mono text-xs break-all text-muted-foreground">
        {entry.transport === "stdio"
          ? [entry.command ?? "", ...entry.args].join(" ")
          : (entry.url ?? "")}
      </p>
      <div className="flex flex-wrap items-center gap-3">
        <Button
          type="button"
          className={control}
          disabled={block !== null}
          aria-label={`Add ${entry.name}`}
          onClick={() => {
            onAdd(entry);
          }}
        >
          Add
        </Button>
        {block === null ? null : <span className="text-sm text-muted-foreground">{block}</span>}
      </div>
    </li>
  );
}

/** The catalog of MCP servers the project publishes; an admin adds one with its secrets. */
export function McpCatalog() {
  const catalog = useMcpCatalog();
  const refresh = useRefreshMcpCatalog();
  const add = useAddMcpFromCatalog();
  const toast = useToast();
  const navigate = useNavigate();
  const [search, setSearch] = useState("");
  const [asking, setAsking] = useState<Entry | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  const shown = useMemo(
    () => searchCatalog(catalog.data?.entries ?? [], search),
    [catalog.data, search],
  );

  if (catalog.error !== null && catalog.data === undefined) {
    return (
      <QueryProblem
        title="MCP catalog"
        error={catalog.error}
        onRetry={() => {
          void catalog.refetch();
        }}
      />
    );
  }
  const view = catalog.data;

  async function refreshNow() {
    setProblem(null);
    try {
      await refresh.mutateAsync(undefined);
      toast(DONE.refresh);
    } catch (error) {
      setProblem(messageOfError(error));
    }
  }

  return (
    <>
      <PageHeader
        title="MCP catalog"
        {...(view === undefined ? {} : { subtitle: sourceText(view) })}
        actions={
          <>
            <Button asChild variant="outline" className={control}>
              <Link to="/mcp">MCP tools</Link>
            </Button>
            <Button
              type="button"
              variant="outline"
              className={control}
              disabled={refresh.isPending || view?.fetch_enabled === false}
              onClick={() => {
                void refreshNow();
              }}
            >
              Refresh
            </Button>
          </>
        }
      />
      {view?.fetch_enabled === false ? (
        <Alert>
          <AlertDescription>
            <p>Fetching the catalog is switched off (UF_MCP_CATALOG=off). This is the copy that ships with the gateway.</p>
          </AlertDescription>
        </Alert>
      ) : null}
      {view?.error == null ? null : (
        <Alert variant="destructive">
          <AlertDescription>
            <p>{view.error}</p>
          </AlertDescription>
        </Alert>
      )}
      {problem === null ? null : (
        <Alert variant="destructive">
          <AlertDescription>
            <p>{problem}</p>
          </AlertDescription>
        </Alert>
      )}
      <div className="flex max-w-md flex-col gap-1">
        <Field label="Search" name="search">
          <Input
            type="search"
            autoComplete="off"
            className={control}
            value={search}
            onChange={(event) => {
              setSearch(event.target.value);
            }}
          />
        </Field>
      </div>
      {view === undefined ? null : shown.length === 0 ? (
        <EmptyState title="No entries" description="Nothing in the catalog matches the search." />
      ) : (
        <ul aria-label="Catalog entries" className="grid gap-3 md:grid-cols-2">
          {shown.map((entry) => (
            <EntryCard key={entry.id} entry={entry} view={view} onAdd={setAsking} />
          ))}
        </ul>
      )}
      <FormDialog
        open={asking !== null}
        running={add.isPending}
        title={asking === null ? "Add server" : `Add ${asking.name}`}
        description="Only what the entry asks for. The tools of a new server start off, and nobody has access to them."
        onCancel={() => {
          setAsking(null);
          add.reset();
        }}
      >
        {asking === null ? null : (
          <AddForm
            entry={asking}
            add={add}
            onCancel={() => {
              setAsking(null);
              add.reset();
            }}
            onDone={() => {
              setAsking(null);
              add.reset();
              toast(DONE.add);
              void navigate({ to: "/mcp" });
            }}
          />
        )}
      </FormDialog>
    </>
  );
}
