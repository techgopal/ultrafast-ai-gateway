import { Link, useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import {
  useCreatePrompt,
  useCreatePromptVersion,
  useDeletePrompt,
  usePrompt,
  usePromptVersion,
} from "@/api/queries";
import type { components } from "@/api/schema";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { FilterSelect, type Choice } from "@/components/FilterSelect";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { NotFoundContent } from "@/components/NotFoundContent";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { idOf } from "@/lib/id";
import {
  diffMessages,
  draftOf,
  emptyDraft,
  requestOf,
  versionRequestOf,
  type DiffLine,
  type Draft,
} from "@/lib/prompts";
import { deleteConsequence } from "@/pages/Prompts";
import { PromptForm } from "@/pages/PromptsForm";

type View = components["schemas"]["PromptView"];
type Version = components["schemas"]["VersionView"];

export const DONE = {
  create: "Template created.",
  version: "Version saved.",
  delete: "Template deleted.",
} as const;

export const UNREADABLE = "This version cannot be read. Add a version to replace it.";
export const NO_VERSIONS = "This template has no versions yet.";

/** What kind of response format a version asks for, in a word; `null` when it asks for none. */
function formatOf(version: Version): string | null {
  const format: unknown = version.params.response_format;
  if (format === null || format === undefined) return null;
  const kind = typeof format === "object" ? (format as { type?: unknown }).type : undefined;
  return typeof kind === "string" ? kind : "set";
}

/** The settings of a version, as a sentence; `null` when it sets none. */
export function settingsOf(version: Version): string | null {
  const params = version.params;
  const parts: string[] = [];
  if (params.temperature != null) parts.push(`Temperature ${String(params.temperature)}`);
  if (params.max_tokens != null) parts.push(`max tokens ${String(params.max_tokens)}`);
  if (params.top_p != null) parts.push(`top P ${String(params.top_p)}`);
  const first = parts[0];
  if (first === undefined) return null;
  return [first.charAt(0).toUpperCase() + first.slice(1), ...parts.slice(1)].join(", ");
}

function BackLink() {
  return (
    <Link
      to="/prompts"
      className="inline-flex min-h-11 w-fit items-center rounded-sm text-sm text-muted-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-8"
    >
      Back to prompts
    </Link>
  );
}

function Loading({ title }: { title: string }) {
  return (
    <>
      <PageHeader title={title} />
      <div role="status" aria-busy="true" aria-label="Loading the template" className="flex flex-col gap-4">
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
        <Skeleton className="h-4 w-full max-w-md" />
      </div>
    </>
  );
}

/** The messages of one version, as they will be sent: the variables are still in them. */
function Messages({ version }: { version: Version }) {
  return (
    <ol aria-label={`Messages of version ${String(version.version)}`} className="flex flex-col gap-3">
      {version.messages.map((message, at) => (
        <li key={at} className="flex flex-col gap-1 rounded-md border p-3">
          <span className="text-xs font-medium text-muted-foreground">{message.role}</span>
          <pre className="font-mono text-xs break-words whitespace-pre-wrap">{message.content}</pre>
        </li>
      ))}
    </ol>
  );
}

/** What changed between two versions, in words as well as in marks. */
function Changes({ lines, from, to }: { lines: readonly DiffLine[]; from: number; to: number }) {
  const heading = `Changes from version ${String(from)} to version ${String(to)}`;
  const changed = lines.some((line) => line.kind !== "same");
  return (
    <section aria-label={heading} className="flex flex-col gap-2">
      <h3 className="text-sm font-medium">{heading}</h3>
      {changed ? null : <p className="text-sm text-muted-foreground">The messages are the same.</p>}
      <ul className="flex flex-col font-mono text-xs">
        {lines.map((line, at) => (
          <li
            key={at}
            className={
              line.kind === "add"
                ? "bg-green-500/15 whitespace-pre-wrap break-words"
                : line.kind === "remove"
                  ? "bg-red-500/15 whitespace-pre-wrap break-words"
                  : "whitespace-pre-wrap break-words text-muted-foreground"
            }
          >
            <span aria-hidden="true">{line.kind === "add" ? "+ " : line.kind === "remove" ? "- " : "  "}</span>
            <span className="sr-only">{line.kind === "add" ? "Added: " : line.kind === "remove" ? "Removed: " : ""}</span>
            {line.text}
          </li>
        ))}
      </ul>
    </section>
  );
}

function VersionPart({
  view,
  mayManage,
}: {
  view: View;
  mayManage: boolean;
}) {
  const toast = useToast();
  const add = useCreatePromptVersion();
  const [shown, setShown] = useState<number | null>(null);
  const [compare, setCompare] = useState<number | null>(null);
  // Where the editor starts: the latest version, or the one chosen with "Use version N as the start".
  const [startFrom, setStartFrom] = useState<{ version: number; times: number } | null>(null);

  const numbers = view.versions.map((one) => one.version);
  const latest = view.latest_version;
  const selected = shown !== null && numbers.includes(shown) ? shown : latest;
  const against = compare !== null && compare !== selected && numbers.includes(compare) ? compare : null;
  const current = usePromptVersion(view.id, selected, selected > 0);
  const other = usePromptVersion(view.id, against ?? 0, against !== null);
  const base = startFrom !== null && numbers.includes(startFrom.version) ? startFrom.version : latest;
  const baseQuery = usePromptVersion(view.id, base, mayManage && base > 0);

  const choices: Choice[] = [...numbers]
    .sort((a, b) => b - a)
    .map((n) => ({ value: String(n), label: `Version ${String(n)}${n === latest ? " (latest)" : ""}` }));
  const compareChoices: Choice[] = [
    { value: "none", label: "Nothing" },
    ...choices.filter((choice) => choice.value !== String(selected)).map((choice) => ({
      value: choice.value,
      label: `Version ${choice.value}`,
    })),
  ];

  const lines = useMemo(
    () =>
      current.data !== undefined && other.data !== undefined && against !== null
        ? diffMessages(other.data.messages, current.data.messages)
        : null,
    [current.data, other.data, against],
  );

  const start: Draft | null = useMemo(() => {
    if (!mayManage) return null;
    if (base === 0) return { ...emptyDraft(), name: view.name, description: view.description };
    return baseQuery.data === undefined ? null : draftOf(view, baseQuery.data);
  }, [mayManage, base, baseQuery.data, view]);
  const unreadableBase = base > 0 && baseQuery.data === undefined && baseQuery.error !== null;
  const editorStart = start ?? (unreadableBase ? { ...emptyDraft(), name: view.name, description: view.description } : null);

  async function save(draft: Draft) {
    await add.mutateAsync({ id: view.id, body: versionRequestOf(draft) });
    add.reset();
    toast(DONE.version);
    setShown(null);
    setCompare(null);
    setStartFrom(null);
  }

  return (
    <>
      <section aria-labelledby="version-heading" className="flex max-w-2xl flex-col gap-4">
        <h2 id="version-heading" className="text-lg font-medium">
          Version
        </h2>
        {numbers.length === 0 ? (
          <p className="text-sm text-muted-foreground">{NO_VERSIONS}</p>
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <FilterSelect
                label="Version"
                value={String(selected)}
                choices={choices}
                onChange={(value) => {
                  setShown(Number(value));
                }}
              />
              {numbers.length > 1 ? (
                <FilterSelect
                  label="Compare with"
                  value={against === null ? "none" : String(against)}
                  choices={compareChoices}
                  onChange={(value) => {
                    setCompare(value === "none" ? null : Number(value));
                  }}
                />
              ) : null}
              <Button asChild variant="outline" className={control}>
                <Link to="/playground" search={{ prompt: view.name, version: selected }}>
                  Open in Playground
                </Link>
              </Button>
              {mayManage && current.data !== undefined ? (
                <Button
                  type="button"
                  variant="outline"
                  className={control}
                  onClick={() => {
                    setStartFrom((before) => ({ version: selected, times: (before?.times ?? 0) + 1 }));
                  }}
                >
                  {`Use version ${String(selected)} as the start`}
                </Button>
              ) : null}
            </div>
            {current.data === undefined ? (
              current.error === null ? (
                <div role="status" aria-busy="true" aria-label="Loading the version">
                  <Skeleton className="h-16 w-full" />
                </div>
              ) : (
                <p role="alert" className="text-sm text-destructive">
                  {UNREADABLE}
                </p>
              )
            ) : (
              <>
                <p className="text-sm text-muted-foreground">
                  Written <Timestamp value={current.data.created_at} />. A version never changes.
                </p>
                <Messages version={current.data} />
                <p className="text-sm">
                  {current.data.variables.length === 0
                    ? "No variables."
                    : `Variables: ${current.data.variables.join(", ")}`}
                </p>
                <p className="text-sm">
                  {current.data.model === null ? "Model: none, a call names one" : `Model: ${current.data.model}`}
                </p>
                {settingsOf(current.data) === null ? null : <p className="text-sm">{settingsOf(current.data)}</p>}
                {formatOf(current.data) === null ? null : (
                  <p className="text-sm">{`Response format: ${formatOf(current.data) ?? ""}`}</p>
                )}
              </>
            )}
            {against !== null && lines !== null ? <Changes lines={lines} from={against} to={selected} /> : null}
          </>
        )}
      </section>
      {mayManage ? (
        <section aria-labelledby="new-version-heading" className="flex max-w-2xl flex-col gap-4">
          <h2 id="new-version-heading" className="text-lg font-medium">
            New version
          </h2>
          <p className="text-sm text-muted-foreground">
            A version stands alone and never changes once it is saved. Calls that name no version get the latest.
          </p>
          {editorStart === null ? (
            <Skeleton className="h-32 w-full" />
          ) : (
            <PromptForm
              key={`${String(view.id)}-${String(latest)}-${String(startFrom?.version ?? 0)}-${String(startFrom?.times ?? 0)}`}
              mode="version"
              start={editorStart}
              label="New version"
              submitLabel={`Save as version ${String(latest + 1)}`}
              pending={add.isPending}
              onSend={save}
            />
          )}
        </section>
      ) : null}
    </>
  );
}

function Existing({ id }: { id: number }) {
  const session = useSession();
  const query = usePrompt(id);
  const remove = useDeletePrompt();
  const toast = useToast();
  const navigate = useNavigate();
  const [asking, setAsking] = useState(false);
  if (session.status !== "signedIn") return null;
  if (query.data === undefined) {
    if (query.error !== null) {
      return (
        <QueryProblem
          title="Prompt template"
          notFound
          error={query.error}
          onRetry={() => {
            void query.refetch();
          }}
        />
      );
    }
    return <Loading title="Prompt template" />;
  }
  const view = query.data;
  const mayManage = can(session.me, { type: "managePrompt", createdBy: view.created_by });
  return (
    <>
      <BackLink />
      <PageHeader
        title={view.name}
        subtitle={view.description === "" ? "Prompt template" : view.description}
        actions={
          mayManage ? (
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setAsking(true);
              }}
            >
              Delete template
            </Button>
          ) : undefined
        }
      />
      <VersionPart view={view} mayManage={mayManage} />
      <ConfirmDialog
        open={asking}
        onOpenChange={(open) => {
          if (open) return;
          setAsking(false);
          remove.reset();
        }}
        title={`Delete ${view.name}?`}
        body={deleteConsequence(view)}
        confirmLabel="Delete"
        tone="danger"
        onConfirm={async () => {
          await remove.mutateAsync({ id: view.id });
          toast(DONE.delete);
          await navigate({ to: "/prompts" });
        }}
      />
    </>
  );
}

