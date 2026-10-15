import { useMcpServers, useMcpToolsOf, useModels, useRoutes } from "@/api/queries";
import { Checks } from "@/components/CheckList";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { callableItems, type CallableItem } from "@/lib/keys";
import { liveTools, toolRef } from "@/lib/mcp";

export const ALL_MODELS = "All models I can use";
/** For a key a lead makes for another member: it calls what is everyone's or the team's. */
export const ALL_TEAM_MODELS = "All models of everyone and the team";
export const ONLY_THESE = "Only these";

/** What the form can offer to limit a key to: `null` while the lists are on their way. */
export interface Callable {
  items: readonly CallableItem[] | null;
  /** The MCP tools the key can be limited to, as `mcp:server/tool`; none while they are on their way. */
  mcp: readonly CallableItem[];
  error: unknown;
  retry: () => void;
}

/**
 * The models and routes the viewer can use, read when the key is limited to
 * some (`enabled`): a key for all of them asks for nothing. The gateway gives
 * a member the models they may call and the routes they may use, and an admin
 * every one. A key a lead makes for another member of their team is limited
 * from what is granted to everyone or to that team (`keyTeamId`), as the
 * gateway checks the names.
 */
export function useCallable(enabled: boolean, keyTeamId: number | null = null): Callable {
  const models = useModels(enabled, keyTeamId);
  const routes = useRoutes(enabled, keyTeamId);
  const servers = useMcpServers(enabled, keyTeamId);
  const serverList = servers.data?.servers ?? [];
  const tools = useMcpToolsOf(serverList, enabled && servers.data !== undefined, keyTeamId);
  const mcp = serverList.flatMap((server, at) =>
    liveTools(tools[at]?.data?.tools ?? []).map((tool) => ({
      id: toolRef(server.name, tool.name),
      route: false,
    })),
  );
  const items =
    models.data === undefined || routes.data === undefined
      ? null
      : callableItems(models.data.models, routes.data.routes);
  return {
    items,
    mcp,
    error: (models.data === undefined ? models.error : null) ?? routes.error,
    retry: () => {
      if (models.data === undefined) void models.refetch();
      if (routes.data === undefined) void routes.refetch();
    },
  };
}

interface AllowedFieldProps {
  mode: "all" | "some";
  onMode: (mode: "all" | "some") => void;
  chosen: readonly string[];
  onChosen: (names: string[]) => void;
  callable: Callable;
  /** What is wrong with the choice of names. */
  error: string | undefined;
  /** A key a lead makes for another member: it calls what is everyone's or the team's. */
  teamKey?: boolean;
  /** What the list says when there is nothing in it. */
  none?: string;
}

/** "All models I can use", or a choice of the models and routes the key may call. */
export function AllowedField({
  mode,
  onMode,
  chosen,
  onChosen,
  callable,
  error,
  teamKey = false,
  none = "There is no model or route you can use yet.",
}: AllowedFieldProps) {
  return (
    <>
      <Field group label="Allowed models" name="allow">
        {({ id, name, ...described }) => (
          <RadioGroup
            {...described}
            id={id}
            name={name}
            value={mode}
            onValueChange={(next) => {
              onMode(next === "some" ? "some" : "all");
            }}
          >
            {(
              [
                ["all", teamKey ? ALL_TEAM_MODELS : ALL_MODELS],
                ["some", ONLY_THESE],
              ] as const
            ).map(([value, label]) => (
              <Label key={value} htmlFor={`${id}-${value}`} className={control}>
                <RadioGroupItem id={`${id}-${value}`} value={value} />
                {label}
              </Label>
            ))}
          </RadioGroup>
        )}
      </Field>
      {mode === "some" ? (
        <Field group label="Models and routes" name="allowed" error={error}>
          {(wiring) => (
            <Checks
              wiring={wiring}
              items={callable.items}
              error={callable.error}
              retry={callable.retry}
              loading="Loading the models and routes"
              none={none}
              checked={chosen}
              onChange={onChosen}
              label={(item) => (
                <>
                  <span className="font-mono break-all">{item.id}</span>
                  {item.route ? <Badge variant="outline">Route</Badge> : null}
                </>
              )}
            />
          )}
        </Field>
      ) : null}
      {mode === "some" && callable.mcp.length > 0 ? (
        <Field group label="MCP tools" name="allowed_mcp">
          {(wiring) => (
            <Checks
              wiring={wiring}
              items={callable.mcp}
              error={null}
              retry={() => undefined}
              loading="Loading the MCP tools"
              none="There is no MCP tool you can use yet."
              checked={chosen}
              onChange={onChosen}
              label={(item) => <span className="font-mono break-all">{item.id}</span>}
            />
          )}
        </Field>
      ) : null}
    </>
  );
}
