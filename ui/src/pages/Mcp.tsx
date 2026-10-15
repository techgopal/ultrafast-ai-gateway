import { Link } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import {
  useCreateMcpServer,
  useDeleteMcpServer,
  useMcpCatalog,
  useMcpServers,
  useMcpStderr,
  useMcpTools,
  useMcpToolsOf,
  usePutMcpToolGrants,
  useSyncMcpServer,
  useUpdateMcpServer,
  useUpdateMcpTool,
} from "@/api/queries";
import { messageOfError } from "@/api/errors";
import type { components } from "@/api/schema";
import { can, type Me } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control, longText } from "@/components/classes";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { PageHeader } from "@/components/PageHeader";
import { QueryProblem } from "@/components/QueryProblem";
import { useToast } from "@/components/toast";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { isLocal, liveTools, statusOf, syncText, toolRef } from "@/lib/mcp";
import { accessSummary, hasNoAccess } from "@/lib/models";
import { AccessDialog } from "@/pages/McpAccess";
import { ServerDialog } from "@/pages/McpServer";

type Server = components["schemas"]["McpServerView"];
type Tool = components["schemas"]["McpToolView"];

export const DELETE_CONSEQUENCE =
  "Its tools are removed, and keys that name them can no longer call them.";
export const NO_ACCESS_HINT = "On, but nobody has access yet.";
export const EMPTY_FOR_MEMBER = "No MCP tools are available to you yet. Ask an admin.";

export const DONE = {
  add: "Server added. Sync it to read its tools.",
  save: "Server saved.",
  access: "Access updated.",
  delete: "Server deleted.",
} as const;

type Asking = "add" | "edit" | "access" | "delete";