function NewTemplate() {
  const create = useCreatePrompt();
  const toast = useToast();
  const navigate = useNavigate();
  const [start] = useState(emptyDraft);
  async function make(draft: Draft) {
    const made = await create.mutateAsync(requestOf(draft));
    create.reset();
    toast(DONE.create);
    await navigate({ to: "/prompts/$id", params: { id: String(made.id) } });
  }
  return (
    <>
      <BackLink />
      <PageHeader title="New template" subtitle="Messages with {{variables}}. Version 1 is saved when you create it." />
      <PromptForm
        mode="create"
        start={start}
        label="Prompt template"
        submitLabel="Create template"
        pending={create.isPending}
        onSend={make}
        extra={
          <Button asChild variant="outline" className={control}>
            <Link to="/prompts">Cancel</Link>
          </Button>
        }
      />
    </>
  );
}

/**
 * The page of a new template (`id` is `null`) or of one template, as the
 * address has its id. Anyone signed in reads a template; making one is for an
 * admin and the lead of a team, and changing it for an admin and its maker.
 */
export function PromptsEdit({ id }: { id: string | null }) {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  if (id === null) {
    if (!can(session.me, { type: "createPrompt" })) return <NotAvailableContent />;
    return <NewTemplate />;
  }
  const number = idOf(id);
  // Not an id: the API is not asked.
  if (number === null) return <NotFoundContent />;
  return <Existing id={number} />;
}

