import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { errors } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { networkFailure, ok, override, refuse } from "@/test/handlers";
import {
  choose,
  counted,
  expectOneH1,
  expectOneMain,
  expectSessionEndsOnPage,
  forgetToasts,
  href,
  installSelect,
  optionsOf,
  settle,
  theClockIs,
  theWindowGetsTheFocus,
  toasts,
} from "@/test/pages";
import { gate } from "@/test/gateway";
import { renderWithApp, type AppRenderResult } from "@/test/render";
import { queryKeys } from "@/api/queries";

beforeAll(installSelect);
afterEach(forgetToasts);

const TABLE = "Request logs";
const LOAD_OLDER = "Load older";

type Options = { user?: fixtures.Me; width?: number; route?: string };

function page(options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/logs", ...options });
}

/** A gateway whose logs are these; it answers as `GET /api/logs` does. `asked` is every query. */
function logsAre(all: readonly fixtures.Log[]) {
  const state = { asked: [] as URLSearchParams[] };
  override("get", "/api/logs", ({ request }) => {
    const query = new URL(request.url).searchParams;
    state.asked.push(query);
    const limit = Number(query.get("limit") ?? 50);
    const before = query.get("before");
    const errorsOnly = query.get("errors") === "true";
    const logs = all
      .filter((row) => before === null || row.id < Number(before))
      .filter((row) => !errorsOnly || row.status >= 400)
      .slice(0, limit);
    return ok("get", "/api/logs", 200, { logs });
  });
  return state;
}

function manyLogs(newest: number, count: number): fixtures.Log[] {
  return Array.from({ length: count }, (_, index) => ({
    ...fixtures.logs.answered,
    id: newest - index,
    at: "2026-09-30 11:00:00",
  }));
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: TABLE });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

function rows(): string[][] {
  const found = screen.queryByRole("table", { name: TABLE });
  if (found === null) return [];
  return [...found.querySelectorAll("tbody tr")].map((row) =>
    [...row.querySelectorAll("td")].map((cell) => cell.textContent.trim()),
  );
}

const control = (name: string) => screen.getByRole("combobox", { name });
const queryControl = (name: string) => screen.queryByRole("combobox", { name });

describe("the list", () => {
  test("shows each call with its key, user, model, status, tokens, cost and duration", async () => {
    await page();
    await table();
    expect(screen.getByRole("heading", { level: 1, name: "Logs" })).toBeInTheDocument();
    const found = rows();
    expect(found).toHaveLength(5);
    // time, key, user, model, status, tokens, cost, duration, tags, endpoint, prompt
    expect(found[0]?.slice(1)).toEqual([
      "platform-prod",
      "arjun@example.test",
      "openai/gpt-4o",
      "200",
      "120 / 48",
      "$1.25",
      "850 ms",
      "No tags",
      "Chat completions",
      "—",
    ]);
    // The model that answered, or the name asked for when none did.
    expect(found[1]?.[3]).toBe("anthropic/claude-haiku");
    expect(found[3]?.[3]).toBe("chat-fast");
    expect(found[3]?.[4]).toBe("502");
    // No tokens; no key.
    expect(found[3]?.[5]).toBe("—");
    expect(found[4]?.[1]).toBe("—");
    // A cost under a cent; a cached answer is $0.00 and says so; unpriced says so.
    expect(found[1]?.[6]).toBe("<$0.01");
    expect(found[2]?.[4]).toContain("Cached");
    expect(found[2]?.[6]).toBe("$0.00");
    expect(found[4]?.[6]).toBe("Unpriced");
    expect(found[3]?.[6]).toBe("—");
    expect(found[0]?.[4]).not.toContain("Cached");
    // Each time leads to the detail.
    const first = within(await table()).getAllByRole("link")[0];
    expect(first).toHaveAttribute("href", "/logs/5");
    expect(toasts()).toEqual([]);
    expectOneMain();
    expectOneH1("Logs");
  });

  test("a call charged an estimate says so", async () => {
    logsAre([fixtures.estimatedLog, ...fixtures.logList]);
    await page();
    await table();
    const found = rows();
    expect(found[0]?.[4]).toContain("499");
    expect(found[0]?.[4]).toContain("Estimated");
    expect(found[1]?.[4]).not.toContain("Estimated");
    expect(found[0]?.[6]).toBe("<$0.01");
  });

  test("asks for the last 24 hours, newest page of 50", async () => {
    theClockIs(fixtures.now);
    const asked = logsAre(fixtures.logList);
    await page();
    await table();
    expect(asked.asked).toHaveLength(1);
    const query = asked.asked[0];
    expect(Object.fromEntries(query?.entries() ?? [])).toEqual({
      limit: "50",
      from: "2026-09-29T12:00:00Z",
    });
  });

  test("auto-refresh is off: the focus asks nothing, and Refresh starts again with one request", async () => {
    theClockIs(fixtures.now);
    const asked = logsAre(fixtures.logList);
    await page();
    await table();
    theWindowGetsTheFocus();
    await settle();
    expect(asked.asked).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => {
      expect(asked.asked).toHaveLength(2);
    });
    await table();
    await settle();
    expect(asked.asked).toHaveLength(2);
    expect(asked.asked[1]?.has("before")).toBe(false);
  });

  test("an empty list says so", async () => {
    logsAre([]);
    await page();
    expect(await screen.findByText("No calls")).toBeInTheDocument();
    expectOneMain();
    expectOneH1("Logs");
  });
});