function ToolsOfServer({
  server,
  onProblem,
  onAccess,
}: {
  server: Server;
  onProblem: (message: string | null) => void;
  onAccess: (tool: Tool) => void;
}) {
  const tools = useMcpTools(server.id);
  const update = useUpdateMcpTool();
  const [changing, setChanging] = useState<ReadonlySet<number>>(new Set());
  const rows = useMemo(
    () => [...(tools.data?.tools ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
    [tools.data],
  );

  async function toggle(tool: Tool, enabled: boolean) {
    onProblem(null);
    setChanging((now) => new Set(now).add(tool.id));
    try {
      await update.mutateAsync({ id: tool.id, body: { enabled } });
    } catch (error) {
      onProblem(messageOfError(error));
    } finally {
      setChanging((now) => {
        const next = new Set(now);
        next.delete(tool.id);
        return next;
      });
    }
  }

  const columns: Column<Tool>[] = [
    {
      id: "name",
      header: "Name",
      cell: (tool) => (
        <span className={`${longText} flex flex-col gap-1 md:max-w-80`}>
          <span className="font-mono font-medium break-all">{tool.name}</span>
          {tool.title === null ? null : (
            <span className="text-xs text-muted-foreground">{tool.title}</span>
          )}
          {tool.removed ? <Badge variant="outline">Removed by the server</Badge> : null}
        </span>
      ),
      sortValue: (tool) => tool.name,
    },
    {
      id: "status",
      header: "Status",
      cell: (tool) => (
        <Label className={`${control} gap-2`}>
          <Switch
            aria-label={`${tool.name} on`}
            checked={tool.enabled}
            disabled={tool.removed || changing.has(tool.id)}
            onCheckedChange={(on) => {
              void toggle(tool, on);
            }}
          />
          <span>{tool.enabled ? "On" : "Off"}</span>
        </Label>
      ),
      sortValue: (tool) => (tool.enabled ? 1 : 0),
    },
    {
      id: "access",
      header: "Access",
      cell: (tool) => (
        <span className="inline-flex flex-col items-start gap-1">
          <span>{accessSummary(tool.grants)}</span>
          {tool.enabled && hasNoAccess(tool.grants) ? (
            <span className="text-xs text-muted-foreground">{NO_ACCESS_HINT}</span>
          ) : null}
        </span>
      ),
    },
  ];

  if (tools.error !== null && tools.data === undefined) {
    return (
      <ErrorState
        error={tools.error}
        onRetry={() => {
          void tools.refetch();
        }}
      />
    );
  }
  return (
    <DataTable
      caption={`Tools of ${server.name}`}
      columns={columns}
      rows={rows}
      loading={tools.isPending}
      getRowId={(tool) => String(tool.id)}
      empty={
        <EmptyState title="No tools" description="Sync the server to read the tools it offers." />
      }
      actions={(tool) => (
        <Button
          type="button"
          variant="outline"
          className={control}
          onClick={() => {
            onAccess(tool);
          }}
        >
          Edit access
        </Button>
      )}
    />
  );
}

/** The end of what the program of a local server wrote to stderr. */
function ServerOutput({ server, onClose }: { server: Server; onClose: () => void }) {
  const output = useMcpStderr(server.id);
  return (
    <section className="flex flex-col gap-3" aria-label={`Output of ${server.name}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-base font-medium">{`Output of ${server.name}`}</h2>
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            className={control}
            onClick={() => {
              void output.refetch();
            }}
          >
            Refresh output
          </Button>
          <Button type="button" variant="outline" className={control} onClick={onClose}>
            Close output
          </Button>
        </div>
      </div>
      {output.error !== null && output.data === undefined ? (
        <ErrorState
          error={output.error}
          onRetry={() => {
            void output.refetch();
          }}
        />
      ) : (
        <Textarea
          readOnly
          aria-label={`Output of ${server.name}, last lines`}
          className="max-h-80 min-h-32 font-mono text-xs"
          value={
            output.data === undefined
              ? "Loading..."
              : output.data.text === ""
                ? "Nothing written yet."
                : output.data.text
          }
        />
      )}
    </section>
  );
}

function AdminMcp() {
  const servers = useMcpServers();
  const create = useCreateMcpServer();
  const update = useUpdateMcpServer();
  const remove = useDeleteMcpServer();
  const sync = useSyncMcpServer();
  const grants = usePutMcpToolGrants();
  const catalog = useMcpCatalog();
  const toast = useToast();
  const [output, setOutput] = useState<Server | null>(null);
  const [asking, setAsking] = useState<Asking | null>(null);
  const [adding, setAdding] = useState(false);
  const [target, setTarget] = useState<Server | null>(null);
  const [tool, setTool] = useState<Tool | null>(null);
  const [picked, setPicked] = useState<number | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  const all = useMemo(
    () => [...(servers.data?.servers ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
    [servers.data],
  );
  // Until one is picked: the first server that has tools.
  const shown =
    all.find((server) => server.id === picked) ?? all.find((server) => server.tool_count > 0) ?? all[0];

  function closing(reset: () => void) {
    setAsking(null);
    reset();
  }

  async function syncServer(server: Server) {
    setProblem(null);
    try {
      toast(syncText(await sync.mutateAsync({ id: server.id })));
    } catch (error) {
      setProblem(messageOfError(error));
    }
  }

  const columns: Column<Server>[] = [
    {
      id: "name",
      header: "Name",
      cell: (server) => (
        <span className={`${longText} flex flex-col gap-1 md:max-w-64`}>
          <span className="font-mono font-medium break-all">{server.name}</span>
          {server.description === "" ? null : (
            <span className="text-xs text-muted-foreground">{server.description}</span>
          )}
        </span>
      ),
      sortValue: (server) => server.name,
    },
    {
      id: "url",
      header: "Address",
      cell: (server) => (
        <span className={`${longText} font-mono text-xs break-all md:max-w-80`}>
          {isLocal(server)
            ? [server.command ?? "", ...(server.args ?? [])].join(" ")
            : (server.url ?? "")}
        </span>
      ),
      hideOnMobile: true,
    },
    {
      id: "status",
      header: "Status",
      cell: (server) => (
        <span className="inline-flex flex-col items-start gap-1">
          <span>{statusOf(server)}</span>
          {server.last_sync_error === null ? null : (
            <span className={`${longText} text-xs text-muted-foreground md:max-w-64`}>
              {server.last_sync_error}
            </span>
          )}
        </span>
      ),
    },
    {
      id: "tools",
      header: "Tools",
      cell: (server) => <span className="tabular-nums">{server.tool_count}</span>,
      sortValue: (server) => server.tool_count,
    },
  ];

  const failed = servers.error !== null && servers.data === undefined;
  if (failed) {
    return (
      <QueryProblem
        title="MCP tools"
        error={servers.error}
        onRetry={() => {
          void servers.refetch();
        }}
      />
    );
  }

  return (
    <>
      <div className="flex flex-col">
        <PageHeader
          title="MCP tools"
          actions={
            <>
              <Button asChild variant="outline" className={control}>
                <Link to="/mcp/catalog">Add from catalog</Link>
              </Button>
              <Button
                type="button"
                className={control}
                onClick={() => {
                  setAdding(true);
                  setTarget(null);
                  setAsking("add");
                }}
              >
                Add server
              </Button>
            </>
          }
        />
        {problem === null ? null : (
          <Alert variant="destructive" className="mt-6">
            <AlertDescription>
              <p>{problem}</p>
            </AlertDescription>
          </Alert>
        )}
      </div>
      <DataTable
        caption="MCP servers"
        columns={columns}
        rows={all}
        loading={servers.isPending}
        getRowId={(server) => String(server.id)}
        empty={
          <EmptyState
            title="No MCP servers"
            description="Add a server, then sync it to read the tools it offers."
          />
        }
        actions={(server) => (
          <>
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setPicked(server.id);
              }}
            >
              Tools
            </Button>
            <Button
              type="button"
              variant="outline"
              className={control}
              disabled={sync.isPending}
              onClick={() => {
                void syncServer(server);
              }}
            >
              Sync
            </Button>
            {isLocal(server) ? (
              <Button
                type="button"
                variant="outline"
                className={control}
                onClick={() => {
                  setOutput(server);
                }}
              >
                Output
              </Button>
            ) : null}
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setAdding(false);
                setTarget(server);
                setAsking("edit");
              }}
            >
              Edit
            </Button>
            <Button
              type="button"
              variant="outline"
              className={control}
              onClick={() => {
                setTarget(server);
                setAsking("delete");
              }}
            >
              Delete
            </Button>
          </>
        )}
      />
      {shown === undefined ? null : (
        <section className="flex flex-col gap-3">
          <h2 className="text-base font-medium">{`Tools of ${shown.name}`}</h2>
          <ToolsOfServer
            key={shown.id}
            server={shown}
            onProblem={setProblem}
            onAccess={(one) => {
              setTool(one);
              setAsking("access");
            }}
          />
        </section>
      )}
      {output === null ? null : (
        <ServerOutput
          server={output}
          onClose={() => {
            setOutput(null);
          }}
        />
      )}
      <ServerDialog
        open={asking === "add" || asking === "edit"}
        adding={adding}
        localEnabled={catalog.data?.local_enabled ?? false}
        server={target}
        create={create}
        update={update}
        onCancel={() => {
          closing(() => {
            create.reset();
            update.reset();
          });
        }}
        onDone={() => {
          const wasAdding = adding;
          closing(() => {
            create.reset();
            update.reset();
          });
          toast(wasAdding ? DONE.add : DONE.save);
        }}
      />
      <AccessDialog
        open={asking === "access"}
        tool={tool}
        put={grants}
        onCancel={() => {
          closing(grants.reset);
        }}
        onDone={() => {
          closing(grants.reset);
          toast(DONE.access);
        }}
      />
      <ConfirmDialog
        open={asking === "delete"}
        onOpenChange={(open) => {
          if (!open) closing(remove.reset);
        }}
        title={`Delete ${target?.name ?? "this server"}?`}
        body={DELETE_CONSEQUENCE}
        confirmLabel="Delete"
        tone="danger"
        onConfirm={async () => {
          if (target === null) return;
          await remove.mutateAsync({ id: target.id });
          toast(DONE.delete);
        }}
      />
    </>
  );
}

function MemberMcp() {
  const servers = useMcpServers();
  const list = useMemo(
    () => [...(servers.data?.servers ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
    [servers.data],
  );
  const tools = useMcpToolsOf(list, servers.data !== undefined);

  if (servers.error !== null && servers.data === undefined) {
    return (
      <QueryProblem
        title="MCP tools"
        error={servers.error}
        onRetry={() => {
          void servers.refetch();
        }}
      />
    );
  }
  const waiting = servers.isPending || tools.some((one) => one.isPending);
  const groups = list
    .map((server, at) => ({ server, tools: liveTools(tools[at]?.data?.tools ?? []) }))
    .filter((group) => group.tools.length > 0);
  return (
    <>
      <PageHeader title="MCP tools" />
      {waiting ? null : groups.length === 0 ? (
        <EmptyState title="No MCP tools" description={EMPTY_FOR_MEMBER} />
      ) : (
        groups.map(({ server, tools: callable }) => (
          <section key={server.id} className="flex flex-col gap-3">
            <h2 className="font-mono text-base font-medium break-all">{server.name}</h2>
            {server.description === "" ? null : (
              <p className="text-sm text-muted-foreground">{server.description}</p>
            )}
            <ul aria-label={`Tools of ${server.name}`} className="flex flex-col gap-2">
              {callable.map((tool) => (
                <li key={tool.id} className="flex flex-col gap-1 rounded-lg border bg-card p-3">
                  <span className="font-mono font-medium break-all">{tool.name}</span>
                  <span className="font-mono text-xs break-all text-muted-foreground">
                    {toolRef(server.name, tool.name)}
                  </span>
                  {tool.description === "" ? null : (
                    <span className="text-sm">{tool.description}</span>
                  )}
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
    </>
  );
}

function McpFor({ me }: { me: Me }) {
  return can(me, { type: "manageModels" }) ? <AdminMcp /> : <MemberMcp />;
}

/** MCP servers and their tools: managed by an admin, a read-only list for everybody else. */
export function Mcp() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  return <McpFor me={session.me} />;
}
