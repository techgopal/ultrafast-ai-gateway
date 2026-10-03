import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import type { components } from "@/api/schema";
import { errors, fieldMessages, validationFailed, type GatewayError } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { noContent, ok, override, refuse } from "@/test/handlers";
import {
  choose,
  counted,
  descriptionOf,
  expectNotAvailable,
  expectOneH1,
  expectOneMain,
  forbid,
  forgetToasts,
  held,
  href,
  installSelect,
  NOT_FOUND,
  optionsOf,
  rowWithCell,
  SESSION_ENDED,
  settle,
  toasts,
} from "@/test/pages";
import { renderWithApp, type AppRenderResult } from "@/test/render";

type RouteRequest = components["schemas"]["RouteRequest"];

const { support } = fixtures.routes;

beforeAll(installSelect);
afterEach(forgetToasts);

type Options = { user?: fixtures.Me; width?: number };

function open(path: string, options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: path, ...options });
}

function viewOf(id: number, body: RouteRequest): fixtures.Route {
  const model = (modelId: number) => {
    const found = fixtures.modelList.find((m) => m.id === modelId);
    return {
      model: `${found?.provider_name ?? "?"}/${found?.name ?? "?"}`,
      enabled: found?.enabled ?? false,
    };
  };
  return {
    id,
    name: body.name,
    primaries: body.primaries.map((p) => ({ model_id: p.model_id, weight: p.weight, ...model(p.model_id) })),
    fallbacks: body.fallbacks.map((f) => ({ model_id: f, ...model(f) })),
    retries: body.retries,
    first_token_timeout_ms: body.first_token_timeout_ms,
    total_timeout_ms: body.total_timeout_ms,
    breaker_failures: body.breaker_failures,
    breaker_window_s: body.breaker_window_s,
    breaker_open_s: body.breaker_open_s,
    cache_enabled: body.cache_enabled ?? false,
    cache_ttl_s: body.cache_ttl_s ?? 300,
    cache_scope: body.cache_scope ?? "team",
    everyone: body.everyone,
    team_ids: body.team_ids,
    broken: false,
    created_at: "2026-09-30 09:00:00",
  };
}

/** A gateway that keeps the routes and changes them as it is told. */
function keeps(start: readonly fixtures.Route[] = fixtures.routeList) {
  const state = {
    routes: [...start],
    lists: 0,
    refuse: [] as GatewayError[],
    created: [] as RouteRequest[],
    updated: [] as { id: string | undefined; body: RouteRequest }[],
    deleted: [] as (string | undefined)[],
  };
  override("get", "/api/routes", () => {
    state.lists += 1;
    return ok("get", "/api/routes", 200, { routes: state.routes });
  });
  override("get", "/api/routes/{id}", ({ params }) => {
    const found = state.routes.find((r) => String(r.id) === params.id);
    return found === undefined
      ? refuse(errors.not_found)
      : ok("get", "/api/routes/{id}", 200, found);
  });
  override("post", "/api/routes", async ({ request }) => {
    const body = (await request.json()) as RouteRequest;
    state.created.push(body);
    const refusal = state.refuse.shift();
    if (refusal !== undefined) return refuse(refusal);
    const made = viewOf(9, body);
    state.routes = [...state.routes, made];
    return ok("post", "/api/routes", 201, made);
  });
  override("put", "/api/routes/{id}", async ({ request, params }) => {
    const body = (await request.json()) as RouteRequest;
    state.updated.push({ id: params.id, body });
    const refusal = state.refuse.shift();
    if (refusal !== undefined) return refuse(refusal);
    const was = state.routes.find((r) => String(r.id) === params.id);
    if (was === undefined) return refuse(errors.not_found);
    const now = viewOf(was.id, body);
    state.routes = state.routes.map((r) => (r.id === was.id ? now : r));
    return ok("put", "/api/routes/{id}", 200, now);
  });
  override("delete", "/api/routes/{id}", ({ params }) => {
    state.deleted.push(params.id);
    const refusal = state.refuse.shift();
    if (refusal !== undefined) return refuse(refusal);
    state.routes = state.routes.filter((r) => String(r.id) !== params.id);
    return noContent();
  });
  return state;
}

async function table(name: string): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

const routesTable = () => table("Routes");

function paste(field: HTMLElement, text: string): Promise<void> {
  return userEvent.click(field).then(() => userEvent.paste(text));
}

function sentAs(body: Partial<RouteRequest>): RouteRequest {
  return {
    name: "chat",
    everyone: false,
    primaries: [{ model_id: 1, weight: 1 }],
    fallbacks: [],
    retries: 2,
    first_token_timeout_ms: 30_000,
    total_timeout_ms: 300_000,
    breaker_failures: 5,
    breaker_window_s: 60,
    breaker_open_s: 30,
    team_ids: [],
    ...body,
  };
}

