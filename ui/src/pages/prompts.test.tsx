import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { errors, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { noContent, ok, override, refuse } from "@/test/handlers";
import {
  choose,
  expectLabelsNameControls,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  href,
  installSelect,
  optionsOf,
  rowWithCell,
  SESSION_ENDED,
  toasts,
} from "@/test/pages";
import { memberUser, renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

const { prompts, promptVersions } = fixtures;

type Versions = Record<string, fixtures.PromptVersion[]>;

function page(path = "/prompts", user?: fixtures.Me): Promise<AppRenderResult> {
  return renderWithApp(null, { route: path, ...(user === undefined ? {} : { user }) });
}

function readBody(body: unknown): Record<string, unknown> {
  return typeof body === "object" && body !== null ? Object.fromEntries(Object.entries(body)) : {};
}

/** A gateway that keeps the templates and changes them as it is told. */
function keeps(start: fixtures.PromptSummary[] = fixtures.promptList) {
  const versions: Versions = {
    summarize: [...promptVersions.summarize],
    greet: [...promptVersions.greet],
  };
  const state = {
    list: [...start],
    lists: 0,
    created: [] as Record<string, unknown>[],
    added: [] as { id: string | undefined; body: Record<string, unknown> }[],
    deleted: [] as (string | undefined)[],
    refuseCreate: [] as ReturnType<typeof validationFailed>[],
  };
  const nameOf = (id: string | undefined) => state.list.find((one) => String(one.id) === id)?.name;
  const viewOf = (summary: fixtures.PromptSummary): fixtures.PromptView => ({
    ...summary,
    versions: (versions[summary.name] ?? []).map(({ version, created_by, created_at }) => ({
      version,
      created_by,
      created_at,
    })),
  });
  override("get", "/api/prompts", () => {
    state.lists += 1;
    return ok("get", "/api/prompts", 200, { prompts: state.list });
  });
  override("get", "/api/prompts/{id}", ({ params }) => {
    const one = state.list.find((item) => String(item.id) === params.id);
    return one === undefined ? refuse(errors.not_found) : ok("get", "/api/prompts/{id}", 200, viewOf(one));
  });
  override("get", "/api/prompts/{id}/versions/{version}", ({ params }) => {
    const found = (versions[nameOf(params.id) ?? ""] ?? []).find((one) => String(one.version) === params.version);
    return found === undefined
      ? refuse(errors.not_found)
      : ok("get", "/api/prompts/{id}/versions/{version}", 200, found);
  });
  override("post", "/api/prompts", async ({ request }) => {
    const body = readBody(await request.json());
    state.created.push(body);
    const refusal = state.refuseCreate.shift();
    if (refusal !== undefined) return refuse(refusal);
    const made: fixtures.PromptSummary = {
      ...prompts.greet,
      id: 9,
      name: String(body.name),
      latest_version: 1,
      version_count: 1,
      variables: [],
    };
    versions[made.name] = [{ ...fixtures.newPromptVersion, version: 1 }];
    state.list = [...state.list, made];
    return ok("post", "/api/prompts", 201, viewOf(made));
  });
  override("post", "/api/prompts/{id}/versions", async ({ request, params }) => {
    const body = readBody(await request.json());
    state.added.push({ id: params.id, body });
    const name = nameOf(params.id);
    if (name === undefined) return refuse(errors.not_found);
    const next = (versions[name]?.length ?? 0) + 1;
    const made: fixtures.PromptVersion = {
      version: next,
      messages: (body.messages as fixtures.PromptVersion["messages"] | undefined) ?? [],
      variables: [],
      model: typeof body.model === "string" ? body.model : null,
      params: {},
      created_by: 1,
      created_at: "2026-09-30 12:00:00",
    };
    versions[name] = [...(versions[name] ?? []), made];
    state.list = state.list.map((one) =>
      one.name === name ? { ...one, latest_version: next, version_count: next } : one,
    );
    return ok("post", "/api/prompts/{id}/versions", 201, made);
  });
  override("delete", "/api/prompts/{id}", ({ params }) => {
    state.deleted.push(params.id);
    state.list = state.list.filter((one) => String(one.id) !== params.id);
    return noContent();
  });
  return state;
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Prompt templates" });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

async function paste(field: HTMLElement, text: string): Promise<void> {
  await userEvent.click(field);
  await userEvent.paste(text);
}

async function replaceText(field: HTMLElement, text: string): Promise<void> {
  await userEvent.clear(field);
  await userEvent.click(field);
  await userEvent.paste(text);
}

/** What a line is to assistive technology: the marks that are for the eyes only are left out. */
function spoken(item: HTMLElement): string {
  return [...item.childNodes]
    .filter((node) => !(node instanceof HTMLElement && node.getAttribute("aria-hidden") === "true"))
    .map((node) => node.textContent)
    .join("");
}

async function submit(label: string): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: label }));
}

