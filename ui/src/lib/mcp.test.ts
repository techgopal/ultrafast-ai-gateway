import { describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import {
  catalogRequest,
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
