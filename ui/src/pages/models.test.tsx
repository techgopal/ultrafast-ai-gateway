import type { QueryClient } from "@tanstack/react-query";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { queryKeys } from "@/api/queries";
import { errors, fieldMessages, validationFailed, type GatewayError } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { startGateway } from "@/test/gateway";
import { noContent, ok, override, refuse } from "@/test/handlers";
import {
  choose,
  counted,
  expectLabelsNameControls,
  expectNotAvailable,
  expectOneH1,
  expectOneMain,
  expectOneRequestWhileTheDialogStays,
  expectSessionEndsOnPage,
  forbid,
  forgetToasts,
  held,
  href,
  installSelect,
  SESSION_ENDED,
  settle,
  toasts,
} from "@/test/pages";
import { renderWithApp, type AppRenderResult } from "@/test/render";

const { openaiMini, openaiFull, openaiDisabled, localLlama } = fixtures.models;
const { withCredential, withoutCredential } = fixtures.providers;

const NO_ACCESS_HINT = "Enabled, but nobody has access yet.";
const DELETE = "Routes that use this model lose this target. Calls to it fail at once.";
const NAME_HINT = "The provider's model ID, for example gpt-4o-mini.";
const EMPTY = "No models are available to you yet. Ask an admin.";

beforeAll(installSelect);
afterEach(forgetToasts);

type Options = { user?: fixtures.Me; width?: number; queryClient?: QueryClient };

function page(options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/models", ...options });
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Models" });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

function rowOf(model: fixtures.Model): HTMLElement {
  const row = screen.getByRole("cell", { name: model.name }).closest("tr");
  if (row === null) throw new Error(`no row for ${model.name}`);
  return row;
}

function read(body: unknown, name: string): unknown {
  return typeof body === "object" && body !== null ? Reflect.get(body, name) : undefined;
}

function buttons(scope: HTMLElement): string[] {
  return within(scope)
    .queryAllByRole("button")
    .map((control) => control.textContent);
}

function namesInOrder(): string[] {
  return [...document.querySelectorAll("tbody tr")].map(
    (row) => row.querySelector("td")?.textContent ?? "",
  );
}

/** A gateway that keeps the models and changes them as it is told. */
function keeps(start: readonly fixtures.Model[] = fixtures.modelList) {
  const state = {
    models: [...start],
    lists: 0,
    refuse: [] as GatewayError[],
    sent: [] as unknown[],
    patched: [] as { id: string | undefined; body: unknown }[],
    grants: [] as { id: string | undefined; body: unknown }[],
    deleted: [] as (string | undefined)[],
    synced: [] as (string | undefined)[],
    syncAnswer: fixtures.syncResult,
  };
  override("get", "/api/models", () => {
    state.lists += 1;
    return ok("get", "/api/models", 200, { models: state.models });
  });
  override("post", "/api/models", async ({ request }) => {
    const body: unknown = await request.json();
    state.sent.push(body);
    const refusal = state.refuse.shift();
    if (refusal !== undefined) return refuse(refusal);
    const providerId = Number(read(body, "provider_id"));
    const provider = fixtures.providerList.find((p) => p.id === providerId);
    const model: fixtures.Model = {
      id: 9,
      provider_id: providerId,
      provider_name: provider?.name ?? "",
      name: String(read(body, "name")),
      enabled: false,
      grants: { everyone: false, team_ids: [], user_ids: [] },
      created_at: "2026-09-30 09:00:00",
    };
    state.models = [...state.models, model];
    return ok("post", "/api/models", 201, model);
  });
  override("patch", "/api/models/{id}", async ({ request, params }) => {
    const body: unknown = await request.json();
    state.patched.push({ id: params.id, body });
    const refusal = state.refuse.shift();
    if (refusal !== undefined) return refuse(refusal);
    const was = state.models.find((m) => String(m.id) === params.id);
    if (was === undefined) return refuse(errors.not_found);
    const now = { ...was, enabled: read(body, "enabled") === true };
    state.models = state.models.map((m) => (m.id === was.id ? now : m));
    return ok("patch", "/api/models/{id}", 200, now);
  });
  override("put", "/api/models/{id}/grants", async ({ request, params }) => {
    const body: unknown = await request.json();
    state.grants.push({ id: params.id, body });
    const refusal = state.refuse.shift();
    if (refusal !== undefined) return refuse(refusal);
    const was = state.models.find((m) => String(m.id) === params.id);
    if (was === undefined) return refuse(errors.not_found);
    const now = { ...was, grants: body as fixtures.Grants };
    state.models = state.models.map((m) => (m.id === was.id ? now : m));
    return ok("put", "/api/models/{id}/grants", 200, now);
  });
  override("delete", "/api/models/{id}", ({ params }) => {
    state.deleted.push(params.id);
    const refusal = state.refuse.shift();
    if (refusal !== undefined) return refuse(refusal);
    state.models = state.models.filter((m) => String(m.id) !== params.id);
    return noContent();
  });
  override("post", "/api/providers/{id}/sync", ({ params }) => {
    state.synced.push(params.id);
    const refusal = state.refuse.shift();
    if (refusal !== undefined) return refuse(refusal);
    return ok("post", "/api/providers/{id}/sync", 200, state.syncAnswer);
  });
  return state;
}

async function closed(): Promise<void> {
  await waitFor(() => {
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

async function openDialog(button: string, name: string): Promise<HTMLElement> {
  await userEvent.click(await screen.findByRole("button", { name: button }));
  return screen.findByRole("dialog", { name });
}

const openSync = () => openDialog("Sync models", "Sync models");
const openAdd = () => openDialog("Add model", "Add model");

async function openAccess(model: fixtures.Model): Promise<HTMLElement> {
  await table();
  await userEvent.click(within(rowOf(model)).getByRole("button", { name: "Edit access" }));
  return screen.findByRole("dialog", { name: "Edit access" });
}

async function askToDelete(model: fixtures.Model): Promise<HTMLElement> {
  await table();
  await userEvent.click(within(rowOf(model)).getByRole("button", { name: "Delete" }));
  return screen.findByRole("alertdialog", { name: `Delete ${model.name}?` });
}

function choice(dialog: HTMLElement, provider: fixtures.Provider): Promise<void> {
  return userEvent.click(within(dialog).getByRole("radio", { name: provider.name }));
}

function press(dialog: HTMLElement, name: string): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name }));
}

