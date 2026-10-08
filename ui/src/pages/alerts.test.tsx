import type { QueryClient } from "@tanstack/react-query";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { errors, fieldMessages, validationFailed, type GatewayError } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { startGateway } from "@/test/gateway";
import { noContent, ok, override, refuse } from "@/test/handlers";
import {
  cached,
  choose,
  clientThatKeepsDataFresh,
  counted,
  expectLabelsNameControls,
  expectNoSecret,
  expectNotAvailable,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  href,
  installSelect,
  optionsOf,
  rowWithCell,
  SESSION_ENDED,
  settle,
  shown,
  toasts,
} from "@/test/pages";
import { memberUser, renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

const { alertRules, alertEvents } = fixtures;

const SECRET_SHOWN_ONCE = "Copy this signing secret now. It is not shown again.";
/** The path of a webhook URL is what makes it secret: Slack puts the token there. */
const HOOK_URL = "https://hooks.example.test/services/T000/B000/XXXXtokenXXXX";
const HOOK_TOKEN = "XXXXtokenXXXX";

type Options = { user?: fixtures.Me; width?: number; queryClient?: QueryClient };

function page(view: "" | "channels" | "history" = "", options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: view === "" ? "/alerts" : `/alerts#${view}`, ...options });
}

function readBody(body: unknown): Record<string, unknown> {
  return typeof body === "object" && body !== null ? Object.fromEntries(Object.entries(body)) : {};
}

/** The last 50 of a number of events, newest first. */
function manyEvents(count: number): fixtures.AlertEvent[] {
  return Array.from({ length: count }, (_, index) => ({
    ...alertEvents.firing,
    id: 1000 - index,
    summary: `Event number ${String(1000 - index)}`,
  }));
}

/** A gateway that keeps the channels, the rules and the events, and changes them as it is told. */
function keeps(
  start: {
    channels?: fixtures.AlertChannel[];
    rules?: fixtures.AlertRule[];
    events?: fixtures.AlertEvent[];
  } = {},
) {
  const state = {
    channels: [...(start.channels ?? fixtures.alertChannelList)],
    rules: [...(start.rules ?? fixtures.alertRuleList)],
    events: [...(start.events ?? fixtures.alertEventList)],
    lists: { channels: 0, rules: 0, events: 0 },
    eventQueries: [] as URLSearchParams[],
    createdChannels: [] as Record<string, unknown>[],
    createdRules: [] as Record<string, unknown>[],
    patchedChannels: [] as { id: string | undefined; body: Record<string, unknown> }[],
    patchedRules: [] as { id: string | undefined; body: Record<string, unknown> }[],
    deletedChannels: [] as (string | undefined)[],
    deletedRules: [] as (string | undefined)[],
    rotated: [] as (string | undefined)[],
    tested: [] as (string | undefined)[],
    testResult: { ok: true, status: 200, error: null } as {
      ok: boolean;
      status: number | null;
      error: string | null;
    },
    refuseChannelCreate: [] as GatewayError[],
    refuseRuleCreate: [] as GatewayError[],
  };
  override("get", "/api/alerts/channels", () => {
    state.lists.channels += 1;
    return ok("get", "/api/alerts/channels", 200, { channels: state.channels });
  });
  override("get", "/api/alerts/rules", () => {
    state.lists.rules += 1;
    return ok("get", "/api/alerts/rules", 200, { rules: state.rules });
  });
  override("get", "/api/alerts/events", ({ request }) => {
    state.lists.events += 1;
    const query = new URL(request.url).searchParams;
    state.eventQueries.push(query);
    const rule = query.get("rule_id");
    const before = query.get("before_id");
    const wanted = query.get("state");
    const limit = Number(query.get("limit") ?? 50);
    const events = state.events
      .filter((event) => rule === null || String(event.rule_id) === rule)
      .filter((event) => wanted === null || event.state === wanted)
      .filter((event) => before === null || event.id < Number(before))
      .sort((a, b) => b.id - a.id)
      .slice(0, limit);
    return ok("get", "/api/alerts/events", 200, { events });
  });
  override("post", "/api/alerts/channels", async ({ request }) => {
    const body = readBody(await request.json());
    state.createdChannels.push(body);
    const refusal = state.refuseChannelCreate.shift();
    if (refusal !== undefined) return refuse(refusal);
    const channel: fixtures.AlertChannel = {
      id: 9,
      name: String(body.name),
      kind: String(body.kind),
      url_host: "https://hooks.example.test",
      enabled: true,
      created_at: "2026-09-30 11:00:00",
      rules: [],
    };
    state.channels = [...state.channels, channel];
    return ok("post", "/api/alerts/channels", 201, { channel, secret: fixtures.newChannelSecret });
  });
  override("patch", "/api/alerts/channels/{id}", async ({ request, params }) => {
    const body = readBody(await request.json());
    state.patchedChannels.push({ id: params.id, body });
    const channel = state.channels.find((one) => String(one.id) === params.id);
    if (channel === undefined) return refuse(errors.not_found);
    const next: fixtures.AlertChannel = {
      ...channel,
      ...(typeof body.name === "string" ? { name: body.name } : {}),
      ...(typeof body.enabled === "boolean" ? { enabled: body.enabled } : {}),
      ...(typeof body.url === "string" ? { url_host: "https://hooks.example.test" } : {}),
    };
    state.channels = state.channels.map((one) => (one.id === next.id ? next : one));
    return ok("patch", "/api/alerts/channels/{id}", 200, next);
  });
  override("delete", "/api/alerts/channels/{id}", ({ params }) => {
    state.deletedChannels.push(params.id);
    state.channels = state.channels.filter((one) => String(one.id) !== params.id);
    return noContent();
  });
  override("post", "/api/alerts/channels/{id}/rotate-secret", ({ params }) => {
    state.rotated.push(params.id);
    return ok("post", "/api/alerts/channels/{id}/rotate-secret", 200, {
      secret: fixtures.rotatedChannelSecret,
    });
  });
  override("post", "/api/alerts/channels/{id}/test", ({ params }) => {
    state.tested.push(params.id);
    return ok("post", "/api/alerts/channels/{id}/test", 200, state.testResult);
  });
  override("post", "/api/alerts/rules", async ({ request }) => {
    const body = readBody(await request.json());
    state.createdRules.push(body);
    const refusal = state.refuseRuleCreate.shift();
    if (refusal !== undefined) return refuse(refusal);
    const ids = Array.isArray(body.channel_ids) ? body.channel_ids : [];
    const rule: fixtures.AlertRule = {
      id: 9,
      name: String(body.name),
      kind: String(body.kind),
      enabled: true,
      created_at: "2026-09-30 11:00:00",
      params: fixtures.freeForm(readBody(body.params)),
      channels: state.channels
        .filter((one) => ids.includes(one.id))
        .map((one) => ({ id: one.id, name: one.name })),
      firing: [],
    };
    state.rules = [...state.rules, rule];
    return ok("post", "/api/alerts/rules", 201, rule);
  });
  override("patch", "/api/alerts/rules/{id}", async ({ request, params }) => {
    const body = readBody(await request.json());
    state.patchedRules.push({ id: params.id, body });
    const rule = state.rules.find((one) => String(one.id) === params.id);
    if (rule === undefined) return refuse(errors.not_found);
    const next: fixtures.AlertRule = {
      ...rule,
      ...(typeof body.name === "string" ? { name: body.name } : {}),
      ...(typeof body.enabled === "boolean" ? { enabled: body.enabled } : {}),
      ...(body.params === undefined ? {} : { params: fixtures.freeForm(readBody(body.params)) }),
    };
    state.rules = state.rules.map((one) => (one.id === next.id ? next : one));
    return ok("patch", "/api/alerts/rules/{id}", 200, next);
  });
  override("delete", "/api/alerts/rules/{id}", ({ params }) => {
    state.deletedRules.push(params.id);
    state.rules = state.rules.filter((one) => String(one.id) !== params.id);
    return noContent();
  });
  return state;
}