describe("the list", () => {
  test("shows each template with its latest version, model and variables", async () => {
    keeps();
    await page();
    const list = await table();
    expectOneMain();
    expectOneH1("Prompts");
    expect(within(list).getAllByRole("columnheader").map((head) => head.textContent)).toEqual([
      "Name",
      "Description",
      "Latest",
      "Model",
      "Variables",
      "Updated",
      "Actions",
    ]);
    const summarize = rowWithCell("summarize");
    expect(summarize).toHaveTextContent("Version 3 of 3");
    expect(summarize).toHaveTextContent("openai/gpt-4o");
    expect(summarize).toHaveTextContent("audience, text");
    expect(within(summarize).getByTitle("2026-09-22 09:00:00 UTC")).toBeInTheDocument();
    const greet = rowWithCell("greet");
    expect(greet).toHaveTextContent("Version 1 of 1");
    expect(greet).toHaveTextContent("A call names one");
    expect(greet).toHaveTextContent("name");
  });

  test("each row opens the template, or the playground with it chosen at its latest version", async () => {
    keeps();
    await page();
    await table();
    const row = rowWithCell("summarize");
    expect(within(row).getByRole("link", { name: "Open" })).toHaveAttribute("href", "/prompts/1");
    expect(within(row).getByRole("link", { name: "Open in Playground" })).toHaveAttribute(
      "href",
      "/playground?prompt=summarize&version=3",
    );
  });

  test("says so when there are none", async () => {
    keeps([]);
    await page();
    expect(await screen.findByText("No prompt templates")).toBeInTheDocument();
    expectOneH1("Prompts");
    expect(screen.getByRole("link", { name: "New template" })).toHaveAttribute("href", "/prompts/new");
  });

  test("a member reads the templates and is offered neither New template nor Delete", async () => {
    keeps();
    await page("/prompts", memberUser);
    await table();
    expect(screen.queryByRole("link", { name: "New template" })).toBeNull();
    expect(within(rowWithCell("summarize")).queryByRole("button", { name: "Delete" })).toBeNull();
    expect(within(rowWithCell("greet")).queryByRole("button", { name: "Delete" })).toBeNull();
    expect(within(rowWithCell("greet")).getByRole("link", { name: "Open in Playground" })).toBeInTheDocument();
  });

  test("a team lead makes templates and deletes only their own", async () => {
    keeps();
    await page("/prompts", fixtures.me.arjun);
    await table();
    expect(screen.getByRole("link", { name: "New template" })).toBeInTheDocument();
    // greet was made by the lead (user 2); summarize by the admin.
    expect(within(rowWithCell("greet")).getByRole("button", { name: "Delete" })).toBeInTheDocument();
    expect(within(rowWithCell("summarize")).queryByRole("button", { name: "Delete" })).toBeNull();
  });

  test("flags a template whose latest version cannot be read", async () => {
    keeps([{ ...prompts.greet, unreadable: true, model: null, variables: [] }]);
    await page();
    await table();
    expect(rowWithCell("greet")).toHaveTextContent("Cannot be read");
  });

  test("a list that fails shows the error with Retry", async () => {
    let fail = true;
    override("get", "/api/prompts", () =>
      fail
        ? refuse(errors.internal_error)
        : ok("get", "/api/prompts", 200, { prompts: fixtures.promptList }),
    );
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    fail = false;
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await table();
    expectOneH1("Prompts");
  });

  test("the session ends while the list is open", async () => {
    keeps();
    const app = await page();
    await table();
    override("get", "/api/prompts", unauthenticated);
    override("get", "/api/auth/me", unauthenticated);
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: ["prompts"] });
    });
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/prompts")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
  });

  test("deleting asks first, says what goes, and the list shows it afterwards", async () => {
    const state = keeps();
    await page();
    await table();
    await userEvent.click(within(rowWithCell("summarize")).getByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete summarize?" });
    expect(dialog).toHaveTextContent(
      "Its 3 versions are deleted and calls that name it fail. The logs keep the name and version they recorded. This cannot be undone.",
    );
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(state.deleted).toEqual(["1"]);
    });
    await waitFor(() => {
      expect(screen.queryByRole("cell", { name: "summarize" })).toBeNull();
    });
    expect(state.lists).toBeGreaterThan(1);
    expect(toasts()).toEqual(["Template deleted."]);
  });

  test("a template that is gone when it is deleted says so in the dialog", async () => {
    keeps();
    await page();
    await table();
    override("delete", "/api/prompts/{id}", () => refuse(errors.not_found));
    await userEvent.click(within(rowWithCell("summarize")).getByRole("button", { name: "Delete" }));
    const dialog = await screen.findByRole("alertdialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(errors.not_found.body.error.message);
    expect(toasts()).toEqual([]);
  });
});

