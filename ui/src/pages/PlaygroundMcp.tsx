// MCP tools mode of the playground: pick a server and a tool, write the
// arguments, run. The call goes through `/api/playground/mcp/{server}`, the
// pipeline of `/mcp/{server}` for the signed-in user, so it is limited,
// guarded and logged like an agent's call.
import { useEffect, useMemo, useRef, useState } from "react";
import { playgroundMcp } from "@/api/client";
import { useMcpServers, useMcpTools } from "@/api/queries";
import { control } from "@/components/classes";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { FilterSelect, type Choice } from "@/components/FilterSelect";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { liveTools, toolRef } from "@/lib/mcp";
import {
  argumentsOf,
  blocksOf,
  callOf,
  hintsOf,
  rpcErrorOf,
  skeletonOf,
  type Block,
} from "@/lib/playgroundMcp";
import { failureText } from "@/pages/PlaygroundRun";

export const NO_MCP_TOOLS = {
  title: "No MCP tools",
  description: "No MCP tool is available to you yet. Ask an admin.",
} as const;

interface Shown {
  called: string;
  failed: boolean;
  blocks: readonly Block[];
  structured: string | null;
  error: string | null;
}

export function McpMode() {
  const servers = useMcpServers();
  const [chosenServer, setChosenServer] = useState("");
  const [chosenTool, setChosenTool] = useState("");
  const [typed, setTyped] = useState("{}");
  const [running, setRunning] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [shown, setShown] = useState<Shown | null>(null);
  const controller = useRef<AbortController | null>(null);

  // A call that is on its way is ended with the page.
  useEffect(
    () => () => {
      controller.current?.abort();
    },
    [],
  );

  const serverList = useMemo(
    () => [...(servers.data?.servers ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
    [servers.data],
  );
  const serverChoices = useMemo<Choice[]>(
    () => serverList.map((one) => ({ value: one.name, label: one.name })),
    [serverList],
  );
  // A choice that is no longer offered is never sent.
  const server = serverList.find((one) => one.name === chosenServer) ?? serverList[0];
  const tools = useMcpTools(server?.id ?? 0, server !== undefined);
  const callable = useMemo(() => liveTools(tools.data?.tools ?? []), [tools.data]);
  const toolChoices = useMemo<Choice[]>(
    () => callable.map((one) => ({ value: one.name, label: one.name })),
    [callable],
  );
  const tool = callable.find((one) => one.name === chosenTool) ?? callable[0];
  const hints = useMemo(() => (tool === undefined ? [] : hintsOf(tool)), [tool]);

  async function run() {
    if (running || server === undefined || tool === undefined) return;
    const checked = argumentsOf(typed);
    if ("error" in checked) {
      setProblem(checked.error);
      return;
    }
    const abort = new AbortController();
    controller.current = abort;
    setRunning(true);
    setProblem(null);
    try {
      const reply = await playgroundMcp(server.name, callOf(tool.name, checked.value), abort.signal);
      const result: unknown = reply.result;
      const structured: unknown =
        typeof result === "object" && result !== null ? Reflect.get(result, "structuredContent") : undefined;
      setShown({
        called: toolRef(server.name, tool.name),
        failed:
          typeof result === "object" && result !== null && Reflect.get(result, "isError") === true,
        blocks: blocksOf(result),
        structured: structured === undefined ? null : JSON.stringify(structured, null, 2),
        error: rpcErrorOf(reply),
      });
    } catch (caught) {
      if (!abort.signal.aborted) {
        setShown(null);
        setProblem(failureText(caught));
      }
    }
    if (controller.current === abort) controller.current = null;
    setRunning(false);
  }

  if (servers.error !== null && servers.data === undefined) {
    return (
      <Alert variant="destructive">
        <AlertDescription>
          <p>The MCP servers could not be read.</p>
        </AlertDescription>
      </Alert>
    );
  }
  if (servers.data !== undefined && serverList.length === 0) {
    return <EmptyState {...NO_MCP_TOOLS} />;
  }

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
      <section aria-label="MCP tools" className="flex min-w-0 flex-col gap-4">
        <form
          aria-label="MCP call"
          noValidate
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            void run();
          }}
        >
          <div className="flex flex-wrap gap-3">
            <div className="flex flex-col gap-1">
              <p className="text-sm font-medium">Server</p>
              <FilterSelect
                label="MCP server"
                value={server?.name ?? ""}
                choices={serverChoices}
                onChange={(next) => {
                  setChosenServer(next);
                  setChosenTool("");
                  setShown(null);
                }}
              />
            </div>
            <div className="flex flex-col gap-1">
              <p className="text-sm font-medium">Tool</p>
              <FilterSelect
                label="MCP tool"
                value={tool?.name ?? ""}
                choices={toolChoices}
                onChange={(next) => {
                  setChosenTool(next);
                  const picked = callable.find((one) => one.name === next);
                  if (picked !== undefined) setTyped(skeletonOf(picked));
                  setShown(null);
                }}
              />
            </div>
          </div>
          {tool === undefined || tool.description === "" ? null : (
            <p className="text-sm text-muted-foreground">{tool.description}</p>
          )}
          <Field
            label="Arguments"
            name="arguments"
            hint={
              hints.length === 0
                ? "A JSON object. This tool takes no arguments that its schema names."
                : "A JSON object."
            }
          >
            {({ id, name, ...described }) => (
              <Textarea
                {...described}
                id={id}
                name={name}
                className="font-mono"
                autoComplete="off"
                spellCheck={false}
                value={typed}
                onChange={(event) => {
                  setTyped(event.target.value);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                    event.preventDefault();
                    void run();
                  }
                }}
              />
            )}
          </Field>
          {hints.length === 0 ? null : (
            <ul aria-label="Arguments the tool takes" className="flex flex-col gap-1 text-sm text-muted-foreground">
              {hints.map((hint) => (
                <li key={hint} className="font-mono text-xs break-words">
                  {hint}
                </li>
              ))}
            </ul>
          )}
          <div className="flex flex-wrap gap-2">
            <Button type="submit" className={control} disabled={tool === undefined || running}>
              Run tool
            </Button>
          </div>
        </form>
        {problem === null ? null : (
          <Alert variant="destructive">
            <AlertDescription>
              <p>{problem}</p>
            </AlertDescription>
          </Alert>
        )}
        {running ? (
          <p role="status" className="text-sm text-muted-foreground">
            Running...
          </p>
        ) : null}
        {shown === null || running ? null : (
          <section aria-label="Result" className="flex flex-col gap-2">
            <p className="font-mono text-xs break-all text-muted-foreground">{shown.called}</p>
            {shown.error === null ? null : (
              <Alert variant="destructive">
                <AlertDescription>
                  <p>{shown.error}</p>
                </AlertDescription>
              </Alert>
            )}
            {shown.failed ? (
              <p role="status" className="text-sm font-medium text-destructive">
                The tool reported an error.
              </p>
            ) : null}
            {shown.blocks.map((block, index) =>
              block.kind === "text" ? (
                <pre
                  key={index}
                  className="max-h-96 overflow-auto rounded-md border bg-muted p-3 font-mono text-xs whitespace-pre-wrap break-all"
                >
                  {block.text}
                </pre>
              ) : (
                <p key={index} className="text-sm text-muted-foreground">
                  {block.label}
                </p>
              ),
            )}
            {shown.structured === null ? null : (
              <details>
                <summary className="cursor-pointer text-sm">Structured result</summary>
                <pre className="max-h-96 overflow-auto rounded-md border bg-muted p-3 font-mono text-xs whitespace-pre-wrap break-all">
                  {shown.structured}
                </pre>
              </details>
            )}
          </section>
        )}
      </section>
      <section aria-label="About MCP calls" className="flex flex-col gap-2 text-sm text-muted-foreground">
        <p>
          Runs the tool as you: the same limits, budgets and guardrails apply as to an agent's call,
          and the call is logged like one.
        </p>
      </section>
    </div>
  );
}