async function openedList(trigger: HTMLElement): Promise<HTMLElement> {
  await userEvent.click(trigger);
  return waitFor(() => {
    const list = document.querySelector<HTMLElement>('[role="listbox"]');
    if (list === null) throw new Error("the list of the select is not open");
    return list;
  });
}

const select = (name: string) => screen.getByRole("combobox", { name });
const button = (name: string) => screen.getByRole("button", { name });

async function editor(path: string): Promise<AppRenderResult> {
  const app = await open(path);
  await screen.findByRole("form", { name: "Route" });
  return app;
}

async function advanced(): Promise<void> {
  await userEvent.click(button("Advanced"));
}

describe("the list of routes", () => {
  test("it shows the name, the targets, the fallbacks, the teams and the status", async () => {
    keeps();
    await open("/routes");
    const found = await routesTable();
    expect(within(found).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Primary targets",
      "Fallbacks",
      "Teams",
      "Status",
      "Actions",
    ]);
    const cells = (name: string) =>
      within(rowWithCell(name)).getAllByRole("cell").map((cell) => cell.textContent);
    expect(cells("support-chat")).toEqual([
      "support-chat",
      "openai/gpt-4o-mini ×3openai/gpt-4o ×1",
      "1",
      "All teams",
      "Ready",
      "EditDelete",
    ]);
    expect(cells("research")).toEqual([
      "research",
      "openai/gpt-4o ×1",
      "0",
      "2 teams",
      "Ready",
      "EditDelete",
    ]);
    expect(cells("legacy.v1").slice(3, 5)).toEqual(["Admins only", "Broken"]);
    expect(rowWithCell("support-chat").querySelector("td")?.firstElementChild?.className).toContain(
      "font-mono",
    );
    expect(screen.getByRole("link", { name: "Add route" })).toHaveAttribute("href", "/routes/new");
    expect(
      within(rowWithCell("research")).getByRole("link", { name: "Edit" }),
    ).toHaveAttribute("href", "/routes/2");
    expectOneMain();
    expectOneH1("Routing");
  });

  test("with no route it says so", async () => {
    keeps([]);
    await open("/routes");
    expect(await screen.findByText("No routes")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Add route" })).toBeInTheDocument();
  });

  test("a failed call has one h1 and a way to retry", async () => {
    override("get", "/api/routes", () => refuse(errors.internal_error));
    await open("/routes");
    await screen.findByRole("alert");
    expectOneH1("Routing");
    expectOneMain();
    keeps();
    await userEvent.click(button("Retry"));
    await routesTable();
  });

  test("on a narrow screen the buttons are high enough to touch", async () => {
    keeps();
    await open("/routes", { width: 390 });
    await screen.findByRole("list", { name: "Routes" });
    for (const control of [
      ...screen.getAllByRole("link", { name: /Edit|Add route/ }),
      ...screen.getAllByRole("button", { name: "Delete" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
  });
});

describe("deleting a route", () => {
  test("the question says what happens, and the route is deleted and gone from the list", async () => {
    const state = keeps();
    await open("/routes");
    await routesTable();
    await userEvent.click(
      within(rowWithCell("support-chat")).getByRole("button", { name: "Delete" }),
    );
    const dialog = await screen.findByRole("alertdialog", { name: "Delete support-chat?" });
    expect(dialog).toHaveAccessibleDescription("Calls that use this route fail at once.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(state.deleted).toEqual(["1"]);
    expect(toasts()).toEqual(["Route deleted."]);
    await waitFor(() => {
      expect(screen.queryByRole("cell", { name: "support-chat" })).toBeNull();
    });
    // The list was asked for again: it is what the gateway says.
    expect(state.lists).toBe(2);
  });

  test("Cancel deletes nothing", async () => {
    const state = keeps();
    await open("/routes");
    await routesTable();
    await userEvent.click(within(rowWithCell("research")).getByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete research?" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(state.deleted).toEqual([]);
  });

  test("a refusal is said in the dialog, which stays", async () => {
    const state = keeps();
    state.refuse.push(errors.forbidden);
    await open("/routes");
    await routesTable();
    await userEvent.click(within(rowWithCell("research")).getByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete research?" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(await within(dialog).findByText(errors.forbidden.body.error.message)).toBeInTheDocument();
    expect(toasts()).toEqual([]);
  });
});

describe("the health of the targets", () => {
  test("every state, in words, with the counts and the last failure", async () => {
    keeps();
    await open("/routes");
    await routesTable();
    expect(await screen.findByRole("heading", { level: 2, name: "Target health" })).toBeInTheDocument();
    expect(
      screen.getByText("From real traffic only. The gateway does not send test requests."),
    ).toBeInTheDocument();
    const health = await table("Target health");
    expect(within(health).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Target",
      "State",
      "Successes",
      "Failures",
      "Last failure",
    ]);
    const cells = (name: string) =>
      within(rowWithCell(name)).getAllByRole("cell").map((cell) => cell.textContent);
    const times = (name: string) =>
      within(rowWithCell(name)).getAllByRole("cell").at(-1)?.querySelector("time")?.getAttribute("title");
    expect(cells("openai/gpt-4o-mini").slice(0, 4)).toEqual(["openai/gpt-4o-mini", "Healthy", "120", "2"]);
    expect(cells("openai/gpt-4o").slice(0, 4)).toEqual(["openai/gpt-4o", "Failing", "40", "9"]);
    expect(cells("local-llm/llama3.1:8b").slice(0, 4)).toEqual(["local-llm/llama3.1:8b", "Testing", "5", "5"]);
    expect(times("openai/gpt-4o")).toBe("2026-09-30 11:30:00 UTC");
    expect(cells("openai/gpt-4o")[4]).toContain("Status 429");
    // No failure: no time, and no status.
    expect(cells("openai/o3-mini")[4]).toBe("Never");
  });

  test("no call yet", async () => {
    keeps();
    override("get", "/api/routing/health", () => ok("get", "/api/routing/health", 200, { targets: [] }));
    await open("/routes");
    await routesTable();
    expect(await screen.findByText("No calls yet")).toBeInTheDocument();
  });

  test("health that cannot be read does not take the routes away", async () => {
    keeps();
    override("get", "/api/routing/health", () => refuse(errors.internal_error));
    await open("/routes");
    await routesTable();
    const part = await screen.findByRole("alert");
    expect(part).toHaveTextContent(errors.internal_error.body.error.message);
    expect(screen.getByRole("heading", { level: 2, name: "Target health" })).toBeInTheDocument();
    expectOneH1("Routing");
  });

  test("the page of a route shows the health of its targets only", async () => {
    keeps();
    await open("/routes/2");
    await screen.findByRole("form", { name: "Route" });
    const health = await table("Health of this route");
    expect(within(health).getAllByRole("row").slice(1).map((row) => row.querySelector("td")?.textContent)).toEqual([
      "openai/gpt-4o",
    ]);
    expect(
      screen.getByText("From real traffic only. The gateway does not send test requests."),
    ).toBeInTheDocument();
  });
});

describe("what a member sees", () => {
  function callable() {
    return counted("get", "/api/routes", () =>
      ok("get", "/api/routes", 200, { routes: fixtures.routesForMember }),
    );
  }

  test("the names of the routes they may use, with their models, read-only", async () => {
    callable();
    const health = forbid("/api/routing/health");
    await open("/routes", { user: fixtures.me.tomas });
    const list = await screen.findByRole("list", { name: "Routes you can use" });
    const items = within(list).getAllByRole("listitem", {}).filter((item) => item.parentElement === list);
    expect(items.map((item) => item.textContent.replace("Copy", ""))).toEqual([
      "researchopenai/gpt-4o",
      "support-chatopenai/gpt-4o-miniopenai/gpt-4olocal-llm/llama3.1:8b (fallback)",
    ]);
    expect(within(list).getByText("support-chat").className).toContain("font-mono");
    for (const name of ["Add route", "Edit", "Delete"]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
      expect(screen.queryByRole("link", { name })).toBeNull();
    }
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByText(/Ready|Broken|weight|Target health/)).toBeNull();
    expect(screen.queryByText(/×/)).toBeNull();
    await settle();
    expect(health.calls).toBe(0);
    expectOneMain();
    expectOneH1("Routing");
  });

  test("the copy button copies the name of the route", async () => {
    callable();
    const written: string[] = [];
    const before = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: (text: string) => Promise.resolve(void written.push(text)) },
    });
    try {
      await open("/routes", { user: fixtures.me.tomas });
      await userEvent.click(await screen.findByRole("button", { name: "Copy support-chat" }));
      await waitFor(() => {
        expect(written).toEqual(["support-chat"]);
      });
      expect(toasts()).toEqual(["Copied."]);
    } finally {
      if (before === undefined) Reflect.deleteProperty(navigator, "clipboard");
      else Object.defineProperty(navigator, "clipboard", before);
    }
  });

  test("a member with no route sees how to get one", async () => {
    override("get", "/api/routes", () => ok("get", "/api/routes", 200, { routes: [] }));
    await open("/routes", { user: fixtures.me.priya });
    expect(await screen.findByText("No routes are available to you yet. Ask an admin.")).toBeInTheDocument();
    expectOneMain();
    expectOneH1("Routing");
  });

  test("at width 390 the buttons are high enough to touch", async () => {
    callable();
    await open("/routes", { user: fixtures.me.tomas, width: 390 });
    const list = await screen.findByRole("list", { name: "Routes you can use" });
    for (const control of within(list).getAllByRole("button")) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
  });

  test("the pages that change routes are not available to them", async () => {
    callable();
    await open("/routes/new", { user: fixtures.me.tomas });
    await expectNotAvailable();
  });

  test("nor is the page of one route", async () => {
    const detail = counted("get", "/api/routes/{id}", () => refuse(errors.forbidden));
    await open("/routes/1", { user: fixtures.me.tomas });
    await expectNotAvailable();
    expect(detail.calls).toBe(0);
  });
});

describe("making a route", () => {
  test("every field is sent: name, targets, fallbacks, teams and settings", async () => {
    const state = keeps();
    const app = await editor("/routes/new");
    expectOneMain();
    expectOneH1("New route");
    await paste(screen.getByLabelText("Name"), "chat.v2");

    await userEvent.click(button("Add primary target"));
    await choose(select("Model of primary target 1"), "openai/gpt-4o-mini");
    const weight = screen.getByLabelText("Weight of primary target 1");
    await userEvent.clear(weight);
    await userEvent.type(weight, "3");
    await userEvent.click(button("Add primary target"));
    await choose(select("Model of primary target 2"), "openai/gpt-4o");

    await userEvent.click(button("Add fallback"));
    await choose(select("Model of fallback 1"), "local-llm/llama3.1:8b");

    await userEvent.click(screen.getByRole("radio", { name: "Chosen teams" }));
    const teams = await screen.findByRole("group", { name: "Chosen teams" });
    await within(teams).findByRole("checkbox", { name: "Platform" });
    await userEvent.click(within(teams).getByRole("checkbox", { name: "Platform" }));
    await userEvent.click(within(teams).getByRole("checkbox", { name: "Growth" }));

    await advanced();
    await userEvent.clear(screen.getByLabelText("Retries"));
    await userEvent.type(screen.getByLabelText("Retries"), "1");
    await userEvent.clear(screen.getByLabelText("First token timeout (s)"));
    await userEvent.type(screen.getByLabelText("First token timeout (s)"), "12.5");
    await userEvent.clear(screen.getByLabelText("Total timeout (s)"));
    await userEvent.type(screen.getByLabelText("Total timeout (s)"), "90");
    await userEvent.clear(screen.getByLabelText("Breaker failures"));
    await userEvent.type(screen.getByLabelText("Breaker failures"), "7");
    await userEvent.clear(screen.getByLabelText("Breaker window (s)"));
    await userEvent.type(screen.getByLabelText("Breaker window (s)"), "20");
    await userEvent.clear(screen.getByLabelText("Breaker open (s)"));
    await userEvent.type(screen.getByLabelText("Breaker open (s)"), "45");

    await userEvent.click(button("Create route"));
    await waitFor(() => {
      expect(href(app)).toBe("/routes");
    });
    expect(state.created).toEqual([
      {
        name: "chat.v2",
        everyone: false,
        primaries: [
          { model_id: 1, weight: 3 },
          { model_id: 2, weight: 1 },
        ],
        fallbacks: [4],
        retries: 1,
        first_token_timeout_ms: 12_500,
        total_timeout_ms: 90_000,
        breaker_failures: 7,
        breaker_window_s: 20,
        breaker_open_s: 45,
        team_ids: [1, 3],
      },
    ]);
    expect(toasts()).toEqual(["Route created."]);
    // The list shows it afterwards.
    expect(await screen.findByRole("cell", { name: "chat.v2" })).toBeInTheDocument();
    expect(state.lists).toBeGreaterThanOrEqual(1);
  });

  test("a new route starts as admins only, with the defaults shown and collapsed", async () => {
    const state = keeps();
    await editor("/routes/new");
    expect(screen.getByRole("radio", { name: "Admins only" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "All teams" })).not.toBeChecked();
    expect(button("Advanced")).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByLabelText("Retries")).not.toBeVisible();
    await advanced();
    expect(screen.getByLabelText("Retries")).toBeVisible();
    expect(button("Advanced")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByLabelText("Retries")).toHaveValue("2");
    expect(screen.getByLabelText("First token timeout (s)")).toHaveValue("30");
    expect(screen.getByLabelText("Total timeout (s)")).toHaveValue("300");
    expect(screen.getByLabelText("Breaker failures")).toHaveValue("5");
    expect(screen.getByLabelText("Breaker window (s)")).toHaveValue("60");
    expect(screen.getByLabelText("Breaker open (s)")).toHaveValue("30");
    expect(descriptionOf(screen.getByLabelText("Retries"))).toContain("Default 2");

    await paste(screen.getByLabelText("Name"), "chat");
    await userEvent.click(button("Add primary target"));
    await choose(select("Model of primary target 1"), "openai/gpt-4o-mini");
    await userEvent.click(button("Create route"));
    await waitFor(() => {
      expect(state.created).toHaveLength(1);
    });
    expect(state.created[0]).toEqual(sentAs({ everyone: false, team_ids: [] }));
  });

  test("all teams sends everyone and no team", async () => {
    const state = keeps();
    await editor("/routes/new");
    await paste(screen.getByLabelText("Name"), "chat");
    await userEvent.click(button("Add primary target"));
    await choose(select("Model of primary target 1"), "openai/gpt-4o-mini");
    await userEvent.click(screen.getByRole("radio", { name: "All teams" }));
    expect(screen.queryByRole("group", { name: "Chosen teams" })).toBeNull();
    await userEvent.click(button("Create route"));
    await waitFor(() => {
      expect(state.created).toHaveLength(1);
    });
    expect(state.created[0]).toEqual(sentAs({ everyone: true, team_ids: [] }));
  });

  test("only enabled models are offered, and a model of another row cannot be chosen again", async () => {
    keeps();
    await editor("/routes/new");
    await userEvent.click(button("Add primary target"));
    await userEvent.click(button("Add fallback"));
    const all = ["local-llm/llama3.1:8b", "openai/gpt-4o", "openai/gpt-4o-mini"];
    expect(await optionsOf(select("Model of primary target 1"))).toEqual(all);
    await choose(select("Model of primary target 1"), "openai/gpt-4o");
    const list = await openedList(select("Model of fallback 1"));
    expect(
      within(list)
        .getAllByRole("option")
        .map((option) => [option.textContent, option.getAttribute("aria-disabled") === "true"]),
    ).toEqual([
      ["local-llm/llama3.1:8b", false],
      ["openai/gpt-4o", true],
      ["openai/gpt-4o-mini", false],
    ]);
    await userEvent.keyboard("{Escape}");
    // The row that has the model can keep it.
    const own = await openedList(select("Model of primary target 1"));
    expect(
      within(own).getByRole("option", { name: "openai/gpt-4o" }).getAttribute("aria-disabled"),
    ).not.toBe("true");
    await userEvent.keyboard("{Escape}");
  });

  test("a row is removed", async () => {
    keeps();
    await editor("/routes/new");
    await userEvent.click(button("Add primary target"));
    await userEvent.click(button("Add primary target"));
    await userEvent.click(button("Remove primary target 1"));
    expect(screen.queryByRole("combobox", { name: "Model of primary target 2" })).toBeNull();
    expect(select("Model of primary target 1")).toBeInTheDocument();
    await userEvent.click(button("Add fallback"));
    await userEvent.click(button("Remove fallback 1"));
    expect(screen.queryByRole("combobox", { name: "Model of fallback 1" })).toBeNull();
  });

  test("Cancel goes back to the list and sends nothing", async () => {
    const state = keeps();
    const app = await editor("/routes/new");
    await userEvent.click(screen.getByRole("link", { name: "Cancel" }));
    await waitFor(() => {
      expect(href(app)).toBe("/routes");
    });
    expect(state.created).toEqual([]);
  });

  test("no model to choose from says so", async () => {
    keeps();
    override("get", "/api/models", () =>
      ok("get", "/api/models", 200, { models: [fixtures.models.openaiDisabled] }),
    );
    await editor("/routes/new");
    expect(
      screen.getByText("There are no models to choose from. Enable a model first."),
    ).toBeInTheDocument();
  });

  test("the models could not be read: it says so, with a way to retry", async () => {
    keeps();
    override("get", "/api/models", () => refuse(errors.internal_error));
    await open("/routes/new");
    await screen.findByRole("alert");
    expectOneH1("Routing");
    expectOneMain();
    expect(button("Retry")).toBeInTheDocument();
  });
});

describe("changing a route", () => {
  test("the form shows the route, and saving it sends the same request", async () => {
    const state = keeps();
    const app = await editor("/routes/2");
    expectOneH1("Edit route");
    expect(screen.getByLabelText("Name")).toHaveValue("research");
    expect(select("Model of primary target 1")).toHaveTextContent("openai/gpt-4o");
    expect(screen.getByLabelText("Weight of primary target 1")).toHaveValue("1");
    expect(screen.getByRole("radio", { name: "Chosen teams" })).toBeChecked();
    const teams = await screen.findByRole("group", { name: "Chosen teams" });
    await waitFor(() => {
      expect(within(teams).getByRole("checkbox", { name: "Platform" })).toBeChecked();
    });
    expect(within(teams).getByRole("checkbox", { name: "Research" })).toBeChecked();
    expect(within(teams).getByRole("checkbox", { name: "Growth" })).not.toBeChecked();
    await advanced();
    expect(screen.getByLabelText("Retries")).toHaveValue("0");
    expect(screen.getByLabelText("First token timeout (s)")).toHaveValue("10");
    expect(screen.getByLabelText("Total timeout (s)")).toHaveValue("120");

    await userEvent.click(button("Save route"));
    await waitFor(() => {
      expect(href(app)).toBe("/routes");
    });
    expect(state.updated).toEqual([
      {
        id: "2",
        body: sentAs({
          name: "research",
          primaries: [{ model_id: 2, weight: 1 }],
          retries: 0,
          first_token_timeout_ms: 10_000,
          total_timeout_ms: 120_000,
          breaker_failures: 3,
          breaker_window_s: 30,
          breaker_open_s: 15,
          team_ids: [1, 2],
        }),
      },
    ]);
    expect(toasts()).toEqual(["Route saved."]);
  });

  test("a change is sent and the list shows it afterwards", async () => {
    const state = keeps();
    await editor("/routes/1");
    await paste(screen.getByLabelText("Name"), "-2");
    const weight = screen.getByLabelText("Weight of primary target 2");
    await userEvent.clear(weight);
    await userEvent.type(weight, "5");
    await userEvent.click(button("Save route"));
    expect(await screen.findByRole("cell", { name: "support-chat-2" })).toBeInTheDocument();
    expect(state.updated).toHaveLength(1);
    expect(state.updated[0]?.body).toMatchObject({
      name: "support-chat-2",
      everyone: true,
      primaries: [
        { model_id: 1, weight: 3 },
        { model_id: 2, weight: 5 },
      ],
      fallbacks: [4],
      team_ids: [],
    });
    expect(within(rowWithCell("support-chat-2")).getByText(/gpt-4o ×5/)).toBeInTheDocument();
  });

  test("a fallback moves up and down by buttons, and the order is sent", async () => {
    const two: fixtures.Route = {
      ...support,
      fallbacks: [
        { model_id: 4, model: "local-llm/llama3.1:8b", enabled: true },
        { model_id: 3, model: "openai/o3-mini", enabled: false },
      ],
    };
    const state = keeps([two]);
    await editor("/routes/1");
    const order = () =>
      screen
        .getAllByRole("combobox", { name: /^Model of fallback/ })
        .map((one) => one.textContent);
    // A disabled model that the route has is shown as it is, and marked.
    expect(order()).toEqual(["local-llm/llama3.1:8b", "openai/o3-mini (disabled)"]);
    expect(button("Move fallback 1 up")).toBeDisabled();
    expect(button("Move fallback 2 down")).toBeDisabled();

    await userEvent.click(button("Move fallback 2 up"));
    expect(order()).toEqual(["openai/o3-mini (disabled)", "local-llm/llama3.1:8b"]);
    expect(button("Move fallback 1 up")).toBeDisabled();
    expect(button("Move fallback 2 down")).toBeDisabled();
    await userEvent.click(button("Move fallback 1 down"));
    expect(order()).toEqual(["local-llm/llama3.1:8b", "openai/o3-mini (disabled)"]);
    await userEvent.click(button("Move fallback 2 up"));
    await userEvent.click(button("Save route"));
    await waitFor(() => {
      expect(state.updated).toHaveLength(1);
    });
    expect(state.updated[0]?.body.fallbacks).toEqual([3, 4]);
  });

  test("a model that is no longer there is not shown as chosen, and is never sent", async () => {
    const gone: fixtures.Route = {
      ...support,
      primaries: [
        { model_id: 1, model: "openai/gpt-4o-mini", weight: 3, enabled: true },
        { model_id: 99, model: "openai/deleted", weight: 1, enabled: true },
      ],
    };
    const state = keeps([gone]);
    await editor("/routes/1");
    expect(select("Model of primary target 2")).toHaveTextContent("Choose a model");
    await userEvent.click(button("Save route"));
    expect(await screen.findByText("Choose a model.")).toBeInTheDocument();
    expect(select("Model of primary target 2")).toHaveAttribute("aria-invalid", "true");
    await settle();
    expect(state.updated).toEqual([]);
  });

  test("while the route and the models load, the page has its heading", async () => {
    keeps();
    const route = gate();
    override("get", "/api/routes/{id}", async () => {
      await route.opened;
      return ok("get", "/api/routes/{id}", 200, fixtures.routes.research);
    });
    await open("/routes/2");
    await screen.findByRole("status", { name: "Loading the route" });
    expectOneMain();
    expectOneH1("Edit route");
    act(() => {
      route.open();
    });
    await screen.findByRole("form", { name: "Route" });
    expectOneH1("Edit route");
  });

  test("while the models load for a new route, the page has its heading", async () => {
    keeps();
    const models = gate();
    override("get", "/api/models", async () => {
      await models.opened;
      return ok("get", "/api/models", 200, { models: fixtures.modelList });
    });
    await open("/routes/new");
    await screen.findByRole("status", { name: "Loading the models" });
    expectOneMain();
    expectOneH1("New route");
    act(() => {
      models.open();
    });
    await screen.findByRole("form", { name: "Route" });
    expectOneH1("New route");
  });

  test("a route that is not there is not found", async () => {
    keeps();
    await open("/routes/99");
    expect(await screen.findByText(NOT_FOUND)).toBeInTheDocument();
    expectOneH1(NOT_FOUND);
    expectOneMain();
  });

  test("an address that is no id does not ask the gateway", async () => {
    const one = counted("get", "/api/routes/{id}", () => refuse(errors.not_found));
    keeps();
    await open("/routes/abc");
    expect(await screen.findByText(NOT_FOUND)).toBeInTheDocument();
    expect(one.calls).toBe(0);
  });

  test("a route that was deleted meanwhile is not found: the page asks the gateway again", async () => {
    const state = keeps();
    await editor("/routes/2");
    state.routes = state.routes.filter((r) => r.id !== 2);
    await userEvent.click(button("Save route"));
    expect(await screen.findByText(NOT_FOUND)).toBeInTheDocument();
    expectOneH1(NOT_FOUND);
    expect(toasts()).toEqual([]);
    expect(state.updated).toHaveLength(1);
  });
});

describe("what is wrong with the form", () => {
  test("the console says it before anything is sent, on the field and on the row", async () => {
    const state = keeps();
    await editor("/routes/new");
    await paste(screen.getByLabelText("Name"), "Bad Name");
    await userEvent.click(button("Add primary target"));
    await userEvent.click(button("Add primary target"));
    await choose(select("Model of primary target 2"), "openai/gpt-4o");
    const weight = screen.getByLabelText("Weight of primary target 2");
    await userEvent.clear(weight);
    await userEvent.type(weight, "0");
    await userEvent.click(button("Add fallback"));
    await userEvent.click(button("Create route"));

    const name = screen.getByLabelText("Name");
    await waitFor(() => {
      expect(name).toHaveAttribute("aria-invalid", "true");
    });
    expect(descriptionOf(name)).toContain(
      "Use 1 to 64 characters: a-z, 0-9, '.', '_' and '-', starting with a letter or a digit.",
    );
    expect(select("Model of primary target 1")).toHaveAttribute("aria-invalid", "true");
    expect(descriptionOf(select("Model of primary target 1"))).toBe("Choose a model.");
    expect(screen.getByLabelText("Weight of primary target 2")).toHaveAttribute("aria-invalid", "true");
    expect(descriptionOf(screen.getByLabelText("Weight of primary target 2"))).toBe(
      "Enter a whole number from 1 to 1000.",
    );
    expect(descriptionOf(select("Model of fallback 1"))).toBe("Choose a model.");
    // The focus goes to the first field with a problem.
    expect(name).toHaveFocus();
    await settle();
    expect(state.created).toEqual([]);

    // It goes on saying what is wrong while it is mended.
    await userEvent.clear(name);
    await userEvent.click(name);
    await userEvent.paste("good");
    expect(name).not.toHaveAttribute("aria-invalid");
    await choose(select("Model of primary target 1"), "openai/gpt-4o-mini");
    expect(select("Model of primary target 1")).not.toHaveAttribute("aria-invalid");
  });

  test("a route needs a primary target, and chosen teams need a team", async () => {
    const state = keeps();
    await editor("/routes/new");
    await paste(screen.getByLabelText("Name"), "chat");
    await userEvent.click(screen.getByRole("radio", { name: "Chosen teams" }));
    await userEvent.click(button("Create route"));
    const group = await screen.findByRole("group", { name: "Primary targets" });
    await waitFor(() => {
      expect(descriptionOf(group)).toContain("Add at least one primary target.");
    });
    expect(descriptionOf(screen.getByRole("group", { name: "Chosen teams" }))).toContain(
      "Choose at least one team, or choose Admins only.",
    );
    expect(state.created).toEqual([]);
  });

  test("a setting out of its limits opens the advanced settings and says so", async () => {
    const state = keeps();
    await editor("/routes/1");
    await advanced();
    const total = screen.getByLabelText("Total timeout (s)");
    await userEvent.clear(total);
    await userEvent.type(total, "5");
    const first = screen.getByLabelText("First token timeout (s)");
    await userEvent.clear(first);
    await userEvent.type(first, "10");
    await userEvent.click(button("Advanced"));
    await userEvent.click(button("Save route"));
    expect(button("Advanced")).toHaveAttribute("aria-expanded", "true");
    await waitFor(() => {
      expect(descriptionOf(screen.getByLabelText("Total timeout (s)"))).toContain(
        "Must not be below the first token timeout.",
      );
    });
    expect(state.updated).toEqual([]);
  });

  test("what the gateway refuses is said on the field, in the words of the form", async () => {
    const state = keeps();
    state.refuse.push(
      validationFailed({
        primaries: fieldMessages.routeOnce,
        first_token_timeout_ms: fieldMessages.routeFirstToken,
        fallbacks: fieldMessages.routeModelMissing,
        team_ids: fieldMessages.routeTeamMissing,
      }),
    );
    await editor("/routes/1");
    await userEvent.click(button("Save route"));
    const group = screen.getByRole("group", { name: "Primary targets" });
    await waitFor(() => {
      expect(descriptionOf(group)).toContain(fieldMessages.routeOnce);
    });
    expect(descriptionOf(screen.getByRole("group", { name: "Fallbacks" }))).toContain(
      fieldMessages.routeModelMissing,
    );
    // The time is in seconds in the form; the gateway's range is in milliseconds.
    expect(button("Advanced")).toHaveAttribute("aria-expanded", "true");
    expect(descriptionOf(screen.getByLabelText("First token timeout (s)"))).toContain(
      "Enter seconds from 1 to 300.",
    );
    expect(descriptionOf(screen.getByLabelText("First token timeout (s)"))).not.toContain("1000");
    // `team_ids` is no field of this form while the route is not for chosen
    // teams: it is said in the alert at the top, not lost.
    expect(screen.queryByRole("group", { name: "Chosen teams" })).toBeNull();
    const note = screen.getByText(`Chosen teams: ${fieldMessages.routeTeamMissing}`);
    expect(note.closest('[role="alert"]')).not.toBeNull();
    expect(toasts()).toEqual([]);
    expect(state.updated).toHaveLength(1);
  });

  test("the advanced settings say the breaker is shared", async () => {
    keeps();
    await editor("/routes/1");
    await advanced();
    expect(
      screen.getByText(
        "Circuit breaker settings apply per provider model and are shared by every route that uses it.",
      ),
    ).toBeInTheDocument();
  });

  test("renaming a route says that keys listing it by name stop working", async () => {
    keeps();
    const hint = "Keys that list this route by name stop working when it is renamed.";
    await editor("/routes/1");
    const name = screen.getByLabelText("Name");
    expect(descriptionOf(name)).not.toContain(hint);
    await paste(name, "-2");
    expect(descriptionOf(name)).toContain(hint);
    await userEvent.clear(name);
    await paste(name, "support-chat");
    expect(descriptionOf(name)).not.toContain(hint);
  });

  test("a new route is not told it is renamed", async () => {
    keeps();
    await editor("/routes/new");
    await paste(screen.getByLabelText("Name"), "x");
    expect(descriptionOf(screen.getByLabelText("Name"))).not.toContain("stop working");
  });

  test("after a target row is removed the focus goes to the next row, or to Add", async () => {
    const two: fixtures.Route = {
      ...support,
      fallbacks: [
        { model_id: 4, model: "local-llm/llama3.1:8b", enabled: true },
        { model_id: 3, model: "openai/o3-mini", enabled: false },
      ],
    };
    keeps([two]);
    await editor("/routes/1");
    await userEvent.click(button("Remove fallback 1"));
    expect(select("Model of fallback 1")).toHaveFocus();
    await userEvent.click(button("Remove fallback 1"));
    expect(button("Add fallback")).toHaveFocus();
    await userEvent.click(button("Remove primary target 1"));
    expect(select("Model of primary target 1")).toHaveFocus();
    await userEvent.click(button("Remove primary target 1"));
    expect(button("Add primary target")).toHaveFocus();
  });

  test("a name that is taken is said on the name", async () => {
    const state = keeps();
    state.refuse.push(errors.route_exists);
    await editor("/routes/new");
    await paste(screen.getByLabelText("Name"), "research");
    await userEvent.click(button("Add primary target"));
    await choose(select("Model of primary target 1"), "openai/gpt-4o-mini");
    await userEvent.click(button("Create route"));
    const name = screen.getByLabelText("Name");
    await waitFor(() => {
      expect(descriptionOf(name)).toContain(errors.route_exists.body.error.message);
    });
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(name).toHaveFocus();
    expect(toasts()).toEqual([]);
  });

  test("the teams could not be read: it says so and cannot save", async () => {
    keeps();
    override("get", "/api/teams", () => refuse(errors.internal_error));
    await editor("/routes/new");
    await userEvent.click(screen.getByRole("radio", { name: "Chosen teams" }));
    expect(await screen.findByText(errors.internal_error.body.error.message)).toBeInTheDocument();
    expect(button("Create route")).toBeDisabled();
    await userEvent.click(screen.getByRole("radio", { name: "All teams" }));
    expect(button("Create route")).toBeEnabled();
  });
});

describe("saving", () => {
  test("while it runs one request is made, and the form can be sent again after a refusal", async () => {
    const request = held("put", "/api/routes/{id}");
    await editor("/routes/1");
    const form = screen.getByRole("form", { name: "Route" });
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    const saving = await screen.findByRole("button", { name: "Saving" });
    expect(saving).toBeDisabled();
    await userEvent.keyboard("{Enter}");
    expect(request.calls).toBe(1);
    request.answer();
    expect(await screen.findByText(errors.forbidden.body.error.message)).toBeInTheDocument();
    await settle();
    expect(request.calls).toBe(1);
    expect(button("Save route")).toBeEnabled();
    expect(toasts()).toEqual([]);
  });

  test("the session ends while it is saved: nothing is said", async () => {
    startGateway({ signedIn: true });
    const puts = counted("put", "/api/routes/{id}", () => refuse(errors.unauthenticated));
    const app = await editor("/routes/2");
    await userEvent.click(button("Save route"));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Froutes%2F2");
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(puts.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
  });

  test("the session ends while a route is made: nothing is said", async () => {
    startGateway({ signedIn: true });
    const posts = counted("post", "/api/routes", () => refuse(errors.unauthenticated));
    const app = await editor("/routes/new");
    await paste(screen.getByLabelText("Name"), "chat");
    await userEvent.click(button("Add primary target"));
    await choose(select("Model of primary target 1"), "openai/gpt-4o-mini");
    await userEvent.click(button("Create route"));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Froutes%2Fnew");
    });
    expect(posts.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