describe("a new template", () => {
  test("is made from a name, messages and settings, and opens when made", async () => {
    const state = keeps();
    const app = await page("/prompts/new");
    expectOneH1("New template");
    await paste(await screen.findByLabelText("Name"), "  translate ");
    await paste(screen.getByLabelText("Description"), "Translates a text.");
    await choose(screen.getByRole("combobox", { name: "Role of message 1" }), "system");
    await paste(screen.getByLabelText("Message 1"), "Translate to {{language}}.");
    await userEvent.click(screen.getByRole("button", { name: "Add message" }));
    await paste(screen.getByLabelText("Message 2"), "{{text}}");
    await paste(screen.getByLabelText("Model"), "openai/gpt-4o");
    await paste(screen.getByLabelText("Temperature"), "0.3");
    await paste(screen.getByLabelText("Max tokens"), "300");
    await paste(screen.getByLabelText("Top P"), "0.9");
    await submit("Create template");
    await waitFor(() => {
      expect(state.created).toHaveLength(1);
    });
    expect(state.created[0]).toEqual({
      name: "translate",
      description: "Translates a text.",
      model: "openai/gpt-4o",
      params: { temperature: 0.3, top_p: 0.9, max_tokens: 300 },
      messages: [
        { role: "system", content: "Translate to {{language}}." },
        { role: "user", content: "{{text}}" },
      ],
    });
    await waitFor(() => {
      expect(href(app)).toBe("/prompts/9");
    });
    expect(toasts()).toEqual(["Template created."]);
  });

  test("names the variables as the messages are written", async () => {
    keeps();
    await page("/prompts/new");
    const status = await screen.findByRole("status", { name: "Variables" });
    expect(status).toHaveTextContent("No variables yet. Write {{name}} where a value goes.");
    await paste(screen.getByLabelText("Message 1"), "Hi {{b}} and {{a}} and {{ not }} and {{a}}");
    expect(status).toHaveTextContent("Variables: a, b");
    await replaceText(screen.getByLabelText("Message 1"), "Plain text");
    expect(status).toHaveTextContent("No variables yet.");
  });

  test("messages can be added, moved and removed; the last one stays", async () => {
    keeps();
    await page("/prompts/new");
    await paste(await screen.findByLabelText("Message 1"), "first");
    await userEvent.click(screen.getByRole("button", { name: "Add message" }));
    await paste(screen.getByLabelText("Message 2"), "second");
    await userEvent.click(screen.getByRole("button", { name: "Move message 2 up" }));
    expect(screen.getByLabelText("Message 1")).toHaveValue("second");
    expect(screen.getByLabelText("Message 2")).toHaveValue("first");
    expect(screen.getByRole("button", { name: "Move message 1 up" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move message 2 down" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Remove message 1" }));
    expect(screen.getByLabelText("Message 1")).toHaveValue("first");
    expect(screen.queryByLabelText("Message 2")).toBeNull();
    expect(screen.getByRole("button", { name: "Remove message 1" })).toBeDisabled();
  });

  test("a form that is not valid says what is wrong, sends nothing and takes the focus", async () => {
    const state = keeps();
    await page("/prompts/new");
    await paste(await screen.findByLabelText("Name"), "a@b");
    await paste(screen.getByLabelText("Temperature"), "3");
    await submit("Create template");
    expect(await screen.findByText("The name cannot contain @: the logs write name@version.")).toBeInTheDocument();
    expect(screen.getByText("Write the message.")).toBeInTheDocument();
    expect(screen.getByText("From 0 to 2.")).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveAttribute("aria-invalid", "true");
    expect(state.created).toEqual([]);
    await waitFor(() => {
      expect(screen.getByLabelText("Name")).toHaveFocus();
    });
  });

  test("a name that is taken shows on the name field", async () => {
    const state = keeps();
    state.refuseCreate.push(errors.prompt_exists);
    await page("/prompts/new");
    await paste(await screen.findByLabelText("Name"), "summarize");
    await paste(screen.getByLabelText("Message 1"), "x");
    await submit("Create template");
    expect(await screen.findByText(errors.prompt_exists.body.error.message)).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveAttribute("aria-invalid", "true");
    expect(toasts()).toEqual([]);
  });

  test("a field the gateway refuses is shown, and the form can be sent again", async () => {
    const state = keeps();
    state.refuseCreate.push(validationFailed({ model: "model must be 1 to 200 characters" }));
    await page("/prompts/new");
    await paste(await screen.findByLabelText("Name"), "t");
    await paste(screen.getByLabelText("Message 1"), "x");
    await paste(screen.getByLabelText("Model"), "m");
    await submit("Create template");
    expect(await screen.findByText("model must be 1 to 200 characters")).toBeInTheDocument();
    await submit("Create template");
    await waitFor(() => {
      expect(state.created).toHaveLength(2);
    });
  });

  test("the labels name the controls", async () => {
    keeps();
    await page("/prompts/new");
    await screen.findByLabelText("Name");
    expectLabelsNameControls(screen.getByRole("form", { name: "Prompt template" }));
  });

  test("the session ends while the form is sent", async () => {
    keeps();
    const app = await page("/prompts/new");
    await paste(await screen.findByLabelText("Name"), "n");
    await paste(screen.getByLabelText("Message 1"), "x");
    override("post", "/api/prompts", unauthenticated);
    override("get", "/api/auth/me", unauthenticated);
    await submit("Create template");
    await waitFor(() => {
      expect(href(app)).toContain("/sign-in");
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(toasts()).toEqual([]);
  });

  test("a member is not offered the page", async () => {
    await page("/prompts/new", memberUser);
    expect(await screen.findByText("This page is not available to your account.")).toBeInTheDocument();
    expectOneMain();
  });
});

describe("one template", () => {
  test("opens at its latest version with the messages, variables, model and settings", async () => {
    keeps();
    await page("/prompts/1");
    expect(await screen.findByRole("heading", { level: 1, name: "summarize" })).toBeInTheDocument();
    expectOneMain();
    expectOneH1("summarize");
    expect(screen.getByText("Summarizes a text for a reader.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to prompts" })).toHaveAttribute("href", "/prompts");
    const messages = await screen.findByRole("list", { name: "Messages of version 3" });
    const items = within(messages).getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "systemYou write short summaries for {{audience}}.",
      "userSummarize {{text}}\nKeep it short.",
    ]);
    const shown = screen.getByRole("region", { name: "Version" });
    expect(within(shown).getByText("Variables: audience, text")).toBeInTheDocument();
    expect(within(shown).getByText("Model: openai/gpt-4o")).toBeInTheDocument();
    expect(within(shown).getByText("Temperature 0.2, max tokens 200")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Version" })).toHaveTextContent("Version 3 (latest)");
    expect(screen.getByRole("link", { name: "Open in Playground" })).toHaveAttribute(
      "href",
      "/playground?prompt=summarize&version=3",
    );
  });

  test("another version is read, with its own variables, and Open in Playground names it", async () => {
    keeps();
    await page("/prompts/1");
    const select = await screen.findByRole("combobox", { name: "Version" });
    expect(await optionsOf(select)).toEqual(["Version 3 (latest)", "Version 2", "Version 1"]);
    await choose(select, "Version 1");
    const messages = await screen.findByRole("list", { name: "Messages of version 1" });
    expect(within(messages).getAllByRole("listitem")).toHaveLength(1);
    const shown = screen.getByRole("region", { name: "Version" });
    expect(within(shown).getByText("Variables: text")).toBeInTheDocument();
    expect(within(shown).getByText("Model: none, a call names one")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open in Playground" })).toHaveAttribute(
      "href",
      "/playground?prompt=summarize&version=1",
    );
  });

  test("compares two versions line by line, with the changes in words and not by colour alone", async () => {
    keeps();
    await page("/prompts/1");
    await screen.findByRole("list", { name: "Messages of version 3" });
    const compare = screen.getByRole("combobox", { name: "Compare with" });
    expect(await optionsOf(compare)).toEqual(["Nothing", "Version 2", "Version 1"]);
    expect(screen.queryByRole("region", { name: "Changes from version 2 to version 3" })).toBeNull();
    await choose(compare, "Version 2");
    const changes = await screen.findByRole("region", { name: "Changes from version 2 to version 3" });
    const lines = within(changes).getAllByRole("listitem").map(spoken);
    expect(lines).toEqual([
      "system:",
      "Removed: You write short summaries.",
      "Added: You write short summaries for {{audience}}.",
      "user:",
      "Summarize {{text}}",
      "Keep it short.",
    ]);
    // Back to nothing.
    await choose(compare, "Nothing");
    expect(screen.queryByRole("region", { name: /Changes from/ })).toBeNull();
  });

  test("compares any two versions, not only the latest", async () => {
    keeps();
    await page("/prompts/1");
    await screen.findByRole("list", { name: "Messages of version 3" });
    await choose(screen.getByRole("combobox", { name: "Version" }), "Version 2");
    await choose(screen.getByRole("combobox", { name: "Compare with" }), "Version 1");
    const changes = await screen.findByRole("region", { name: "Changes from version 1 to version 2" });
    expect(changes).toHaveTextContent("Added: You write short summaries.");
  });

  test("a template with one version offers nothing to compare with", async () => {
    keeps();
    await page("/prompts/2");
    await screen.findByRole("list", { name: "Messages of version 1" });
    expect(screen.queryByRole("combobox", { name: "Compare with" })).toBeNull();
  });

  test("a member reads it and has no editor and no Delete", async () => {
    keeps();
    await page("/prompts/1", memberUser);
    await screen.findByRole("list", { name: "Messages of version 3" });
    expect(screen.queryByRole("heading", { name: "New version" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete template" })).toBeNull();
    expect(screen.getByRole("link", { name: "Open in Playground" })).toBeInTheDocument();
  });

  test("an admin adds the next version, starting from the latest, and sends only what a version has", async () => {
    const state = keeps();
    await page("/prompts/1", fixtures.me.maya);
    const first = await screen.findByLabelText("Message 1");
    expect(first).toHaveValue("You write short summaries for {{audience}}.");
    expect(screen.getByLabelText("Message 2")).toHaveValue("Summarize {{text}}\nKeep it short.");
    expect(screen.getByLabelText("Model")).toHaveValue("openai/gpt-4o");
    expect(screen.getByLabelText("Temperature")).toHaveValue("0.2");
    expect(screen.getByLabelText("Max tokens")).toHaveValue("200");
    // A version has no name or description of its own.
    expect(screen.queryByLabelText("Name")).toBeNull();
    expect(screen.queryByLabelText("Description")).toBeNull();
    await replaceText(first, "Short summaries only, for {{reader}}.");
    expect(screen.getByRole("status", { name: "Variables" })).toHaveTextContent("Variables: reader, text");
    await submit("Save as version 4");
    await waitFor(() => {
      expect(state.added).toHaveLength(1);
    });
    expect(state.added[0]).toEqual({
      id: "1",
      body: {
        model: "openai/gpt-4o",
        params: { temperature: 0.2, max_tokens: 200 },
        messages: [
          { role: "system", content: "Short summaries only, for {{reader}}." },
          { role: "user", content: "Summarize {{text}}\nKeep it short." },
        ],
      },
    });
    expect(toasts()).toEqual(["Version saved."]);
    // The page shows the version that was made, as the latest.
    expect(await screen.findByRole("button", { name: "Save as version 5" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Version" })).toHaveTextContent("Version 4 (latest)");
  });

  test("an old version can be taken as the start of the next", async () => {
    keeps();
    await page("/prompts/1", fixtures.me.maya);
    await screen.findByLabelText("Message 1");
    await choose(screen.getByRole("combobox", { name: "Version" }), "Version 1");
    await userEvent.click(screen.getByRole("button", { name: "Use version 1 as the start" }));
    await waitFor(() => {
      expect(screen.getByLabelText("Message 1")).toHaveValue("Summarize {{text}}");
    });
    expect(screen.queryByLabelText("Message 2")).toBeNull();
    expect(screen.getByLabelText("Model")).toHaveValue("");
    expect(screen.getByRole("button", { name: "Save as version 4" })).toBeInTheDocument();
  });

  test("the response format of the version before is kept in the next one", async () => {
    const state = keeps();
    const format = { type: "json_object" } as unknown as Record<string, never>;
    const withFormat = { ...fixtures.newPromptVersion, params: { response_format: format } };
    override("get", "/api/prompts/{id}/versions/{version}", ({ params }) =>
      ok("get", "/api/prompts/{id}/versions/{version}", 200, {
        ...withFormat,
        version: Number(params.version),
      }),
    );
    await page("/prompts/1", fixtures.me.maya);
    await screen.findByLabelText("Message 1");
    expect(screen.getByText("Response format: kept from this version.")).toBeInTheDocument();
    await submit("Save as version 4");
    await waitFor(() => {
      expect(state.added).toHaveLength(1);
    });
    expect(state.added[0]?.body.params).toEqual({ response_format: format });
  });

  test("a version that is not valid is not sent", async () => {
    const state = keeps();
    await page("/prompts/1", fixtures.me.maya);
    await replaceText(await screen.findByLabelText("Message 1"), "   ");
    await submit("Save as version 4");
    expect(await screen.findByText("Write the message.")).toBeInTheDocument();
    expect(state.added).toEqual([]);
  });

  test("a lead edits the template they made, and not another", async () => {
    keeps();
    await page("/prompts/2", fixtures.me.arjun);
    expect(await screen.findByRole("button", { name: "Save as version 2" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete template" })).toBeInTheDocument();
  });

  test("a lead does not edit the admin's template", async () => {
    keeps();
    await page("/prompts/1", fixtures.me.arjun);
    await screen.findByRole("list", { name: "Messages of version 3" });
    expect(screen.queryByRole("button", { name: /Save as version/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete template" })).toBeNull();
  });

  test("the session ends while a version is sent", async () => {
    keeps();
    const app = await page("/prompts/1", fixtures.me.maya);
    await screen.findByLabelText("Message 1");
    override("post", "/api/prompts/{id}/versions", unauthenticated);
    override("get", "/api/auth/me", unauthenticated);
    await submit("Save as version 4");
    await waitFor(() => {
      expect(href(app)).toContain("/sign-in");
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(toasts()).toEqual([]);
  });

  test("deleting asks first, and leaves for the list", async () => {
    const state = keeps();
    const app = await page("/prompts/1", fixtures.me.maya);
    await userEvent.click(await screen.findByRole("button", { name: "Delete template" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete summarize?" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(state.deleted).toEqual(["1"]);
    });
    await waitFor(() => {
      expect(href(app)).toBe("/prompts");
    });
    expect(toasts()).toEqual(["Template deleted."]);
  });

  test("a template that does not exist is not found, and an address that is no id asks nothing", async () => {
    keeps();
    await page("/prompts/77");
    expect(await screen.findByText("Page not found")).toBeInTheDocument();
    expectOneMain();
  });

  test("an address that is no id is not found", async () => {
    keeps();
    await page("/prompts/abc");
    expect(await screen.findByText("Page not found")).toBeInTheDocument();
  });

  test("a template whose latest version cannot be read says so", async () => {
    keeps([{ ...prompts.summarize, unreadable: true, model: null, variables: [] }]);
    override("get", "/api/prompts/{id}/versions/{version}", () => refuse(errors.internal_error));
    await page("/prompts/1", fixtures.me.maya);
    expect(await screen.findByText(/This version cannot be read/)).toBeInTheDocument();
  });
});
