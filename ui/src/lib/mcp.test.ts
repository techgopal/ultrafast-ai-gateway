import { describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import {
  hostChanged,
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
