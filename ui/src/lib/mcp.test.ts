import { describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import {
  HOST_CHANGED,
  linesOf,
  projectionOf,
  projectionValues,
  sampleOf,
  argsOf,
  catalogRequest,
  envOf,
  localCreateOf,
  localUpdateOf,
  entryBlock,
  formFieldsOf,
  hostChanged,
  searchCatalog,
  sourceText,
  statusOf,
  syncText,
  toolRef,
  updateOf,
  type ServerValues,
} from "@/lib/mcp";

describe("toolRef", () => {
  test("names a tool as a key's allowlist does", () => {
    expect(toolRef("github", "create_issue")).toBe("mcp:github/create_issue");
  });
});

describe("syncText", () => {
  test("says what the sync changed", () => {
    expect(syncText(fixtures.mcpSyncResult)).toBe("Synced: added 1, updated 1, removed 0, skipped 0.");
  });
});

describe("hostChanged", () => {
  test("compares hosts, not paths", () => {
    expect(hostChanged("https://a.test/mcp", "https://a.test/other")).toBe(false);
    expect(hostChanged("https://a.test/mcp", "https://b.test/mcp")).toBe(true);
    expect(hostChanged("https://a.test/mcp", "https://a.test:8443/mcp")).toBe(true);
    expect(hostChanged("https://a.test/mcp", "not a url")).toBe(true);
  });
});

describe("statusOf", () => {
  test("says what an admin needs to know of a server", () => {
    expect(statusOf(fixtures.mcpServers.github)).toBe("Synced");
    expect(statusOf(fixtures.mcpServers.docs)).toBe("Sync failed");
    expect(statusOf({ ...fixtures.mcpServers.docs, last_sync_error: null })).toBe("Not synced yet");
    expect(statusOf({ ...fixtures.mcpServers.github, enabled: false })).toBe("Disabled");
    expect(statusOf({ ...fixtures.mcpServers.github, usable: false })).toBe("Secret needed");
  });
});

const github = fixtures.mcpServers.github;
const same: ServerValues = {
  url: "https://mcp.example.test/github",
  description: github.description,
  auth_kind: "bearer",
  auth_header: "",
  secret: "",
  oauth_token_url: "",
  oauth_client_id: "",
  oauth_scopes: "",
  oauth_grant: "client_credentials",
};

describe("updateOf", () => {
  test("sends only what changed, and the secret only when typed", () => {
    expect(updateOf(github, same)).toEqual({});
    expect(updateOf(github, { ...same, description: "New" })).toEqual({ description: "New" });
    expect(updateOf(github, { ...same, secret: "s3" })).toEqual({ secret: "s3" });
  });
  test("a new host needs the secret again", () => {
    expect(() => updateOf(github, { ...same, url: "https://other.test/mcp" })).toThrow(
      "Enter the secret again: the host changed.",
    );
    expect(updateOf(github, { ...same, url: "https://other.test/mcp", secret: "s3" })).toEqual({
      url: "https://other.test/mcp",
      secret: "s3",
    });
  });
});

describe("the catalog", () => {
  const { github, filesystem: files, slack } = fixtures.mcpCatalogEntries;

  test("an entry that runs a program needs local servers, one that signs in with OAuth is added by hand", () => {
    expect(entryBlock(github, fixtures.mcpCatalog)).toBeNull();
    expect(entryBlock(files, fixtures.mcpCatalog)).toBe(
      "Needs local servers: start the gateway with UF_MCP_LOCAL=on.",
    );
    expect(entryBlock(files, { ...fixtures.mcpCatalog, local_enabled: true })).toBeNull();
    expect(entryBlock({ ...github, auth: "oauth" }, fixtures.mcpCatalog)).toBe("Signs in with OAuth. Add it by hand.");
  });

  test("the request holds the entry and what it asks for, and a name only when it differs", () => {
    expect(catalogRequest(github, { name: "github", env_GITHUB_TOKEN: " tok " })).toEqual({
      catalog_id: "github",
      secrets: { GITHUB_TOKEN: " tok " },
    });
    expect(
      catalogRequest(slack, { name: " work ", env_SLACK_TOKEN: "a", env_SLACK_TEAM: "" }),
    ).toEqual({ catalog_id: "slack", secrets: { SLACK_TOKEN: "a" }, name: "work" });
  });

  test("a missing secret is refused on its field", () => {
    expect(() => catalogRequest(github, { name: "github", env_GITHUB_TOKEN: " " })).toThrow("Enter a value.");
  });

  test("the gateway's secrets.NAME is the form's env_NAME", () => {
    expect(formFieldsOf({ "secrets.A": "x", name: "y" })).toEqual({ env_A: "x", name: "y" });
  });

  test("says where the copy came from", () => {
    expect(sourceText(fixtures.mcpCatalog)).toBe("Showing the copy that ships with this gateway.");
    expect(sourceText({ ...fixtures.mcpCatalog, source: "remote", fetched_at: "2026-10-05T01:02:03Z" })).toBe(
      "Fetched from 2026-10-05.",
    );
    expect(sourceText({ ...fixtures.mcpCatalog, source: "cache", fetched_at: "2026-10-05T01:02:03Z" })).toBe(
      "Showing the copy from 2026-10-05.",
    );
  });

  test("the search needs every word", () => {
    const ids = (q: string) => searchCatalog(fixtures.mcpCatalog.entries, q).map((e) => e.id);
    expect(ids("")).toEqual(["github", "filesystem", "slack"]);
    expect(ids("issues code")).toEqual(["github"]);
    expect(ids("LOCAL files")).toEqual(["filesystem"]);
    expect(ids("zzz")).toEqual([]);
  });
});

describe("local servers", () => {
  const files = fixtures.mcpLocalServer;

  test("the arguments are the lines of the box", () => {
    expect(argsOf("-y\r\nfiles-mcp\n\n  spaced arg \n")).toEqual(["-y", "files-mcp", "  spaced arg "]);
    expect(argsOf("")).toEqual([]);
  });

  test("the environment is checked before it is sent", () => {
    const row = (name: string, value: string, stored = false) => ({ name, value, secret: true, stored });
    expect(envOf([row("A", "1"), row("B", "", true)])).toEqual({ A: "1", B: "" });
    expect(() => envOf([row(" ", "1")])).toThrow("Enter the name of the variable.");
    expect(() => envOf([row("A", "1"), row("A", "2")])).toThrow("A variable is listed twice.");
    expect(() => envOf([row("A", "")])).toThrow("Enter a value.");
  });

  test("a registration holds the program and what it needs", () => {
    expect(
      localCreateOf(
        { name: " files ", command: " npx ", args: "-y\nx", description: "d" },
        [{ name: "T", value: "v", secret: true, stored: false }],
      ),
    ).toEqual({ name: "files", transport: "stdio", command: "npx", args: ["-y", "x"], description: "d", env: { T: "v" } });
    expect(
      localCreateOf({ name: "f", command: "npx", args: "", description: "" }, []),
    ).toEqual({ name: "f", transport: "stdio", command: "npx", args: [], description: "" });
    expect(() => localCreateOf({ name: "f", command: " ", args: "", description: "" }, [])).toThrow(
      "Enter the program to run.",
    );
  });

  test("an edit sends only the difference", () => {
    const same = { command: "npx", args: "-y\nfiles-mcp", description: "" };
    const kept = [{ name: "API_TOKEN", value: "", secret: true, stored: true }];
    expect(localUpdateOf(files, same, kept)).toEqual({});
    expect(localUpdateOf(files, { ...same, command: "uvx" }, kept)).toEqual({ command: "uvx" });
    expect(localUpdateOf(files, { ...same, args: "-y" }, kept)).toEqual({ args: ["-y"] });
    expect(localUpdateOf(files, same, [{ ...kept[0], value: "new" }] as never)).toEqual({ env: { API_TOKEN: "new" } });
    // A variable taken away is a change of the set.
    expect(localUpdateOf(files, same, [])).toEqual({ env: {} });
  });

  test("how a program stands", () => {
    const at = (phase: string, restarts = 0) => statusOf({ ...files, local: { phase, restarts } });
    expect(at("crashlooping", 7)).toBe("Crash-looping (7 restarts)");
    expect(at("backoff", 1)).toBe("Restarting");
    expect(at("starting")).toBe("Starting");
    expect(at("off")).toBe("Local servers are off");
    expect(at("running")).toBe("Not synced yet");
    expect(statusOf({ ...files, enabled: false })).toBe("Disabled");
    const found = (status: string) =>
      statusOf({ ...fixtures.mcpServers.github, health: { status, latency_ms: null, checked_at: "" } });
    expect(found("down")).toBe("Down");
    expect(found("degraded")).toBe("Degraded");
    expect(found("crashlooping")).toBe("Crash-looping");
    expect(found("ok")).toBe("Synced");
  });
});

describe("the projection form", () => {
  test("lines are trimmed and empty ones left out", () => {
    expect(linesOf(" $.a \n\n$.b\r\n")).toEqual(["$.a", "$.b"]);
  });

  test("the values become a projection, limits as numbers or none", () => {
    expect(projectionOf({ keep: "$.a\n$.b", drop: "", max_items: " 5 ", max_bytes: "" })).toEqual({
      keep: ["$.a", "$.b"],
      drop: [],
      max_items: 5,
      max_bytes: null,
    });
    expect(() => projectionOf({ keep: "", drop: "", max_items: "x", max_bytes: "" })).toThrow(
      "Enter a whole number, or leave it empty.",
    );
    expect(() => projectionOf({ keep: "", drop: "", max_items: "", max_bytes: "-1" })).toThrow();
  });

  test("a tool's projection fills the form, and none leaves it empty", () => {
    expect(projectionValues({ projection: null })).toEqual({ keep: "", drop: "", max_items: "", max_bytes: "" });
    expect(
      projectionValues({ projection: { keep: ["$.a"], drop: ["$.b", "$.c"], max_items: 3, max_bytes: null } }),
    ).toEqual({ keep: "$.a", drop: "$.b\n$.c", max_items: "3", max_bytes: "" });
  });

  test("a sample must be an object", () => {
    expect(sampleOf('{"content": []}')).toEqual({ value: { content: [] } });
    expect("error" in sampleOf("[1]")).toBe(true);
    expect("error" in sampleOf("{")).toBe(true);
  });
});

describe("OAuth in the server form", () => {
  const was = { ...fixtures.mcpServers.github, auth_kind: "oauth", has_secret: true,
    oauth_token_url: "https://idp.example.test/token", oauth_client_id: "gw", oauth_scopes: "a", oauth_grant: "client_credentials" };
  const values: ServerValues = {
    url: was.url,
    description: was.description,
    auth_kind: "oauth",
    auth_header: "",
    secret: "",
    oauth_token_url: "https://idp.example.test/token",
    oauth_client_id: "gw",
    oauth_scopes: "a",
    oauth_grant: "client_credentials",
  };

  test("an edit sends only what changed and keeps the secret", () => {
    expect(updateOf(was, values)).toEqual({});
    expect(updateOf(was, { ...values, oauth_scopes: "a b", oauth_grant: "token_exchange" })).toEqual({
      oauth_scopes: "a b",
      oauth_grant: "token_exchange",
    });
    expect(updateOf(was, { ...values, oauth_scopes: "" })).toEqual({ oauth_scopes: "" });
  });

  test("another host for the token endpoint needs the secret again, before any request", () => {
    expect(() => updateOf(was, { ...values, oauth_token_url: "https://other.example.test/token" })).toThrow(
      HOST_CHANGED,
    );
    expect(
      updateOf(was, { ...values, oauth_token_url: "https://other.example.test/token", secret: "s" }),
    ).toEqual({ oauth_token_url: "https://other.example.test/token", secret: "s" });
  });

  test("a server that did not use OAuth needs everything, secret included", () => {
    const plain = fixtures.mcpServers.github;
    expect(() => updateOf(plain, { ...values, oauth_token_url: "" })).toThrow("Enter the address of the token endpoint.");
    expect(() => updateOf(plain, { ...values, oauth_client_id: " " })).toThrow("Enter the client id.");
    expect(() => updateOf(plain, values)).toThrow("Enter the secret.");
    expect(updateOf(plain, { ...values, secret: "s" })).toMatchObject({
      auth_kind: "oauth",
      oauth_token_url: "https://idp.example.test/token",
      oauth_client_id: "gw",
      secret: "s",
    });
  });
});