async function table(name: "Alert rules" | "Alert channels" | "Alert history"): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

function names(scope: HTMLElement): (string | null)[] {
  return within(scope)
    .queryAllByRole("button")
    .map((control) => control.textContent);
}

async function dialogOf(name: string): Promise<HTMLElement> {
  return screen.findByRole("dialog", { name });
}

/** The form of a dialog is there once the lists it offers have arrived. */
async function formOf(name: string): Promise<HTMLElement> {
  const dialog = await dialogOf(name);
  await within(dialog).findByLabelText("Name");
  return dialog;
}

async function paste(field: HTMLElement, text: string): Promise<void> {
  await userEvent.click(field);
  await userEvent.paste(text);
}

async function openAddRule(): Promise<HTMLElement> {
  await userEvent.click(await screen.findByRole("button", { name: "Add rule" }));
  return formOf("Add rule");
}

async function openEditRule(name: string): Promise<HTMLElement> {
  await table("Alert rules");
  await userEvent.click(within(rowWithCell(name)).getByRole("button", { name: "Edit" }));
  return formOf("Edit rule");
}

async function openAddChannel(): Promise<HTMLElement> {
  await userEvent.click(await screen.findByRole("button", { name: "Add channel" }));
  return formOf("Add channel");
}

async function openEditChannel(name: string): Promise<HTMLElement> {
  await table("Alert channels");
  await userEvent.click(within(rowWithCell(name)).getByRole("button", { name: "Edit" }));
  return formOf("Edit channel");
}

function combo(scope: HTMLElement, name: string): HTMLElement {
  return within(scope).getByRole("combobox", { name });
}

function submit(dialog: HTMLElement, name: string): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name }));
}

async function closeSecret(): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: "Done" }));
  const question = await screen.findByRole("alertdialog");
  await userEvent.click(within(question).getByRole("button", { name: "Close" }));
  await waitFor(() => {
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

describe("the page", () => {
  test("has one main and one h1, and three views", async () => {
    keeps();
    const app = await page();
    await table("Alert rules");
    expectOneMain();
    expectOneH1("Alerts");
    const nav = screen.getByRole("navigation", { name: "Alerts sections" });
    expect(within(nav).getAllByRole("link").map((link) => link.textContent)).toEqual([
      "Rules",
      "Channels",
      "History",
    ]);
    expect(within(nav).getByRole("link", { name: "Rules" })).toHaveAttribute("aria-current", "page");
    expect(within(nav).getAllByRole("link").filter((link) => link.hasAttribute("aria-current"))).toHaveLength(1);

    await userEvent.click(within(nav).getByRole("link", { name: "Channels" }));
    await table("Alert channels");
    expect(href(app)).toBe("/alerts#channels");
    expect(within(nav).getByRole("link", { name: "Channels" })).toHaveAttribute("aria-current", "page");
    expectOneH1("Alerts");

    await userEvent.click(within(nav).getByRole("link", { name: "History" }));
    await table("Alert history");
    expect(href(app)).toBe("/alerts#history");
    expectOneH1("Alerts");
    expectOneMain();
  });

  test("opens at the view the address names", async () => {
    keeps();
    await page("history");
    await table("Alert history");
    expect(screen.queryByRole("table", { name: "Alert rules" })).toBeNull();
  });

  test("is not available to a member, who also asks for nothing", async () => {
    const rules = counted("get", "/api/alerts/rules", () => refuse(errors.forbidden));
    const events = counted("get", "/api/alerts/events", () => refuse(errors.forbidden));
    await page("", { user: memberUser });
    await expectNotAvailable();
    expectOneMain();
    await settle();
    expect(rules.calls).toBe(0);
    expect(events.calls).toBe(0);
  });

  test("a list that fails shows the error with Retry", async () => {
    let fail = true;
    override("get", "/api/alerts/rules", () =>
      fail
        ? refuse(errors.internal_error)
        : ok("get", "/api/alerts/rules", 200, { rules: fixtures.alertRuleList }),
    );
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    fail = false;
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await table("Alert rules");
    expectOneH1("Alerts");
  });

  test("the session ends while the rules are open", async () => {
    keeps();
    const app = await page();
    await table("Alert rules");
    override("get", "/api/alerts/rules", unauthenticated);
    override("get", "/api/auth/me", unauthenticated);
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: ["alerts"] });
    });
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/alerts")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
  });
});