describe("the cursor", () => {
  test("Load older asks below the last id and stops when a page is short", async () => {
    const asked = logsAre(manyLogs(120, 70));
    await page();
    await waitFor(() => {
      expect(rows()).toHaveLength(50);
    });
    await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
    await waitFor(() => {
      expect(rows()).toHaveLength(70);
    });
    expect(asked.asked[1]?.get("before")).toBe("71");
    expect(screen.queryByRole("button", { name: LOAD_OLDER })).toBeNull();
  });

  test("older calls that cannot be loaded: what is loaded stays, Retry asks again", async () => {
    const all = manyLogs(120, 70);
    logsAre(all);
    await page();
    await waitFor(() => {
      expect(rows()).toHaveLength(50);
    });
    override("get", "/api/logs", () => refuse(errors.internal_error));
    await userEvent.click(screen.getByRole("button", { name: LOAD_OLDER }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(rows()).toHaveLength(50);
    logsAre(all);
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => {
      expect(rows()).toHaveLength(70);
    });
  });
});

describe("the filters", () => {
  test("the time range: presets and a custom range", async () => {
    theClockIs(fixtures.now);
    const asked = logsAre(fixtures.logList);
    await page();
    await table();
    expect(await optionsOf(control("Time range"))).toEqual([
      "Last hour",
      "Last 24 hours",
      "Last 7 days",
      "Last 30 days",
      "Custom range",
    ]);
    await choose(control("Time range"), "Last hour");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("from")).toBe("2026-09-30T11:00:00Z");
    });
    await choose(control("Time range"), "Last 7 days");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("from")).toBe("2026-09-23T12:00:00Z");
    });
    await choose(control("Time range"), "Last 30 days");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("from")).toBe("2026-08-31T12:00:00Z");
    });
    await choose(control("Time range"), "Custom range");
    await userEvent.type(screen.getByLabelText("From (UTC)"), "2026-09-01");
    await userEvent.type(screen.getByLabelText("To (UTC)"), "2026-09-15");
    await waitFor(() => {
      const last = asked.asked.at(-1);
      expect([last?.get("from"), last?.get("to")]).toEqual(["2026-09-01", "2026-09-15"]);
    });
  });

  test("key, user, team and model narrow the request, and are never sent when not chosen", async () => {
    const asked = logsAre(fixtures.logList);
    await page();
    await table();
    const first = asked.asked[0];
    for (const name of ["key_id", "user_id", "team_id", "model", "status", "to"]) {
      expect(first?.has(name), name).toBe(false);
    }
    await choose(control("Key"), "platform-prod");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("key_id")).toBe(String(fixtures.keys.active.id));
    });
    await choose(control("User"), fixtures.users.arjun.email);
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("user_id")).toBe(String(fixtures.users.arjun.id));
    });
    await choose(control("Team"), fixtures.teams.platform.name);
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("team_id")).toBe(String(fixtures.teams.platform.id));
    });
    await userEvent.type(screen.getByRole("textbox", { name: "Model" }), "gpt-4o{Enter}");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("model")).toBe("gpt-4o");
    });
    // Back to "all": the parameter is gone again.
    await choose(control("Key"), "All keys");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.has("key_id")).toBe(false);
    });
  });

  test("the endpoint narrows the request, and is never sent when not chosen", async () => {
    const asked = logsAre(fixtures.logList);
    await page();
    await table();
    expect(asked.asked[0]?.has("endpoint")).toBe(false);
    expect(await optionsOf(control("Endpoint"))).toEqual([
      "All endpoints",
      "Chat completions",
      "Messages",
      "Responses",
      "Embeddings",
      "Images",
      "Transcriptions",
      "Translations",
      "Speech",
      "Playground",
    ]);
    await choose(control("Endpoint"), "Images");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("endpoint")).toBe("images");
    });
    await choose(control("Endpoint"), "Responses");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("endpoint")).toBe("responses");
    });
    await choose(control("Endpoint"), "All endpoints");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.has("endpoint")).toBe(false);
    });
  });

  test("the endpoint filter is there for a member too", async () => {
    logsAre(fixtures.logList);
    await page({ user: fixtures.me.tomas });
    await table();
    expect(control("Endpoint")).toBeInTheDocument();
  });

  test("an endpoint a call has is shown by its name; one this console does not know as it is", async () => {
    logsAre([
      { ...fixtures.logs.answered, id: 11, endpoint: "speech" },
      { ...fixtures.logs.answered, id: 10, endpoint: "somethingnew" },
    ]);
    await page();
    await table();
    expect(rows().map((cells) => cells[9])).toEqual(["Speech", "somethingnew"]);
  });

  test("the prompt a call used is shown as name@version", async () => {
    logsAre([fixtures.promptedLog, ...fixtures.logList]);
    await page();
    await table();
    const found = rows();
    expect(found[0]?.[10]).toBe("summarize@3");
    expect(found[1]?.[10]).toBe("—");
  });

  test("errors only asks the API for errors, and shows what it answers", async () => {
    const asked = logsAre(fixtures.logList);
    await page();
    await table();
    expect(asked.asked[0]?.has("errors")).toBe(false);
    expect(await optionsOf(control("Status"))).toEqual(["All statuses", "Errors only"]);
    await choose(control("Status"), "Errors only");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.get("errors")).toBe("true");
    });
    await waitFor(() => {
      expect(rows().map((cells) => cells[4])).toEqual(["502"]);
    });
    await choose(control("Status"), "All statuses");
    await waitFor(() => {
      expect(rows()).toHaveLength(5);
    });
    expect(asked.asked.at(-1)?.has("errors")).toBe(false);
  });

  test("errors only finds errors beyond the first page and says nothing about loaded calls", async () => {
    // 70 calls without an error, then 3 errors older than the first 50.
    const all = [
      ...manyLogs(120, 70),
      ...Array.from({ length: 3 }, (_, index) => ({
        ...fixtures.logs.answered,
        id: 40 - index,
        status: 502,
      })),
    ];
    logsAre(all);
    await page();
    await waitFor(() => {
      expect(rows()).toHaveLength(50);
    });
    await choose(control("Status"), "Errors only");
    await waitFor(() => {
      expect(rows()).toHaveLength(3);
    });
    expect(screen.queryByText(/loaded calls only/)).toBeNull();
  });
});