function notice(): HTMLElement {
  return screen.getByRole("status");
}

describe("the list of models", () => {
  test("the list shows the name, the provider, the status and who has access", async () => {
    await page();
    const models = await table();
    expect(within(models).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Provider",
      "Status",
      "Access",
      "Actions",
    ]);
    // By provider, then by name.
    expect(namesInOrder()).toEqual(["llama3.1:8b", "gpt-4o", "gpt-4o-mini", "o3-mini"]);
    const cells = (model: fixtures.Model) =>
      within(rowOf(model)).getAllByRole("cell").slice(0, 2).map((cell) => cell.textContent);
    expect(cells(openaiMini)).toEqual(["gpt-4o-mini", "openai"]);
    expect(rowOf(openaiMini).querySelector("td")?.firstElementChild?.className).toContain("font-mono");
    expect(within(rowOf(openaiMini)).getByRole("switch")).toBeChecked();
    expect(within(rowOf(openaiMini)).getByText("Enabled")).toBeInTheDocument();
    expect(within(rowOf(openaiDisabled)).getByRole("switch")).not.toBeChecked();
    expect(within(rowOf(openaiDisabled)).getByText("Disabled")).toBeInTheDocument();
    expect(rowOf(openaiMini)).toHaveTextContent("Everyone");
    expect(rowOf(openaiFull)).toHaveTextContent("2 teams, 1 user");
    expect(rowOf(openaiDisabled)).toHaveTextContent("No one");
    expect(buttons(rowOf(openaiMini))).toEqual(["Edit access", "Delete"]);
    expect(screen.getByRole("button", { name: "Sync models" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add model" })).toBeInTheDocument();
    expectOneMain();
    expectOneH1("Models");
  });

  test("an enabled model that nobody may call says so in its row", async () => {
    await page();
    await table();
    expect(rowOf(localLlama)).toHaveTextContent(NO_ACCESS_HINT);
    expect(rowOf(openaiMini)).not.toHaveTextContent(NO_ACCESS_HINT);
    // Disabled: nothing to say yet.
    expect(rowOf(openaiDisabled)).not.toHaveTextContent(NO_ACCESS_HINT);
  });

  test("the filters: text, provider and status", async () => {
    await page();
    await table();
    const search = screen.getByRole("searchbox", { name: "Search" });
    await userEvent.type(search, "MINI");
    expect(namesInOrder()).toEqual(["gpt-4o-mini", "o3-mini"]);
    await userEvent.clear(search);
    await userEvent.type(search, "local");
    expect(namesInOrder()).toEqual(["llama3.1:8b"]);
    await userEvent.clear(search);

    await choose(screen.getByRole("combobox", { name: "Provider" }), "openai");
    expect(namesInOrder()).toEqual(["gpt-4o", "gpt-4o-mini", "o3-mini"]);
    await choose(screen.getByRole("combobox", { name: "Status" }), "Disabled");
    expect(namesInOrder()).toEqual(["o3-mini"]);
    await choose(screen.getByRole("combobox", { name: "Status" }), "Enabled");
    expect(namesInOrder()).toEqual(["gpt-4o", "gpt-4o-mini"]);
    await choose(screen.getByRole("combobox", { name: "Provider" }), "All providers");
    await choose(screen.getByRole("combobox", { name: "Status" }), "All statuses");
    expect(namesInOrder()).toHaveLength(4);

    await userEvent.type(search, "nothing like it");
    expect(await screen.findByText("No models match")).toBeInTheDocument();
    expectOneH1();
  });

  test("no models: an empty state with the actions still there", async () => {
    keeps([]);
    await page();
    expect(await screen.findByText("No models")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sync models" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add model" })).toBeInTheDocument();
    expectOneMain();
    expectOneH1();
  });

  test("a failed list call shows the error under the title: one h1", async () => {
    override("get", "/api/models", () => refuse(errors.internal_error));
    await page();
    await screen.findByRole("alert");
    expectOneH1("Models");
    expectOneMain();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("a 403 of the list: not available", async () => {
    forbid("/api/models");
    await page();
    await expectNotAvailable();
    expectOneMain();
    expectOneH1();
  });

  test("the session ends while the list is open", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    await table();
    await expectSessionEndsOnPage(app, {
      path: "/api/models",
      queryKey: queryKeys.models.list(),
      at: "/models",
    });
  });

  test("at width 390 the rows are cards with their labels and actions", async () => {
    await page({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Models" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(fixtures.modelList.length);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Name",
      "Provider",
      "Status",
      "Access",
    ]);
    for (const control of [
      ...screen.getAllByRole("button", { name: /Sync models|Add model|Edit access|Delete/ }),
      ...screen.getAllByRole("switch").map((toggle) => toggle.closest("label") ?? toggle),
      screen.getByRole("searchbox", { name: "Search" }),
      screen.getByRole("combobox", { name: "Provider" }),
      screen.getByRole("combobox", { name: "Status" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
  });
});

describe("enabling and disabling", () => {
  test("a switch sends the change, and the row follows", async () => {
    const state = keeps();
    await page();
    await table();
    await userEvent.click(within(rowOf(openaiMini)).getByRole("switch"));
    await waitFor(() => {
      expect(within(rowOf(openaiMini)).getByRole("switch")).not.toBeChecked();
    });
    expect(state.patched).toEqual([{ id: "1", body: { enabled: false } }]);
    expect(within(rowOf(openaiMini)).getByText("Disabled")).toBeInTheDocument();
    // The list was asked for again: the row is what the gateway says.
    expect(state.lists).toBe(2);

    await userEvent.click(within(rowOf(openaiDisabled)).getByRole("switch"));
    await waitFor(() => {
      expect(within(rowOf(openaiDisabled)).getByRole("switch")).toBeChecked();
    });
    expect(state.patched[1]).toEqual({ id: "3", body: { enabled: true } });
    // Enabled with no grant: the hint comes.
    expect(rowOf(openaiDisabled)).toHaveTextContent(NO_ACCESS_HINT);
  });

  test("a refused change is said, and the switch stays as the gateway has it", async () => {
    const state = keeps();
    state.refuse.push(errors.forbidden);
    await page();
    await table();
    await userEvent.click(within(rowOf(openaiMini)).getByRole("switch"));
    expect(await screen.findByRole("alert")).toHaveTextContent(errors.forbidden.body.error.message);
    expect(within(rowOf(openaiMini)).getByRole("switch")).toBeChecked();
    expect(toasts()).toEqual([]);
  });

  test("the switch of a row cannot be pressed again while its change is on its way", async () => {
    const request = held("patch", "/api/models/{id}");
    await page();
    await table();
    const toggle = within(rowOf(openaiMini)).getByRole("switch");
    await userEvent.click(toggle);
    await waitFor(() => {
      expect(toggle).toBeDisabled();
    });
    await userEvent.click(toggle);
    expect(request.calls).toBe(1);
    request.answer();
    await screen.findByRole("alert");
    expect(request.calls).toBe(1);
    await waitFor(() => {
      expect(toggle).toBeEnabled();
    });
  });
});

describe("syncing the models of a provider", () => {
  test("it asks for a provider and says how many models were added", async () => {
    const state = keeps();
    await page();
    await table();
    const dialog = await openSync();
    expectLabelsNameControls(dialog);
    const group = within(dialog).getByRole("radiogroup", { name: "Provider" });
    expect(within(group).getAllByRole("radio").map((r) => r.getAttribute("aria-label") ?? r.id)).toHaveLength(2);
    await choice(dialog, withCredential);
    await press(dialog, "Sync");
    await closed();
    expect(state.synced).toEqual(["1"]);
    expect(notice()).toHaveTextContent("Added 2 models. They start disabled.");
    // The new models are listed afterwards.
    expect(state.lists).toBe(2);
    expect(toasts()).toEqual([]);
  });

  test("no new names", async () => {
    const state = keeps();
    state.syncAnswer = { added: [], existing: 7 };
    await page();
    const dialog = await openSync();
    await choice(dialog, withoutCredential);
    await press(dialog, "Sync");
    await closed();
    expect(state.synced).toEqual(["2"]);
    expect(notice()).toHaveTextContent("No new models.");
  });

  test("a provider that cannot be synced says what the gateway says, and the dialog stays", async () => {
    const state = keeps();
    state.refuse.push(errors.sync_unsupported);
    await page();
    const dialog = await openSync();
    await choice(dialog, withCredential);
    await press(dialog, "Sync");
    expect(await within(dialog).findByText(errors.sync_unsupported.body.error.message)).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Sync models" })).toBe(dialog);
    // Behind the open dialog the page is hidden from the accessibility tree.
    expect(screen.getByRole("status", { hidden: true })).toBeEmptyDOMElement();
  });

  test("a provider that did not answer: 502", async () => {
    const state = keeps();
    state.refuse.push(errors.sync_failed);
    await page();
    const dialog = await openSync();
    await choice(dialog, withCredential);
    await press(dialog, "Sync");
    expect(await within(dialog).findByText(errors.sync_failed.body.error.message)).toBeInTheDocument();
    // It can be tried again.
    await press(dialog, "Sync");
    await closed();
    expect(state.synced).toEqual(["1", "1"]);
    expect(notice()).toHaveTextContent("Added 2 models. They start disabled.");
  });

  test("while it runs the dialog stays and one request is made", async () => {
    const request = held("post", "/api/providers/{id}/sync", errors.sync_failed);
    await page();
    const dialog = await openSync();
    await choice(dialog, withCredential);
    await expectOneRequestWhileTheDialogStays(
      dialog,
      within(dialog).getByRole("radio", { name: withCredential.name }),
      "Syncing",
      request,
      errors.sync_failed,
    );
  });

  test("the session ends while the sync runs: nothing is said", async () => {
    startGateway({ signedIn: true });
    const posts = counted("post", "/api/providers/{id}/sync", () => refuse(errors.unauthenticated));
    const app = await page();
    const dialog = await openSync();
    await choice(dialog, withCredential);
    await press(dialog, "Sync");
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fmodels");
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(posts.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("adding a model", () => {
  test("a provider and a name: the model is added, disabled, and listed", async () => {
    const state = keeps();
    await page();
    await table();
    const dialog = await openAdd();
    expectLabelsNameControls(dialog);
    expect(within(dialog).getByLabelText("Model name")).toHaveAccessibleDescription(NAME_HINT);
    await choice(dialog, withoutCredential);
    await userEvent.type(within(dialog).getByLabelText("Model name"), "qwen2.5:7b");
    await press(dialog, "Add model");
    await closed();
    expect(state.sent).toEqual([{ provider_id: 2, name: "qwen2.5:7b" }]);
    const row = await screen.findByRole("cell", { name: "qwen2.5:7b" });
    expect(row.closest("tr")).toHaveTextContent("Disabled");
    expect(toasts()).toEqual(["Model added."]);
    expect(toasts().join()).not.toContain("qwen");
  });

  test("a model the provider has already is refused on the name field", async () => {
    const state = keeps();
    state.refuse.push(errors.model_exists);
    await page();
    const dialog = await openAdd();
    await choice(dialog, withCredential);
    const name = within(dialog).getByLabelText("Model name");
    await userEvent.click(name);
    await userEvent.paste("gpt-4o");
    await press(dialog, "Add model");
    await waitFor(() => {
      expect(name).toHaveAccessibleDescription(
        new RegExp(errors.model_exists.body.error.message.replace(".", "\\.")),
      );
    });
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(name).toHaveFocus();
    expect(screen.getByRole("dialog", { name: "Add model" })).toBe(dialog);
    expect(toasts()).toEqual([]);
  });

  test("the gateway's field errors show on their fields", async () => {
    const state = keeps();
    state.refuse.push(
      validationFailed({
        name: fieldMessages.modelNameWhitespace,
        provider_id: fieldMessages.providerMissing,
      }),
    );
    await page();
    const dialog = await openAdd();
    await choice(dialog, withCredential);
    await userEvent.click(within(dialog).getByLabelText("Model name"));
    await userEvent.paste("a b");
    await press(dialog, "Add model");
    await waitFor(() => {
      expect(within(dialog).getByLabelText("Model name")).toHaveAccessibleDescription(
        new RegExp(fieldMessages.modelNameWhitespace),
      );
    });
    expect(within(dialog).getByRole("radiogroup", { name: "Provider" })).toHaveAccessibleDescription(
      fieldMessages.providerMissing,
    );
  });

  test("the console asks for a name before it sends anything", async () => {
    const state = keeps();
    await page();
    const dialog = await openAdd();
    await choice(dialog, withCredential);
    await press(dialog, "Add model");
    await settle();
    expect(state.sent).toEqual([]);
    expect(within(dialog).getByLabelText("Model name")).toHaveAttribute("aria-invalid", "true");
  });

  test("while it runs the dialog stays and one request is made", async () => {
    const request = held("post", "/api/models");
    await page();
    const dialog = await openAdd();
    await choice(dialog, withCredential);
    const name = within(dialog).getByLabelText("Model name");
    await userEvent.click(name);
    await userEvent.paste("gpt-4.1");
    await expectOneRequestWhileTheDialogStays(dialog, name, "Adding the model", request);
  });
});

describe("who has access", () => {
  function everyone(dialog: HTMLElement): HTMLElement {
    return within(dialog).getByRole("switch", { name: "Everyone" });
  }
  function box(dialog: HTMLElement, group: "Teams" | "Users", name: string | RegExp): HTMLElement {
    return within(within(dialog).getByRole("group", { name: group })).getByRole("checkbox", { name });
  }

  test("the dialog shows the grants of the model", async () => {
    await page();
    const dialog = await openAccess(openaiFull);
    expect(everyone(dialog)).not.toBeChecked();
    expect(await within(dialog).findByRole("group", { name: "Teams" })).toBeInTheDocument();
    expect(box(dialog, "Teams", "Platform")).toBeChecked();
    expect(box(dialog, "Teams", "Research")).toBeChecked();
    expect(box(dialog, "Teams", "Growth")).not.toBeChecked();
    expect(box(dialog, "Users", /Priya Raman/)).toBeChecked();
    expect(box(dialog, "Users", /Lena Fischer/)).not.toBeChecked();
    expectLabelsNameControls(dialog);
  });

  test("it sends the full grants: teams and users", async () => {
    const state = keeps();
    await page();
    const dialog = await openAccess(openaiDisabled);
    await within(dialog).findByRole("group", { name: "Teams" });
    await userEvent.click(box(dialog, "Teams", "Growth"));
    await userEvent.click(box(dialog, "Users", /Lena Fischer/));
    await userEvent.click(box(dialog, "Users", /Tomas Novak/));
    await press(dialog, "Save access");
    await closed();
    expect(state.grants).toEqual([
      { id: "3", body: { everyone: false, team_ids: [3], user_ids: [3, 4] } },
    ]);
    expect(toasts()).toEqual(["Access updated."]);
    await waitFor(() => {
      expect(rowOf(openaiDisabled)).toHaveTextContent("1 team, 2 users");
    });
  });

  test("everyone hides the teams and users and sends none", async () => {
    const state = keeps();
    await page();
    const dialog = await openAccess(openaiFull);
    await within(dialog).findByRole("group", { name: "Teams" });
    await userEvent.click(everyone(dialog));
    expect(everyone(dialog)).toBeChecked();
    expect(within(dialog).queryByRole("group", { name: "Teams" })).toBeNull();
    expect(within(dialog).queryByRole("group", { name: "Users" })).toBeNull();
    await press(dialog, "Save access");
    await closed();
    expect(state.grants).toEqual([
      { id: "2", body: { everyone: true, team_ids: [], user_ids: [] } },
    ]);
    await waitFor(() => {
      expect(rowOf(openaiFull)).toHaveTextContent("Everyone");
    });
  });

  test("no one at all can be sent", async () => {
    const state = keeps();
    await page();
    const dialog = await openAccess(openaiMini);
    expect(everyone(dialog)).toBeChecked();
    await userEvent.click(everyone(dialog));
    await within(dialog).findByRole("group", { name: "Teams" });
    await press(dialog, "Save access");
    await closed();
    expect(state.grants).toEqual([
      { id: "1", body: { everyone: false, team_ids: [], user_ids: [] } },
    ]);
  });

  test("a team that is no longer there is not sent", async () => {
    const state = keeps([
      { ...openaiFull, grants: { everyone: false, team_ids: [1, 99], user_ids: [5, 98] } },
    ]);
    await page();
    const dialog = await openAccess(openaiFull);
    await within(dialog).findByRole("group", { name: "Teams" });
    await press(dialog, "Save access");
    await closed();
    expect(state.grants[0]?.body).toEqual({ everyone: false, team_ids: [1], user_ids: [5] });
  });

  test("a refusal of the grants shows on its field and the dialog stays", async () => {
    const state = keeps();
    state.refuse.push(validationFailed({ everyone: fieldMessages.grantsEveryone }));
    await page();
    const dialog = await openAccess(openaiMini);
    await press(dialog, "Save access");
    await waitFor(() => {
      expect(everyone(dialog)).toHaveAccessibleDescription(fieldMessages.grantsEveryone);
    });
    expect(screen.getByRole("dialog", { name: "Edit access" })).toBe(dialog);
    expect(toasts()).toEqual([]);
  });

  test("the teams could not be read: it says so and cannot save", async () => {
    override("get", "/api/teams", () => refuse(errors.internal_error));
    await page();
    const dialog = await openAccess(openaiFull);
    expect(await within(dialog).findByRole("alert")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Save access" })).toBeDisabled();
    // With everyone on there is nothing to read.
    await userEvent.click(everyone(dialog));
    expect(within(dialog).getByRole("button", { name: "Save access" })).toBeEnabled();
  });

  test("while it runs the dialog stays and one request is made", async () => {
    const request = held("put", "/api/models/{id}/grants");
    await page();
    const dialog = await openAccess(openaiMini);
    await expectOneRequestWhileTheDialogStays(dialog, everyone(dialog), "Saving", request);
  });

  test("the access dialog of a model that is gone: the list shows what is so", async () => {
    const state = keeps();
    await page();
    const dialog = await openAccess(openaiMini);
    state.refuse.push(errors.not_found);
    await press(dialog, "Save access");
    expect(await within(dialog).findByText(errors.not_found.body.error.message)).toBeInTheDocument();
  });
});

describe("deleting a model", () => {
  test("the question says what happens, and the model is deleted", async () => {
    const state = keeps();
    await page();
    const dialog = await askToDelete(openaiMini);
    expect(dialog).toHaveAccessibleDescription(DELETE);
    await press(dialog, "Delete");
    await closed();
    expect(state.deleted).toEqual(["1"]);
    expect(screen.queryByRole("cell", { name: "gpt-4o-mini" })).toBeNull();
    expect(toasts()).toEqual(["Model deleted."]);
  });

  test("Cancel deletes nothing", async () => {
    const state = keeps();
    await page();
    const dialog = await askToDelete(openaiMini);
    await press(dialog, "Cancel");
    await closed();
    expect(state.deleted).toEqual([]);
    expect(toasts()).toEqual([]);
  });

  test("a refusal stays in the dialog", async () => {
    const state = keeps();
    state.refuse.push(errors.internal_error);
    await page();
    const dialog = await askToDelete(openaiMini);
    await press(dialog, "Delete");
    expect(await within(dialog).findByText(errors.internal_error.body.error.message)).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "gpt-4o-mini", hidden: true })).toBeInTheDocument();
  });
});

describe("who sees what", () => {
  function callable() {
    return counted("get", "/api/models", () =>
      ok("get", "/api/models", 200, { models: fixtures.callableModels }),
    );
  }

  test("a member sees the models they can use, read-only, with a copy button", async () => {
    callable();
    const teams = forbid("/api/teams");
    const users = forbid("/api/users");
    await page({ user: fixtures.me.tomas });
    const list = await screen.findByRole("list", { name: "Models you can use" });
    expect(screen.getByRole("heading", { level: 2, name: "Models you can use" })).toBeInTheDocument();
    const items = within(list).getAllByRole("listitem");
    expect(items.map((item) => item.textContent.replace("Copy", ""))).toEqual([
      "openai/gpt-4o",
      "openai/gpt-4o-mini",
    ]);
    expect(within(list).getByText("openai/gpt-4o-mini").className).toContain("font-mono");
    for (const name of [
      "Sync models",
      "Add model",
      "Edit access",
      "Delete",
    ]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(screen.queryByText(/Everyone|No one/)).toBeNull();
    await settle();
    expect(teams.calls + users.calls).toBe(0);
    expectOneMain();
    expectOneH1("Models");
  });

  test("the copy button copies provider/model", async () => {
    callable();
    const written: string[] = [];
    const before = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: (text: string) => Promise.resolve(void written.push(text)) },
    });
    try {
      await page({ user: fixtures.me.tomas });
      const list = await screen.findByRole("list", { name: "Models you can use" });
      await userEvent.click(within(list).getByRole("button", { name: "Copy openai/gpt-4o" }));
      await waitFor(() => {
        expect(written).toEqual(["openai/gpt-4o"]);
      });
      expect(toasts()).toEqual(["Copied."]);
    } finally {
      if (before === undefined) Reflect.deleteProperty(navigator, "clipboard");
      else Object.defineProperty(navigator, "clipboard", before);
    }
  });

  test("a clipboard that refuses is said without a name", async () => {
    callable();
    const before = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: () => Promise.reject(new Error("not allowed")) },
    });
    try {
      await page({ user: fixtures.me.tomas });
      const list = await screen.findByRole("list", { name: "Models you can use" });
      await userEvent.click(within(list).getByRole("button", { name: "Copy openai/gpt-4o" }));
      await waitFor(() => {
        expect(toasts()).toEqual(["Could not copy. Select the name and copy it by hand."]);
      });
    } finally {
      if (before === undefined) Reflect.deleteProperty(navigator, "clipboard");
      else Object.defineProperty(navigator, "clipboard", before);
    }
  });

  test("a member with no model sees how to get one", async () => {
    override("get", "/api/models", () => ok("get", "/api/models", 200, { models: [] }));
    await page({ user: fixtures.me.priya });
    expect(await screen.findByText(EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Models you can use" })).toBeNull();
    expectOneMain();
    expectOneH1("Models");
  });

  test("a failed list call for a member: one h1 and a way to retry", async () => {
    override("get", "/api/models", () => refuse(errors.internal_error));
    await page({ user: fixtures.me.tomas });
    await screen.findByRole("alert");
    expectOneH1("Models");
    expectOneMain();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("at width 390 the member's list fits and its buttons are high enough to touch", async () => {
    callable();
    await page({ user: fixtures.me.tomas, width: 390 });
    const list = await screen.findByRole("list", { name: "Models you can use" });
    for (const control of within(list).getAllByRole("button")) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
  });

  test("a failed list is retried", async () => {
    let calls = 0;
    override("get", "/api/models", () => {
      calls += 1;
      return calls === 1
        ? refuse(errors.internal_error)
        : ok("get", "/api/models", 200, { models: fixtures.callableModels });
    });
    await page({ user: fixtures.me.tomas });
    await screen.findByRole("alert");
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("list", { name: "Models you can use" })).toBeInTheDocument();
  });
});