describe("rules", () => {
  test("lists each rule with its condition, its channels and whether it fires", async () => {
    keeps();
    await page();
    const list = await table("Alert rules");
    expect(within(list).getAllByRole("columnheader").map((head) => head.textContent)).toEqual([
      "Name",
      "Kind",
      "Condition",
      "Channels",
      "Status",
      "Enabled",
      "Actions",
    ]);
    const budget = rowWithCell("Search budget 80%");
    expect(budget).toHaveTextContent("Budget");
    expect(budget).toHaveTextContent("Budget 'team Platform weekly' at 80%");
    expect(budget).toHaveTextContent("ops-webhook, team-slack");
    expect(within(budget).getByText("Firing")).toBeInTheDocument();
    expect(budget).toHaveTextContent("team Platform weekly from 2026-09-28");
    expect(within(budget).getByRole("switch", { name: "Search budget 80%" })).toBeChecked();

    const errorsRow = rowWithCell("Chat errors");
    expect(errorsRow).toHaveTextContent("Error rate");
    expect(errorsRow).toHaveTextContent("Errors ≥ 10% over 5 min on route support-chat");
    expect(within(errorsRow).getByText("OK")).toBeInTheDocument();

    const circuit = rowWithCell("Circuit anywhere");
    expect(circuit).toHaveTextContent("Circuit opens on any target");
    expect(within(circuit).getByRole("switch", { name: "Circuit anywhere" })).not.toBeChecked();
    expect(names(circuit)).toEqual(["Edit", "Delete"]);
  });

  test("says so when there are none", async () => {
    keeps({ rules: [] });
    await page();
    expect(await screen.findByText("No alert rules")).toBeInTheDocument();
    expect(
      screen.getByText("Add a rule to be told when a budget, an error rate or a circuit needs attention."),
    ).toBeInTheDocument();
    expectOneH1("Alerts");
  });

  test("the switch turns a rule off and on, and sends only that", async () => {
    const state = keeps();
    await page();
    await table("Alert rules");
    await userEvent.click(within(rowWithCell("Chat errors")).getByRole("switch", { name: "Chat errors" }));
    await waitFor(() => {
      expect(state.patchedRules).toEqual([{ id: "2", body: { enabled: false } }]);
    });
    await waitFor(() => {
      expect(within(rowWithCell("Chat errors")).getByRole("switch")).not.toBeChecked();
    });
    await userEvent.click(within(rowWithCell("Circuit anywhere")).getByRole("switch"));
    await waitFor(() => {
      expect(state.patchedRules.at(-1)).toEqual({ id: "3", body: { enabled: true } });
    });
  });

  test("a switch that fails says why", async () => {
    keeps();
    override("patch", "/api/alerts/rules/{id}", () => refuse(errors.internal_error));
    await page();
    await table("Alert rules");
    await userEvent.click(within(rowWithCell("Chat errors")).getByRole("switch"));
    expect(await screen.findByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    expect(within(rowWithCell("Chat errors")).getByRole("switch")).toBeChecked();
  });

  test("adds a budget rule, and the list has it afterwards", async () => {
    const state = keeps();
    await page("", { queryClient: clientThatKeepsDataFresh() });
    await table("Alert rules");
    const lists = state.lists.rules;
    const dialog = await openAddRule();
    expectLabelsNameControls(dialog);
    expect(within(dialog).getByRole("radio", { name: "Budget" })).toBeChecked();
    await paste(within(dialog).getByLabelText("Name"), "Gateway spend");
    await choose(combo(dialog, "Budget"), "gateway monthly");
    const percent = within(dialog).getByLabelText("Percent of the budget");
    expect(percent).toHaveValue("80");
    await userEvent.clear(percent);
    await paste(percent, "90");
    await userEvent.click(within(dialog).getByRole("checkbox", { name: "team-slack" }));
    await submit(dialog, "Add rule");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(state.createdRules).toEqual([
      {
        name: "Gateway spend",
        kind: "budget",
        params: { budget_id: 1, percent: 90 },
        channel_ids: [2],
      },
    ]);
    expect(toasts()).toEqual(["Rule added."]);
    expect(await screen.findByRole("cell", { name: "Gateway spend" })).toBeInTheDocument();
    expect(state.lists.rules).toBe(lists + 1);
  });

  test("a budget rule can be for any budget", async () => {
    const state = keeps();
    await page();
    const dialog = await openAddRule();
    expect(combo(dialog, "Budget")).toHaveTextContent("Any budget");
    expect(await optionsOf(combo(dialog, "Budget"))).toEqual([
      "Any budget",
      "gateway monthly",
      "team Platform weekly",
      `key ${fixtures.keys.active.name} daily`,
    ]);
    await paste(within(dialog).getByLabelText("Name"), "Any budget");
    await submit(dialog, "Add rule");
    await waitFor(() => {
      expect(state.createdRules).toHaveLength(1);
    });
    expect(state.createdRules[0]).toEqual({
      name: "Any budget",
      kind: "budget",
      params: { budget_id: null, percent: 80 },
      channel_ids: [],
    });
  });

  test("adds an error-rate rule, with the help about when it resolves", async () => {
    const state = keeps();
    await page();
    const dialog = await openAddRule();
    await userEvent.click(within(dialog).getByRole("radio", { name: "Error rate" }));
    await paste(within(dialog).getByLabelText("Name"), "Provider errors");
    await choose(combo(dialog, "Scope"), "Provider");
    await choose(combo(dialog, "Provider"), "openai");
    expect(within(dialog).getByLabelText("Error rate (percent)")).toHaveValue("10");
    await userEvent.clear(within(dialog).getByLabelText("Error rate (percent)"));
    await paste(within(dialog).getByLabelText("Error rate (percent)"), "25");
    await userEvent.clear(within(dialog).getByLabelText("Window (minutes)"));
    await paste(within(dialog).getByLabelText("Window (minutes)"), "15");
    expect(within(dialog).getByLabelText("Minimum calls in the window")).toHaveValue("20");
    expect(dialog).toHaveTextContent(
      "It resolves after the rate stays below the threshold for a full window. That can take up to about two windows after the last error.",
    );
    await submit(dialog, "Add rule");
    await waitFor(() => {
      expect(state.createdRules).toHaveLength(1);
    });
    expect(state.createdRules[0]).toEqual({
      name: "Provider errors",
      kind: "error_rate",
      params: {
        scope: "provider",
        subject: "openai",
        percent: 25,
        window_minutes: 15,
        min_requests: 20,
      },
      channel_ids: [],
    });
  });

  test("the scope gateway has no subject, and a key is chosen by name", async () => {
    const state = keeps();
    await page();
    const dialog = await openAddRule();
    await userEvent.click(within(dialog).getByRole("radio", { name: "Error rate" }));
    await paste(within(dialog).getByLabelText("Name"), "Key errors");
    await choose(combo(dialog, "Scope"), "Gateway");
    expect(within(dialog).queryByRole("combobox", { name: /^(Route|Provider|Key)$/ })).toBeNull();
    await choose(combo(dialog, "Scope"), "Key");
    await choose(combo(dialog, "Key"), fixtures.keys.active.name);
    await userEvent.clear(within(dialog).getByLabelText("Error rate (percent)"));
    await paste(within(dialog).getByLabelText("Error rate (percent)"), "50");
    await submit(dialog, "Add rule");
    await waitFor(() => {
      expect(state.createdRules).toHaveLength(1);
    });
    expect(state.createdRules[0]).toMatchObject({
      params: { scope: "key", subject: String(fixtures.keys.active.id), percent: 50 },
    });
  });

  test("adds a circuit rule for a provider", async () => {
    const state = keeps();
    await page();
    const dialog = await openAddRule();
    await userEvent.click(within(dialog).getByRole("radio", { name: "Circuit" }));
    await paste(within(dialog).getByLabelText("Name"), "OpenAI circuit");
    await choose(combo(dialog, "Provider"), "openai");
    expect(await optionsOf(combo(dialog, "Model"))).toEqual(["Any model", "gpt-4o-mini", "gpt-4o", "o3-mini"]);
    await submit(dialog, "Add rule");
    await waitFor(() => {
      expect(state.createdRules).toHaveLength(1);
    });
    expect(state.createdRules[0]).toEqual({
      name: "OpenAI circuit",
      kind: "circuit_open",
      params: { provider: "openai", model: null },
      channel_ids: [],
    });
  });

  test("a percent that cannot be right is said on its field and sends nothing", async () => {
    const state = keeps();
    await page();
    const dialog = await openAddRule();
    await paste(within(dialog).getByLabelText("Name"), "Too much");
    const percent = within(dialog).getByLabelText("Percent of the budget");
    await userEvent.clear(percent);
    await paste(percent, "150");
    await submit(dialog, "Add rule");
    expect(await within(dialog).findByText("Enter a whole number from 1 to 100.")).toBeInTheDocument();
    expect(percent).toHaveAttribute("aria-invalid", "true");
    expect(percent).toHaveFocus();
    expect(state.createdRules).toEqual([]);
  });

  test("a name that is taken is said on the name, and the form stays", async () => {
    const state = keeps();
    state.refuseRuleCreate.push(errors.alert_rule_exists);
    await page();
    const dialog = await openAddRule();
    await paste(within(dialog).getByLabelText("Name"), "Chat errors");
    await submit(dialog, "Add rule");
    const name = within(dialog).getByLabelText("Name");
    await waitFor(() => {
      expect(name).toHaveAttribute("aria-invalid", "true");
    });
    expect(within(dialog).getByText(errors.alert_rule_exists.body.error.message)).toBeInTheDocument();
    expect(name).toHaveValue("Chat errors");
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });

  test("an answer about the parameters is said by the form", async () => {
    const state = keeps();
    state.refuseRuleCreate.push(validationFailed({ params: fieldMessages.budgetMissing }));
    await page();
    const dialog = await openAddRule();
    await paste(within(dialog).getByLabelText("Name"), "Gone budget");
    await submit(dialog, "Add rule");
    expect(await within(dialog).findByText(`params: ${fieldMessages.budgetMissing}`)).toBeInTheDocument();
  });

  test("edits a rule: the kind is shown and cannot be changed, and only the change is sent", async () => {
    const state = keeps();
    await page();
    const dialog = await openEditRule("Search budget 80%");
    expect(within(dialog).queryByRole("radio")).toBeNull();
    expect(dialog).toHaveTextContent("Kind");
    expect(within(dialog).getByLabelText("Name")).toHaveValue("Search budget 80%");
    expect(combo(dialog, "Budget")).toHaveTextContent("team Platform weekly");
    expect(within(dialog).getByLabelText("Percent of the budget")).toHaveValue("80");
    expect(within(dialog).getByRole("checkbox", { name: "ops-webhook" })).toBeChecked();
    expect(within(dialog).getByRole("checkbox", { name: "team-slack" })).toBeChecked();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.clear(name);
    await paste(name, "Platform budget");
    await submit(dialog, "Save rule");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(state.patchedRules).toEqual([{ id: "1", body: { name: "Platform budget" } }]);
    expect(toasts()).toEqual(["Rule updated."]);
    expect(await screen.findByRole("cell", { name: "Platform budget" })).toBeInTheDocument();
  });

  test("new parameters and channels are sent, and nothing else", async () => {
    const state = keeps();
    await page();
    const dialog = await openEditRule("Chat errors");
    const percent = within(dialog).getByLabelText("Error rate (percent)");
    await userEvent.clear(percent);
    await paste(percent, "30");
    await userEvent.click(within(dialog).getByRole("checkbox", { name: "team-slack" }));
    await submit(dialog, "Save rule");
    await waitFor(() => {
      expect(state.patchedRules).toHaveLength(1);
    });
    expect(state.patchedRules[0]).toEqual({
      id: "2",
      body: {
        params: { scope: "route", subject: "support-chat", percent: 30, window_minutes: 5, min_requests: 20 },
        channel_ids: [1, 2],
      },
    });
  });

  test("a save that changes nothing sends nothing", async () => {
    const state = keeps();
    await page();
    const dialog = await openEditRule("Chat errors");
    await submit(dialog, "Save rule");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(state.patchedRules).toEqual([]);
    expect(toasts()).toEqual([]);
  });

  test("a route that is gone is not offered, and is not sent as it was", async () => {
    const state = keeps({
      rules: [
        {
          ...alertRules.errors,
          params: fixtures.freeForm({ ...alertRules.errors.params, subject: "route-that-was-deleted" }),
        },
      ],
    });
    await page();
    const dialog = await openEditRule("Chat errors");
    expect(combo(dialog, "Route")).toHaveTextContent("Each route");
    await submit(dialog, "Save rule");
    await waitFor(() => {
      expect(state.patchedRules).toHaveLength(1);
    });
    expect(state.patchedRules[0]?.body).toMatchObject({ params: { subject: null } });
  });

  test("deletes a rule after asking, and says its events stay", async () => {
    const state = keeps();
    await page("", { queryClient: clientThatKeepsDataFresh() });
    await table("Alert rules");
    await userEvent.click(within(rowWithCell("Chat errors")).getByRole("button", { name: "Delete" }));
    const question = await screen.findByRole("alertdialog", { name: "Delete Chat errors?" });
    expect(question).toHaveTextContent("It stops watching at once. Its past events stay in History.");
    await userEvent.click(within(question).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(state.deletedRules).toEqual(["2"]);
    expect(toasts()).toEqual(["Rule deleted."]);
    await waitFor(() => {
      expect(screen.queryByRole("cell", { name: "Chat errors" })).toBeNull();
    });
  });

  test("the session ends while a rule is added: signed out, and the form says nothing", async () => {
    keeps();
    startGateway({ signedIn: true });
    counted("post", "/api/alerts/rules", unauthenticated);
    const app = await page();
    const dialog = await openAddRule();
    await paste(within(dialog).getByLabelText("Name"), "Late rule");
    await submit(dialog, "Add rule");
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/alerts")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  });

  test("at 390 px the rules are cards, the form fits and its controls can be touched", async () => {
    keeps();
    await page("", { width: 390 });
    const cards = await screen.findByRole("list", { name: "Alert rules" });
    expect(cards.children).toHaveLength(3);
    expectOneH1("Alerts");
    for (const link of within(screen.getByRole("navigation", { name: "Alerts sections" })).getAllByRole("link")) {
      expect(link.className.split(/\s+/)).toContain("min-h-11");
    }
    const dialog = await openAddRule();
    for (const control of [
      within(dialog).getByLabelText("Name"),
      combo(dialog, "Budget"),
      within(dialog).getByLabelText("Percent of the budget"),
      within(dialog).getByRole("button", { name: "Add rule" }),
      within(dialog).getByRole("button", { name: "Cancel" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
  });
});

describe("channels", () => {
  test("lists each channel with its host and the rules that use it", async () => {
    keeps();
    await page("channels");
    const list = await table("Alert channels");
    expect(within(list).getAllByRole("columnheader").map((head) => head.textContent)).toEqual([
      "Name",
      "Kind",
      "Host",
      "Rules",
      "Enabled",
      "Actions",
    ]);
    const ops = rowWithCell("ops-webhook");
    expect(ops).toHaveTextContent("Webhook");
    expect(ops).toHaveTextContent("https://hooks.example.test");
    expect(ops).toHaveTextContent("Search budget 80%, Chat errors");
    expect(within(ops).getByRole("switch", { name: "ops-webhook" })).toBeChecked();
    expect(names(ops)).toEqual(["Send test", "Edit", "Rotate secret", "Delete"]);
    expect(rowWithCell("team-slack")).toHaveTextContent("Slack");
    expect(shown()).not.toContain("hooks.example.test/");
  });

  test("a channel from a file has no URL: it says so and cannot be enabled", async () => {
    const state = keeps();
    await page("channels");
    await table("Alert channels");
    const row = rowWithCell("imported-pager");
    expect(within(row).getByText("Needs a URL")).toBeInTheDocument();
    const toggle = within(row).getByRole("switch", { name: "imported-pager" });
    expect(toggle).toBeDisabled();
    expect(toggle).not.toBeChecked();
    await userEvent.click(toggle);
    expect(state.patchedChannels).toEqual([]);

    const dialog = await openEditChannel("imported-pager");
    expect(dialog).toHaveTextContent("This channel has no URL yet. Set one to be able to enable it.");
    await paste(within(dialog).getByLabelText("URL"), HOOK_URL);
    await submit(dialog, "Save channel");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(state.patchedChannels).toEqual([{ id: "3", body: { url: HOOK_URL } }]);
    await waitFor(() => {
      expect(within(rowWithCell("imported-pager")).queryByText("Needs a URL")).toBeNull();
    });
    expect(within(rowWithCell("imported-pager")).getByRole("switch")).toBeEnabled();
  });

  test("the switch of a channel sends only the change", async () => {
    const state = keeps();
    await page("channels");
    await table("Alert channels");
    await userEvent.click(within(rowWithCell("team-slack")).getByRole("switch"));
    await waitFor(() => {
      expect(state.patchedChannels).toEqual([{ id: "2", body: { enabled: false } }]);
    });
  });

  test("says so when there are none", async () => {
    keeps({ channels: [] });
    await page("channels");
    expect(await screen.findByText("No channels")).toBeInTheDocument();
    expect(screen.getByText("Add a channel to receive alerts.")).toBeInTheDocument();
  });

  test("adds a channel: the signing secret is shown once, the URL is nowhere afterwards", async () => {
    const state = keeps();
    const app = await page("channels", { queryClient: clientThatKeepsDataFresh() });
    await table("Alert channels");
    const dialog = await openAddChannel();
    expectLabelsNameControls(dialog);
    expect(within(dialog).getByRole("radio", { name: "Webhook" })).toBeChecked();
    await paste(within(dialog).getByLabelText("Name"), "pager");
    await paste(within(dialog).getByLabelText("URL"), HOOK_URL);
    await submit(dialog, "Add channel");

    const secret = await screen.findByRole("dialog", { name: "Signing secret" });
    expect(secret).toHaveTextContent(SECRET_SHOWN_ONCE);
    expect(within(secret).getByLabelText("Signing secret")).toHaveValue(fixtures.newChannelSecret);
    expect(within(secret).getByRole("button", { name: "Copy" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Add channel" })).toBeNull();
    expect(state.createdChannels).toEqual([{ name: "pager", kind: "webhook", url: HOOK_URL }]);
    expect(cached(app.queryClient)).not.toContain(fixtures.newChannelSecret);
    expect(cached(app.queryClient)).not.toContain(HOOK_TOKEN);
    expect(shown()).not.toContain(HOOK_TOKEN);
    expect(toasts()).toEqual([]);

    await closeSecret();
    expectNoSecret(app, fixtures.newChannelSecret);
    expectNoSecret(app, HOOK_TOKEN);
    expect(await screen.findByRole("cell", { name: "pager" })).toBeInTheDocument();
  });

  test("a Slack channel is sent as Slack", async () => {
    const state = keeps();
    await page("channels");
    const dialog = await openAddChannel();
    await userEvent.click(within(dialog).getByRole("radio", { name: "Slack" }));
    await paste(within(dialog).getByLabelText("Name"), "slack-two");
    await paste(within(dialog).getByLabelText("URL"), HOOK_URL);
    await submit(dialog, "Add channel");
    await screen.findByRole("dialog", { name: "Signing secret" });
    expect(state.createdChannels).toEqual([{ name: "slack-two", kind: "slack", url: HOOK_URL }]);
  });

  test("a refusal keeps the typed URL in its field; closing clears it", async () => {
    const state = keeps();
    state.refuseChannelCreate.push(errors.alert_channel_exists);
    const app = await page("channels");
    const dialog = await openAddChannel();
    await paste(within(dialog).getByLabelText("Name"), "ops-webhook");
    await paste(within(dialog).getByLabelText("URL"), HOOK_URL);
    await submit(dialog, "Add channel");
    await waitFor(() => {
      expect(within(dialog).getByLabelText("Name")).toHaveAttribute("aria-invalid", "true");
    });
    expect(within(dialog).getByText(errors.alert_channel_exists.body.error.message)).toBeInTheDocument();
    expect(within(dialog).getByLabelText("URL")).toHaveValue(HOOK_URL);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expectNoSecret(app, HOOK_TOKEN);
    const again = await openAddChannel();
    expect(within(again).getByLabelText("URL")).toHaveValue("");
  });

  test("a URL the gateway refuses is said on the URL", async () => {
    const state = keeps();
    state.refuseChannelCreate.push(validationFailed({ url: fieldMessages.channelUrlScheme }));
    await page("channels");
    const dialog = await openAddChannel();
    await paste(within(dialog).getByLabelText("Name"), "bad");
    await paste(within(dialog).getByLabelText("URL"), "ftp://example.test/x");
    await submit(dialog, "Add channel");
    await waitFor(() => {
      expect(within(dialog).getByLabelText("URL")).toHaveAttribute("aria-invalid", "true");
    });
    expect(within(dialog).getByText(fieldMessages.channelUrlScheme)).toBeInTheDocument();
  });

  test("types for real into the form of a channel", async () => {
    const state = keeps();
    await page("channels");
    const dialog = await openAddChannel();
    await userEvent.type(within(dialog).getByLabelText("Name"), "typed");
    await userEvent.type(within(dialog).getByLabelText("URL"), "https://typed.example.test/hook");
    await userEvent.keyboard("{Enter}");
    await screen.findByRole("dialog", { name: "Signing secret" });
    expect(state.createdChannels).toEqual([
      { name: "typed", kind: "webhook", url: "https://typed.example.test/hook" },
    ]);
  });

  test("edits a channel: only the name goes, and the URL is not shown", async () => {
    const state = keeps();
    await page("channels");
    const dialog = await openEditChannel("ops-webhook");
    expect(within(dialog).getByLabelText("Name")).toHaveValue("ops-webhook");
    expect(within(dialog).getByLabelText("URL")).toHaveValue("");
    expect(dialog).toHaveTextContent("Leave empty to keep the current URL. The gateway never shows it.");
    const name = within(dialog).getByLabelText("Name");
    await userEvent.clear(name);
    await paste(name, "ops-hook");
    await submit(dialog, "Save channel");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(state.patchedChannels).toEqual([{ id: "1", body: { name: "ops-hook" } }]);
    expect(toasts()).toEqual(["Channel updated."]);
  });

  describe("send test", () => {
    test("says that the receiver answered", async () => {
      const state = keeps();
      await page("channels");
      await table("Alert channels");
      await userEvent.click(within(rowWithCell("ops-webhook")).getByRole("button", { name: "Send test" }));
      expect(await screen.findByText("Test to ops-webhook: the receiver answered 200.")).toBeInTheDocument();
      expect(state.tested).toEqual(["1"]);
      expect(toasts()).toEqual([]);
    });

    test("says why it failed", async () => {
      const state = keeps();
      state.testResult = { ok: false, status: 500, error: "the receiver answered 500" };
      await page("channels");
      await table("Alert channels");
      await userEvent.click(within(rowWithCell("team-slack")).getByRole("button", { name: "Send test" }));
      expect(
        await screen.findByText("Test to team-slack failed: the receiver answered 500."),
      ).toBeInTheDocument();
    });

    test("says it when the gateway could not make the test", async () => {
      keeps();
      override("post", "/api/alerts/channels/{id}/test", () => refuse(errors.internal_error));
      await page("channels");
      await table("Alert channels");
      await userEvent.click(within(rowWithCell("ops-webhook")).getByRole("button", { name: "Send test" }));
      expect(await screen.findByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    });

    test("the result goes when it is dismissed, and the history has the test afterwards", async () => {
      const state = keeps();
      await page("channels", { queryClient: clientThatKeepsDataFresh() });
      await table("Alert channels");
      await userEvent.click(within(rowWithCell("ops-webhook")).getByRole("button", { name: "Send test" }));
      await screen.findByText("Test to ops-webhook: the receiver answered 200.");
      await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
      expect(screen.queryByText(/^Test to/)).toBeNull();
      const before = state.lists.events;
      await userEvent.click(within(screen.getByRole("navigation", { name: "Alerts sections" })).getByRole("link", { name: "History" }));
      await table("Alert history");
      expect(state.lists.events).toBeGreaterThan(before);
    });
  });

  test("rotates the secret after asking, and shows the new one once", async () => {
    const state = keeps();
    const app = await page("channels");
    await table("Alert channels");
    await userEvent.click(within(rowWithCell("ops-webhook")).getByRole("button", { name: "Rotate secret" }));
    const question = await screen.findByRole("alertdialog", { name: "Rotate the secret of ops-webhook?" });
    expect(question).toHaveTextContent("The old secret stops working at once. Update the receiver with the new one.");
    expect(state.rotated).toEqual([]);
    await userEvent.click(within(question).getByRole("button", { name: "Rotate secret" }));
    const secret = await screen.findByRole("dialog", { name: "New signing secret" });
    expect(state.rotated).toEqual(["1"]);
    expect(secret).toHaveTextContent(SECRET_SHOWN_ONCE);
    expect(within(secret).getByLabelText("New signing secret")).toHaveValue(fixtures.rotatedChannelSecret);
    expect(cached(app.queryClient)).not.toContain(fixtures.rotatedChannelSecret);
    await closeSecret();
    expectNoSecret(app, fixtures.rotatedChannelSecret);
  });

  test("Cancel in the question rotates nothing", async () => {
    const state = keeps();
    await page("channels");
    await table("Alert channels");
    await userEvent.click(within(rowWithCell("ops-webhook")).getByRole("button", { name: "Rotate secret" }));
    const question = await screen.findByRole("alertdialog");
    await userEvent.click(within(question).getByRole("button", { name: "Cancel" }));
    await settle();
    expect(state.rotated).toEqual([]);
  });

  test("delete asks first and lists the rules that send to the channel", async () => {
    const state = keeps();
    await page("channels", { queryClient: clientThatKeepsDataFresh() });
    await table("Alert channels");
    await userEvent.click(within(rowWithCell("ops-webhook")).getByRole("button", { name: "Delete" }));
    const question = await screen.findByRole("alertdialog", { name: "Delete ops-webhook?" });
    expect(question).toHaveTextContent(
      "These rules stop sending here: Search budget 80%, Chat errors.",
    );
    expect(state.deletedChannels).toEqual([]);
    await userEvent.click(within(question).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(state.deletedChannels).toEqual(["1"]);
    expect(toasts()).toEqual(["Channel deleted."]);
    await waitFor(() => {
      expect(screen.queryByRole("cell", { name: "ops-webhook" })).toBeNull();
    });
  });

  test("delete of a channel no rule uses says so", async () => {
    keeps();
    await page("channels");
    await table("Alert channels");
    await userEvent.click(within(rowWithCell("imported-pager")).getByRole("button", { name: "Delete" }));
    const question = await screen.findByRole("alertdialog", { name: "Delete imported-pager?" });
    expect(question).toHaveTextContent("No rule sends here.");
  });

  test("a channel deleted meanwhile is dropped from the list", async () => {
    const state = keeps();
    await page("channels", { queryClient: clientThatKeepsDataFresh() });
    await table("Alert channels");
    state.channels = state.channels.filter((one) => one.id !== 2);
    override("delete", "/api/alerts/channels/{id}", () => refuse(errors.not_found));
    await userEvent.click(within(rowWithCell("team-slack")).getByRole("button", { name: "Delete" }));
    const question = await screen.findByRole("alertdialog");
    await userEvent.click(within(question).getByRole("button", { name: "Delete" }));
    expect(await within(question).findByRole("alert")).toHaveTextContent(errors.not_found.body.error.message);
    await userEvent.click(within(question).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("cell", { name: "team-slack" })).toBeNull();
    });
  });

  test("tells how to verify signatures, with the header and an example in two languages", async () => {
    keeps();
    await page("channels");
    await table("Alert channels");
    const disclosure = screen.getByText("How to verify signatures").closest("details");
    expect(disclosure).not.toBeNull();
    if (disclosure === null) return;
    expect(disclosure).not.toHaveAttribute("open");
    await userEvent.click(within(disclosure).getByText("How to verify signatures"));
    expect(disclosure).toHaveAttribute("open");
    expect(disclosure).toHaveTextContent("x-uf-signature: t=<unix seconds>,v1=<hex>");
    expect(disclosure).toHaveTextContent("HMAC-SHA256");
    const code = [...disclosure.querySelectorAll("pre")];
    expect(code).toHaveLength(2);
    expect(code[0]?.textContent).toContain("hmac.compare_digest");
    expect(code[1]?.textContent).toContain("timingSafeEqual");
    for (const block of code) {
      expect(block.textContent.trim().split("\n").length).toBeLessThanOrEqual(7);
    }
  });

  test("the session ends while a channel is added with its URL typed", async () => {
    keeps();
    startGateway({ signedIn: true });
    override("post", "/api/alerts/channels", unauthenticated);
    const app = await page("channels");
    const dialog = await openAddChannel();
    await paste(within(dialog).getByLabelText("Name"), "late");
    await paste(within(dialog).getByLabelText("URL"), HOOK_URL);
    await submit(dialog, "Add channel");
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/alerts#channels")}`);
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expectNoSecret(app, HOOK_TOKEN);
  });

  test("at 390 px the channels are cards and the form fits", async () => {
    keeps();
    await page("channels", { width: 390 });
    const cards = await screen.findByRole("list", { name: "Alert channels" });
    expect(cards.children).toHaveLength(3);
    const disclosure = screen.getByText("How to verify signatures");
    expect(disclosure.className.split(/\s+/)).toContain("min-h-11");
    const dialog = await openAddChannel();
    for (const control of [
      within(dialog).getByLabelText("Name"),
      within(dialog).getByLabelText("URL"),
      within(dialog).getByRole("button", { name: "Add channel" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
  });
});

describe("history", () => {
  test("lists the events, newest first, with what was delivered", async () => {
    keeps();
    await page("history");
    const list = await table("Alert history");
    expect(within(list).getAllByRole("columnheader").map((head) => head.textContent)).toEqual([
      "Time",
      "Rule",
      "State",
      "Subject",
      "Summary",
      "Deliveries",
    ]);
    const rows = within(list).getAllByRole("row").slice(1);
    expect(rows.map((row) => within(row).getAllByRole("cell")[2]?.textContent)).toEqual([
      "Firing",
      "Resolved",
      "Test",
    ]);
    const firing = rows[0];
    expect(firing).toHaveTextContent("Search budget 80%");
    expect(firing).toHaveTextContent("team Platform weekly from 2026-09-28");
    expect(firing).toHaveTextContent("Budget 'team Platform weekly' passed 80% (8.00 of 10.00 USD)");
    expect(firing).toHaveTextContent("1 of 2 delivered");
    expect(firing).toHaveTextContent("ops-webhook: delivered (200)");
    expect(firing).toHaveTextContent("team-slack: failed, the receiver answered 500 (3 tries)");
    expect(rows[1]).toHaveTextContent("1 delivered");
    expect(rows[2]).toHaveTextContent("None yet");
    expectOneH1("Alerts");
  });

  test("asks for the newest 50, and filters by rule and by state", async () => {
    const state = keeps();
    await page("history");
    await table("Alert history");
    expect(state.eventQueries.at(-1)?.get("limit")).toBe("50");
    expect(state.eventQueries.at(-1)?.has("rule_id")).toBe(false);

    expect(await optionsOf(combo(document.body, "Rule"))).toEqual([
      "Any rule",
      "Search budget 80%",
      "Chat errors",
      "Circuit anywhere",
    ]);
    await choose(combo(document.body, "Rule"), "Chat errors");
    await waitFor(() => {
      expect(state.eventQueries.at(-1)?.get("rule_id")).toBe("2");
    });
    await waitFor(() => {
      expect(within(screen.getByRole("table", { name: "Alert history" })).getAllByRole("row")).toHaveLength(2);
    });

    expect(await optionsOf(combo(document.body, "State"))).toEqual([
      "Any state",
      "Firing",
      "Resolved",
      "Test",
    ]);
    await choose(combo(document.body, "State"), "Resolved");
    await waitFor(() => {
      expect(state.eventQueries.at(-1)?.get("state")).toBe("resolved");
    });
    expect(state.eventQueries.at(-1)?.get("rule_id")).toBe("2");
  });

  test("says so when no event matches, and when there are none", async () => {
    keeps({ events: [] });
    await page("history");
    expect(await screen.findByText("No alerts yet")).toBeInTheDocument();
    expect(
      screen.getByText("Events appear here when a rule fires or resolves, or when a test is sent."),
    ).toBeInTheDocument();
  });

  test("loads older events a page at a time", async () => {
    const state = keeps({ events: manyEvents(52) });
    await page("history");
    const list = await table("Alert history");
    expect(within(list).getAllByRole("row")).toHaveLength(51);
    expect(screen.getByText("Event number 1000")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Load older" }));
    await waitFor(() => {
      expect(within(screen.getByRole("table", { name: "Alert history" })).getAllByRole("row")).toHaveLength(53);
    });
    expect(state.eventQueries.at(-1)?.get("before_id")).toBe("951");
    expect(screen.queryByRole("button", { name: "Load older" })).toBeNull();
    expect(screen.getByText("Event number 949")).toBeInTheDocument();
  });

  test("a query the gateway refuses shows its message with Retry", async () => {
    keeps();
    override("get", "/api/alerts/events", () =>
      refuse(validationFailed({ state: "state must be firing, resolved or test" })),
    );
    await page("history");
    expect(await screen.findByRole("alert")).toHaveTextContent("Some fields are not valid.");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expectOneH1("Alerts");
  });

  test("a rule that was deleted is still named in its events", async () => {
    keeps({ events: [{ ...alertEvents.firing, rule_id: null }] });
    await page("history");
    await table("Alert history");
    expect(screen.getByRole("cell", { name: "Search budget 80%" })).toBeInTheDocument();
  });

  test("at 390 px the events are cards", async () => {
    keeps();
    await page("history", { width: 390 });
    const cards = await screen.findByRole("list", { name: "Alert history" });
    expect(cards.children).toHaveLength(3);
    expect(combo(document.body, "Rule").className.split(/\s+/)).toContain("min-h-11");
  });
});