describe("scope", () => {
  test("a member is offered no filter by key, user or team, and asks for no list of them", async () => {
    const keys = counted("get", "/api/keys", () => ok("get", "/api/keys", 200, { keys: [] }));
    const users = counted("get", "/api/users", () => ok("get", "/api/users", 200, { users: [] }));
    await page({ user: fixtures.me.tomas });
    await table();
    for (const name of ["Key", "User", "Team"]) expect(queryControl(name)).toBeNull();
    expect(control("Time range")).toBeInTheDocument();
    expect(control("Status")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Model" })).toBeInTheDocument();
    expect([keys.calls, users.calls]).toEqual([0, 0]);
  });

  test("a lead and an admin are offered them", async () => {
    for (const user of [fixtures.me.arjun, fixtures.me.maya]) {
      const app = await page({ user });
      await table();
      for (const name of ["Key", "User", "Team"]) {
        await waitFor(() => {
          expect(control(name)).toBeInTheDocument();
        });
      }
      app.unmount();
    }
  });
});

describe("states", () => {
  test("a failed list shows the error with Retry; one main and one h1", async () => {
    override("get", "/api/logs", () => refuse(errors.internal_error));
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expectOneMain();
    expectOneH1();
  });

  test("a gateway that cannot be reached", async () => {
    override("get", "/api/logs", networkFailure);
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
  });

  test("a 403 is not available", async () => {
    override("get", "/api/logs", () => refuse(errors.forbidden));
    await page();
    expect(await screen.findByRole("heading", { name: "Not available" })).toBeInTheDocument();
    expectOneH1("Not available");
  });

  test("loading: one main and one h1", async () => {
    await page();
    expectOneMain();
    expectOneH1("Logs");
    await table();
  });

  test("the session ends on the page", async () => {
    const app = await page();
    await table();
    await expectSessionEndsOnPage(app, {
      path: "/api/logs",
      queryKey: queryKeys.logs.all(),
      at: "/logs",
    });
  });

  test("at width 390 the rows are cards and the controls are high enough to touch", async () => {
    await page({ width: 390 });
    const cards = await screen.findByRole("list", { name: TABLE });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(5);
    expect([...(items[0]?.querySelectorAll("dt") ?? [])].map((dt) => dt.textContent)).toEqual([
      "Time",
      "Key",
      "User",
      "Model",
      "Status",
      "Tokens",
      "Cost",
      "Duration",
      "Tags",
      "Endpoint",
      "Prompt",
    ]);
    const buttons = [
      screen.getByRole("button", { name: "Refresh" }),
      control("Time range"),
      control("Status"),
      screen.getByRole("textbox", { name: "Model" }),
      screen.getByRole("textbox", { name: "Tag" }),
    ];
    for (const one of buttons) expect(one.className.split(/\s+/)).toContain("min-h-11");
    expect(within(items[0] ?? cards).getAllByRole("link")[0]?.className).toContain("min-h-11");
  });
});

describe("the detail", () => {
  test("shows every field and the routing attempts in order, with their outcomes", async () => {
    await page({ route: "/logs/4" });
    const details = await screen.findByLabelText("Details");
    expect(screen.getByRole("heading", { level: 1, name: "Call" })).toBeInTheDocument();
    const text = details.textContent;
    for (const part of [
      "chat-fast",
      "Chat completions",
      "anthropic",
      "claude-haiku",
      "platform-prod",
      "arjun@example.test",
      "Platform",
      "<$0.01",
      "2.4 s",
    ]) {
      expect(text).toContain(part);
    }
    const attempts = screen.getByRole("table", { name: "Routing attempts" });
    const found = [...attempts.querySelectorAll("tbody tr")].map((row) =>
      [...row.querySelectorAll("td")].map((cell) => cell.textContent.trim()),
    );
    expect(found).toEqual([
      ["openai/gpt-4o-mini", "Circuit open", "—", "0 ms"],
      ["openai/gpt-4o", "Retried", "503", "900 ms"],
      ["anthropic/claude-haiku", "Answered", "200", "1.5 s"],
    ]);
    expect(screen.getByRole("link", { name: "Back to logs" })).toHaveAttribute("href", "/logs");
    expectOneMain();
    expectOneH1("Call");
  });

  test("outcomes Failed, Skipped and Cached", async () => {
    await page({ route: "/logs/2" });
    const attempts = await screen.findByRole("table", { name: "Routing attempts" });
    expect(attempts).toHaveTextContent("Failed");
    expect(attempts).toHaveTextContent("Skipped");
    expect(within(attempts).getByText("Failed")).toHaveAttribute("data-slot", "badge");
  });

  test("a skipped attempt shows why it was passed over", async () => {
    await page({ route: "/logs/2" });
    const attempts = await screen.findByRole("table", { name: "Routing attempts" });
    const reason = within(attempts).getByText("skipped: unsupported:audio");
    expect(reason).toHaveAttribute("data-slot", "badge");
    expect(within(attempts).getAllByText(/^skipped:/)).toHaveLength(1);
  });

  test("the detail names the prompt template and version, or none", async () => {
    override("get", "/api/logs/{id}", () =>
      ok("get", "/api/logs/{id}", 200, { ...fixtures.promptedLog, attempts: [] }),
    );
    await page({ route: "/logs/12" });
    const details = await screen.findByLabelText("Details");
    expect(within(details).getByText("Prompt").nextElementSibling).toHaveTextContent("summarize@3");
  });

  test("a cached call says so", async () => {
    await page({ route: "/logs/3" });
    const attempts = await screen.findByRole("table", { name: "Routing attempts" });
    expect(within(attempts).getByText("Cached")).toBeInTheDocument();
    expect(screen.getByLabelText("Details")).toHaveTextContent("$0.00");
  });

  test("a call charged an estimate says so", async () => {
    override("get", "/api/logs/{id}", () =>
      ok("get", "/api/logs/{id}", 200, { ...fixtures.estimatedLog, attempts: [] }),
    );
    await page({ route: "/logs/6" });
    expect(await screen.findByLabelText("Details")).toHaveTextContent("Estimated");
  });

  test("while the call loads, the page has its heading", async () => {
    const door = gate();
    override("get", "/api/logs/{id}", async () => {
      await door.opened;
      return ok("get", "/api/logs/{id}", 200, { ...fixtures.logs.answered, attempts: [] });
    });
    await page({ route: "/logs/5" });
    await screen.findByRole("status", { name: "Loading the call" });
    expectOneMain();
    expectOneH1("Call");
    act(() => {
      door.open();
    });
    expect(await screen.findByLabelText("Details")).toBeInTheDocument();
    expectOneH1("Call");
  });

  test("a call that is not there, or not the viewer's, is not found", async () => {
    await page({ route: "/logs/999" });
    expect(await screen.findByText("Page not found")).toBeInTheDocument();
    expectOneMain();
    expectOneH1("Page not found");
  });

  test("a call that has no attempts says so", async () => {
    override("get", "/api/logs/{id}", () =>
      ok("get", "/api/logs/{id}", 200, { ...fixtures.logs.answered, attempts: [] }),
    );
    await page({ route: "/logs/5" });
    expect(await screen.findByText("No attempts were recorded.")).toBeInTheDocument();
  });

  test("the time in the list leads to the detail", async () => {
    const app = await page();
    const found = await table();
    await userEvent.click(within(found).getAllByRole("link")[1] as HTMLElement);
    expect(await screen.findByRole("heading", { level: 1, name: "Call" })).toBeInTheDocument();
    expect(href(app)).toBe("/logs/4");
    await act(async () => {
      await Promise.resolve();
    });
  });

  test("a failed detail shows the error with Retry", async () => {
    override("get", "/api/logs/{id}", () => refuse(errors.internal_error));
    await page({ route: "/logs/5" });
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expectOneH1();
  });
});
