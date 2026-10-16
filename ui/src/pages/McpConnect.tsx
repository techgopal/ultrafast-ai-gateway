import { useState } from "react";
import { control } from "@/components/classes";
import { Button } from "@/components/ui/button";

/** How an agent reaches the gateway: one address for every tool, or one for each server. */
export function ConnectPanel({ servers }: { servers: readonly string[] }) {
  const [open, setOpen] = useState(false);
  const origin = window.location.origin;
  return (
    <div className="flex flex-col gap-3">
      <div>
        <Button
          type="button"
          variant="outline"
          className={control}
          aria-expanded={open}
          onClick={() => {
            setOpen((now) => !now);
          }}
        >
          Connect an agent
        </Button>
      </div>
      {open ? (
        <section aria-label="Connect an agent" className="flex flex-col gap-3 rounded-lg border p-4 text-sm">
          <p>
            Give the agent the address and a virtual key as the bearer token. The key decides which
            tools it sees.
          </p>
          <p className="font-medium">All tools</p>
          <code className="font-mono text-xs break-all">{`${origin}/mcp`}</code>
          <p className="text-muted-foreground">
            Tools are named <code className="font-mono">server__tool</code>. A key with many tools
            lists a search tool, <code className="font-mono">find_tools</code>, instead of all of
            them.
          </p>
          {servers.length === 0 ? null : (
            <>
              <p className="font-medium">One server</p>
              <ul aria-label="Addresses of the servers" className="flex flex-col gap-1">
                {servers.map((name) => (
                  <li key={name} className="font-mono text-xs break-all">{`${origin}/mcp/${name}`}</li>
                ))}
              </ul>
            </>
          )}
          <pre className="overflow-auto rounded-md border bg-muted p-3 font-mono text-xs">{`{ "mcpServers": { "gateway": { "url": "${origin}/mcp",
    "headers": { "Authorization": "Bearer uf_..." } } } }`}</pre>
        </section>
      ) : null}
    </div>
  );
}
