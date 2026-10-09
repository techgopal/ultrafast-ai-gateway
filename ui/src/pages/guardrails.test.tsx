import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { ApiError } from "@/api/errors";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { noContent, ok, override, refuse } from "@/test/handlers";
import {
  choose,
  counted,
  expectLabelsNameControls,
  expectNoSecret,
  expectNotAvailable,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  href,
  installSelect,
  rowWithCell,
  SESSION_ENDED,
  settle,
  toasts,
} from "@/test/pages";
import { memberUser, renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

const { guardrails } = fixtures;

const HOOK_URL = "https://guard.example.test/services/T000/XXXXtokenXXXX";
const HOOK_TOKEN = "XXXXtokenXXXX";
const MAIL = "ada@example.com";

function page(path = "/guardrails", user?: fixtures.Me): Promise<AppRenderResult> {
  return renderWithApp(null, { route: path, ...(user === undefined ? {} : { user }) });
}

function readBody(body: unknown): Record<string, unknown> {
  return typeof body === "object" && body !== null ? Object.fromEntries(Object.entries(body)) : {};
}

/** A gateway that keeps the guardrails and changes them as it is told. */
function keeps(start: fixtures.Guardrail[] = fixtures.guardrailList) {
  const state = {
    list: [...start],
    lists: 0,
    created: [] as Record<string, unknown>[],
    patched: [] as { id: string | undefined; body: Record<string, unknown> }[],
    deleted: [] as (string | undefined)[],
    rotated: [] as (string | undefined)[],
    tested: [] as Record<string, unknown>[],
    refuseCreate: [] as ReturnType<typeof validationFailed>[],
    testAnswer: {
      redacted_text: "Write to [REDACTED:EMAIL].",
      outcome: { blocked_by: null, flags: [], redactions: { EMAIL: 1 } },
    } as {
      redacted_text: string;
      outcome: {
        blocked_by: { id: number; name: string } | null;
        flags: { guardrail_id: number; guardrail_name: string; rule_id: string }[];
        redactions: Record<string, number>;
      };
    },
  };
  override("get", "/api/guardrails", () => {
    state.lists += 1;
    return ok("get", "/api/guardrails", 200, { guardrails: state.list });
  });
  override("post", "/api/guardrails", async ({ request }) => {
    const body = readBody(await request.json());
    state.created.push(body);
    const refusal = state.refuseCreate.shift();
    if (refusal !== undefined) return refuse(refusal);
    const external = body.kind === "external";
    const made: fixtures.Guardrail = {
      ...guardrails.pii,
      id: 9,
      name: String(body.name),
      kind: String(body.kind),
      is_default: false,
      rules: [],
      url_host: external ? "https://guard.example.test" : null,
    };
    state.list = [...state.list, made];
    return ok("post", "/api/guardrails", 201, {
      guardrail: made,
      secret: external ? fixtures.newGuardrailSecret : null,
    });
  });
  override("patch", "/api/guardrails/{id}", async ({ request, params }) => {
    const body = readBody(await request.json());
    state.patched.push({ id: params.id, body });
    const one = state.list.find((g) => String(g.id) === params.id);
    if (one === undefined) return refuse(errors.not_found);
    const next: fixtures.Guardrail = {
      ...one,
      ...(typeof body.name === "string" ? { name: body.name } : {}),
      ...(typeof body.enabled === "boolean" ? { enabled: body.enabled } : {}),
    };
    state.list = state.list.map((g) => (g.id === next.id ? next : g));
    return ok("patch", "/api/guardrails/{id}", 200, next);
  });
  override("delete", "/api/guardrails/{id}", ({ params }) => {
    state.deleted.push(params.id);
    state.list = state.list.filter((g) => String(g.id) !== params.id);
    return noContent();
  });
  override("post", "/api/guardrails/{id}/rotate-secret", ({ params }) => {
    state.rotated.push(params.id);
    return ok("post", "/api/guardrails/{id}/rotate-secret", 200, {
      secret: fixtures.rotatedGuardrailSecret,
    });
  });
  override("post", "/api/guardrails/test", async ({ request }) => {
    state.tested.push(readBody(await request.json()));
    return ok("post", "/api/guardrails/test", 200, state.testAnswer);
  });
  return state;
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Guardrails" });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

async function paste(field: HTMLElement, text: string): Promise<void> {
  await userEvent.click(field);
  await userEvent.paste(text);
}

async function nameField(): Promise<HTMLElement> {
  return screen.findByLabelText("Name");
}

async function submit(label = "Create guardrail"): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: label }));
}

function rule(n: number): HTMLElement {
  return screen.getByRole("group", { name: `Rule ${String(n)}` });
}

describe("the list", () => {
  test("shows each guardrail with its kind, what it checks, where it applies and whether it is on", async () => {
    keeps();
    await page();
    const list = await table();
    expectOneMain();
    expectOneH1("Guardrails");
    expect(within(list).getAllByRole("columnheader").map((head) => head.textContent)).toEqual([
      "Name",
      "Kind",
      "Checks",
      "Applies to",
      "Enabled",
      "Actions",
    ]);
    const pii = rowWithCell("mask-emails");
    expect(pii).toHaveTextContent("Rules");
    expect(pii).toHaveTextContent("1 rule");
    expect(pii).toHaveTextContent("Every call");
    expect(within(pii).getByRole("switch", { name: "mask-emails" })).toBeChecked();

    const words = rowWithCell("house-rules");
    expect(words).toHaveTextContent("2 rules");
    expect(words).toHaveTextContent("Routes: support-chat");
    expect(words).toHaveTextContent("2 keys");

    const external = rowWithCell("acme-scanner");
    expect(external).toHaveTextContent("External");
    expect(external).toHaveTextContent("https://guard.example.test");
    expect(external).toHaveTextContent("Input and output, fails open");
    expect(external).toHaveTextContent("Nothing yet");
    // Rotating a secret is for external guardrails only.
    expect(within(external).getByRole("button", { name: "Rotate secret" })).toBeInTheDocument();
    expect(within(pii).queryByRole("button", { name: "Rotate secret" })).toBeNull();

    const imported = rowWithCell("imported-scanner");
    expect(imported).toHaveTextContent("Needs a URL");
    expect(within(imported).getByRole("switch", { name: "imported-scanner" })).toBeDisabled();
  });

  test("says so when there are none, and offers Add guardrail", async () => {
    keeps([]);
    await page();
    expect(await screen.findByText("No guardrails")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Add guardrail" })).toHaveAttribute("href", "/guardrails/new");
    expectOneH1("Guardrails");
  });

  test("explains how external guardrails work and how to verify a signature", async () => {
    keeps();
    await page();
    await table();
    const how = screen.getByText("How external guardrails work").closest("details");
    expect(how).not.toBeNull();
    expect(how).toHaveTextContent('"version": 1');
    // Every endpoint a call is logged under, and the test of a guardrail.
    expect(how).toHaveTextContent(
      "endpoint is chat, messages, responses, embeddings, images, transcriptions, translations, speech, playground, or test.",
    );
    expect(how).toHaveTextContent('{"action": "redact", "texts"');
    const verify = screen.getByText("How to verify signatures").closest("details");
    expect(verify).toHaveTextContent("Every request carries the header");
    expect(verify).toHaveTextContent("signing secret of the guardrail as the key");
  });

  test("is not available to a member, who also asks for nothing", async () => {
    const asked = counted("get", "/api/guardrails", () => refuse(errors.forbidden));
    await page("/guardrails", memberUser);
    await expectNotAvailable();
    expectOneMain();
    await settle();
    expect(asked.calls).toBe(0);
  });

  test("a list that fails shows the error with Retry", async () => {
    let fail = true;
    override("get", "/api/guardrails", () =>
      fail
        ? refuse(errors.internal_error)
        : ok("get", "/api/guardrails", 200, { guardrails: fixtures.guardrailList }),
    );
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    fail = false;
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await table();
    expectOneH1("Guardrails");
  });

  test("the session ends while the list is open", async () => {
    keeps();
    const app = await page();
    await table();
    override("get", "/api/guardrails", unauthenticated);
    override("get", "/api/auth/me", unauthenticated);
    await act(async () => {
      await app.queryClient.invalidateQueries({ queryKey: ["guardrails"] });
    });
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/guardrails")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
  });

  test("the switch turns a guardrail off, sends only that, and the list shows it afterwards", async () => {
    const state = keeps();
    await page();
    await table();
    await userEvent.click(within(rowWithCell("mask-emails")).getByRole("switch"));
    await waitFor(() => {
      expect(state.patched).toEqual([{ id: "1", body: { enabled: false } }]);
    });
    await waitFor(() => {
      expect(within(rowWithCell("mask-emails")).getByRole("switch")).not.toBeChecked();
    });
    expect(state.lists).toBeGreaterThan(1);
  });

  test("a switch that fails says why", async () => {
    keeps();
    override("patch", "/api/guardrails/{id}", () => refuse(errors.internal_error));
    await page();
    await table();
    await userEvent.click(within(rowWithCell("mask-emails")).getByRole("switch"));
    expect(await screen.findByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    expect(within(rowWithCell("mask-emails")).getByRole("switch")).toBeChecked();
  });

  test("deleting asks first, says what stops being checked, and the list shows it afterwards", async () => {
    const state = keeps();
    await page();
    await table();
    await userEvent.click(within(rowWithCell("house-rules")).getByRole("button", { name: "Delete" }));
    const question = await screen.findByRole("alertdialog");
    expect(question).toHaveTextContent("Delete house-rules?");
    expect(question).toHaveTextContent(
      "Calls stop being checked by it on the routes support-chat, 2 keys. This cannot be undone.",
    );
    await userEvent.click(within(question).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(state.deleted).toEqual(["2"]);
    });
    await waitFor(() => {
      expect(screen.queryByText("house-rules")).toBeNull();
    });
    expect(toasts()).toEqual(["Guardrail deleted."]);
  });

  test("a guardrail that is attached to nothing says so when it is deleted", async () => {
    keeps();
    await page();
    await table();
    await userEvent.click(within(rowWithCell("acme-scanner")).getByRole("button", { name: "Delete" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("It is attached to nothing.");
  });

  test("rotating a secret shows the new one once and nowhere after", async () => {
    const state = keeps();
    const app = await page();
    await table();
    await userEvent.click(within(rowWithCell("acme-scanner")).getByRole("button", { name: "Rotate secret" }));
    const question = await screen.findByRole("alertdialog");
    expect(question).toHaveTextContent("The old secret stops working at once.");
    await userEvent.click(within(question).getByRole("button", { name: "Rotate secret" }));
    const dialog = await screen.findByRole("dialog", { name: "New signing secret" });
    expect(dialog).toHaveTextContent("Copy this signing secret now. It is not shown again.");
    expect(within(dialog).getByDisplayValue(fixtures.rotatedGuardrailSecret)).toBeInTheDocument();
    expect(state.rotated).toEqual(["3"]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Done" }));
    const sure = await screen.findByRole("alertdialog");
    await userEvent.click(within(sure).getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expectNoSecret(app, fixtures.rotatedGuardrailSecret);
  });
});

describe("a new guardrail", () => {
  test("starts as a rules guardrail with one PII rule, and is made with it", async () => {
    const state = keeps();
    const app = await page("/guardrails/new");
    expectOneMain();
    expectOneH1("New guardrail");
    await paste(await nameField(), "mask-phones");
    const first = rule(1);
    expect(within(first).getByRole("checkbox", { name: /^Email address/ })).toBeChecked();
    await userEvent.click(within(first).getByRole("checkbox", { name: /^Phone number/ }));
    await submit();
    await waitFor(() => {
      expect(state.created).toEqual([
        {
          name: "mask-phones",
          description: "",
          kind: "rules",
          enabled: true,
          is_default: false,
          rules: [
            { id: "email", matcher: { pii: ["EMAIL", "PHONE"] }, action: "redact", directions: "both" },
          ],
        },
      ]);
    });
    await waitFor(() => {
      expect(href(app)).toBe("/guardrails");
    });
    expect(toasts()).toEqual(["Guardrail created."]);
    // The list shows it afterwards.
    expect(await screen.findByText("mask-phones")).toBeInTheDocument();
  });

  test("keywords and a regular expression are written as the gateway takes them", async () => {
    const state = keeps();
    await page("/guardrails/new");
    await paste(await nameField(), "words");
    await paste(screen.getByLabelText("Rule id"), "x");
    await choose(within(rule(1)).getByRole("combobox", { name: "Matches" }), "Keywords");
    await paste(within(rule(1)).getByLabelText("Words"), "swordfish\n  project x \n\n");
    await choose(within(rule(1)).getByRole("combobox", { name: "Action" }), "Block");
    await choose(within(rule(1)).getByRole("combobox", { name: "Applies to" }), "Input only");
    await userEvent.click(within(rule(1)).getByRole("switch", { name: "Whole words only" }));
    await userEvent.click(screen.getByRole("button", { name: "Add rule" }));
    await paste(within(rule(2)).getByLabelText("Rule id"), "ticket");
    await choose(within(rule(2)).getByRole("combobox", { name: "Matches" }), "Regular expression");
    await paste(within(rule(2)).getByLabelText("Regular expression"), "TICKET-[0-9]+");
    await choose(within(rule(2)).getByRole("combobox", { name: "Action" }), "Flag");
    await userEvent.click(screen.getByRole("switch", { name: "Applies to every call" }));
    await submit();
    await waitFor(() => {
      expect(state.created).toHaveLength(1);
    });
    expect(state.created[0]).toMatchObject({
      is_default: true,
      rules: [
        {
          id: "emailx",
          matcher: { keywords: { words: ["swordfish", "project x"], whole_word: false } },
          action: "block",
          directions: "input",
        },
        { id: "ticket", matcher: { regex: "TICKET-[0-9]+" }, action: "flag", directions: "both" },
      ],
    });
  });

  test("an external guardrail needs a URL, shows its secret once, and the URL is nowhere after", async () => {
    const state = keeps();
    const app = await page("/guardrails/new");
    await paste(await nameField(), "scanner");
    await userEvent.click(screen.getByRole("radio", { name: "External webhook" }));
    expect(screen.queryByRole("group", { name: "Rule 1" })).toBeNull();
    await submit();
    expect(await screen.findByText("Enter the URL to post to.")).toBeInTheDocument();
    expect(state.created).toEqual([]);

    await paste(screen.getByLabelText(/^URL/), HOOK_URL);
    await userEvent.click(screen.getByRole("radio", { name: /^Closed/ }));
    await userEvent.click(screen.getByRole("radio", { name: "Output only" }));
    await userEvent.clear(screen.getByLabelText("Timeout (ms)"));
    await paste(screen.getByLabelText("Timeout (ms)"), "5000");
    await submit();
    const dialog = await screen.findByRole("dialog", { name: "Signing secret" });
    expect(state.created).toEqual([
      {
        name: "scanner",
        description: "",
        kind: "external",
        enabled: true,
        is_default: false,
        url: HOOK_URL,
        directions: "output",
        fail_mode: "closed",
        timeout_ms: 5000,
      },
    ]);
    expect(within(dialog).getByDisplayValue(fixtures.newGuardrailSecret)).toBeInTheDocument();
    expect(dialog).toHaveTextContent("Copy this signing secret now. It is not shown again.");
    expect(href(app)).toBe("/guardrails/new");
    await userEvent.click(within(dialog).getByRole("button", { name: "Done" }));
    const sure = await screen.findByRole("alertdialog");
    await userEvent.click(within(sure).getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(href(app)).toBe("/guardrails");
    });
    expect(toasts()).toEqual(["Guardrail created."]);
    expectNoSecret(app, fixtures.newGuardrailSecret);
    expectNoSecret(app, HOOK_TOKEN);
  });

  test("a form that is not complete says what is missing and sends nothing", async () => {
    const state = keeps();
    await page("/guardrails/new");
    await nameField();
    await paste(screen.getByLabelText("Rule id"), "");
    await userEvent.clear(screen.getByLabelText("Rule id"));
    await userEvent.click(within(rule(1)).getByRole("checkbox", { name: /^Email address/ }));
    await submit();
    expect(await screen.findByText("Enter a name.")).toBeInTheDocument();
    expect(screen.getByText("Enter an id for the rule.")).toBeInTheDocument();
    expect(screen.getByText("Some fields are not valid. They are marked below.")).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveFocus();
    expect(state.created).toEqual([]);
    // It is said while it is mended.
    await paste(screen.getByLabelText("Name"), "n");
    await waitFor(() => {
      expect(screen.queryByText("Enter a name.")).toBeNull();
    });
    expect(screen.getByText("Enter an id for the rule.")).toBeInTheDocument();
  });

  test("a rule needs something to look for, and an id of its own", async () => {
    keeps();
    await page("/guardrails/new");
    await paste(await nameField(), "n");
    await userEvent.click(within(rule(1)).getByRole("checkbox", { name: /^Email address/ }));
    await userEvent.click(screen.getByRole("button", { name: "Add rule" }));
    await paste(within(rule(2)).getByLabelText("Rule id"), "email");
    await choose(within(rule(2)).getByRole("combobox", { name: "Matches" }), "Keywords");
    await submit();
    expect(await within(rule(2)).findByText("Enter at least one word.")).toBeInTheDocument();
    await paste(within(rule(2)).getByLabelText("Words"), "swordfish");
    expect(await within(rule(1)).findByText("Choose at least one type.")).toBeInTheDocument();
    expect(await within(rule(2)).findByText("Another rule has this id.")).toBeInTheDocument();
    // Removing the rule takes its problem with it.
    await userEvent.click(within(rule(2)).getByRole("button", { name: "Remove rule 2" }));
    expect(screen.queryByRole("group", { name: "Rule 2" })).toBeNull();
  });

  test("a rule the gateway cannot compile is marked on its row", async () => {
    const state = keeps();
    state.refuseCreate.push(
      validationFailed({
        "rules[1]": "regex parse error: unclosed group",
        "rules[0].types": fieldMessages.guardrailPiiType,
      }),
    );
    await page("/guardrails/new");
    await paste(await nameField(), "n");
    await userEvent.click(screen.getByRole("button", { name: "Add rule" }));
    await paste(within(rule(2)).getByLabelText("Rule id"), "re");
    await choose(within(rule(2)).getByRole("combobox", { name: "Matches" }), "Regular expression");
    await paste(within(rule(2)).getByLabelText("Regular expression"), "(a");
    await submit();
    expect(await within(rule(2)).findByText("regex parse error: unclosed group")).toBeInTheDocument();
    expect(within(rule(1)).getByText(fieldMessages.guardrailPiiType)).toBeInTheDocument();
    expect(within(rule(2)).getByLabelText("Regular expression")).toHaveAttribute("aria-invalid", "true");
    // Mending the rules takes the gateway's word away.
    await paste(within(rule(2)).getByLabelText("Regular expression"), ")");
    await waitFor(() => {
      expect(screen.queryByText("regex parse error: unclosed group")).toBeNull();
    });
    expect(toasts()).toEqual([]);
  });

  test("a name that is taken is said on the name", async () => {
    const state = keeps();
    state.refuseCreate.push(errors.guardrail_exists);
    await page("/guardrails/new");
    await paste(await nameField(), "mask-emails");
    await submit();
    expect(await screen.findByText(errors.guardrail_exists.body.error.message)).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveAttribute("aria-invalid", "true");
    expect(toasts()).toEqual([]);
  });

  test("the labels name the controls", async () => {
    keeps();
    await page("/guardrails/new");
    await nameField();
    expectLabelsNameControls(screen.getByRole("form", { name: "Guardrail" }));
  });

  test("the session ends while the form is sent", async () => {
    keeps();
    const app = await page("/guardrails/new");
    await paste(await nameField(), "n");
    override("post", "/api/guardrails", unauthenticated);
    override("get", "/api/auth/me", unauthenticated);
    await submit();
    await waitFor(() => {
      expect(href(app)).toContain("/sign-in");
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(toasts()).toEqual([]);
  });

  test("is not available to a member", async () => {
    await page("/guardrails/new", memberUser);
    await expectNotAvailable();
    expectOneMain();
  });
});

describe("an existing guardrail", () => {
  test("opens with its rules, and sends what changed as the whole of it", async () => {
    const state = keeps();
    const app = await page("/guardrails/2");
    expectOneH1("Edit guardrail");
    expect(await screen.findByDisplayValue("house-rules")).toBeInTheDocument();
    // Where it is attached.
    expect(screen.getByText(/On the routes support-chat, and on 2 keys\./)).toBeInTheDocument();
    expect(within(rule(1)).getByLabelText("Words")).toHaveValue("swordfish\nproject x");
    expect(within(rule(2)).getByLabelText("Regular expression")).toHaveValue("TICKET-[0-9]+");
    // The kind is chosen once.
    expect(screen.queryByRole("radio", { name: "External webhook" })).toBeNull();
    await userEvent.click(within(rule(2)).getByRole("button", { name: "Remove rule 2" }));
    await submit("Save guardrail");
    await waitFor(() => {
      expect(state.patched).toHaveLength(1);
    });
    expect(state.patched[0]?.id).toBe("2");
    expect(state.patched[0]?.body).toEqual({
      name: "house-rules",
      description: "",
      enabled: true,
      is_default: false,
      rules: [guardrails.words.rules[0]],
    });
    await waitFor(() => {
      expect(href(app)).toBe("/guardrails");
    });
    expect(toasts()).toEqual(["Guardrail saved."]);
  });

  test("an external guardrail keeps its URL unless one is typed, and never shows it", async () => {
    const state = keeps();
    const app = await page("/guardrails/3");
    expect(await screen.findByDisplayValue("acme-scanner")).toBeInTheDocument();
    const url = screen.getByLabelText(/^URL/);
    expect(url).toHaveValue("");
    expect(screen.getByText(/Leave empty to keep the current URL/)).toHaveTextContent(
      "It posts to https://guard.example.test.",
    );
    expect(screen.getByLabelText("Timeout (ms)")).toHaveValue("3000");
    await userEvent.click(screen.getByRole("radio", { name: /^Closed/ }));
    await submit("Save guardrail");
    await waitFor(() => {
      expect(state.patched).toHaveLength(1);
    });
    expect(state.patched[0]?.body).toEqual({
      name: "acme-scanner",
      description: "The security team's scanner.",
      enabled: true,
      is_default: false,
      directions: "both",
      fail_mode: "closed",
      timeout_ms: 3000,
    });
    await waitFor(() => {
      expect(href(app)).toBe("/guardrails");
    });
  });

  test("a new URL is sent once and is not kept", async () => {
    const state = keeps();
    const app = await page("/guardrails/3");
    await paste(await screen.findByLabelText(/^URL/), HOOK_URL);
    await submit("Save guardrail");
    await waitFor(() => {
      expect(state.patched[0]?.body.url).toBe(HOOK_URL);
    });
    await waitFor(() => {
      expect(href(app)).toBe("/guardrails");
    });
    expectNoSecret(app, HOOK_TOKEN);
  });

  test("a guardrail without a URL says so", async () => {
    keeps();
    await page("/guardrails/4");
    expect(await screen.findByDisplayValue("imported-scanner")).toBeInTheDocument();
    expect(screen.getAllByText(/This guardrail has no URL yet/).length).toBeGreaterThan(0);
  });

  test("one that is not there is not found, and so is a number that is not an id", async () => {
    keeps();
    await page("/guardrails/99");
    expect(await screen.findByRole("heading", { name: "Page not found" })).toBeInTheDocument();
    expectOneMain();
  });

  test("an address that is not an id asks for nothing", async () => {
    const asked = counted("get", "/api/guardrails", () => ok("get", "/api/guardrails", 200, { guardrails: [] }));
    await page("/guardrails/abc");
    expect(await screen.findByRole("heading", { name: "Page not found" })).toBeInTheDocument();
    expect(asked.calls).toBe(0);
  });
});

describe("Try it", () => {
  test("checks a text with the rules of the form and marks the placeholders, without showing what matched", async () => {
    const state = keeps();
    await page("/guardrails/new");
    await paste(await nameField(), "n");
    const text = screen.getByLabelText("Text to check");
    await paste(text, `Write to ${MAIL}.`);
    await choose(screen.getByRole("combobox", { name: "Check as" }), "Output of a call");
    await userEvent.click(screen.getByRole("button", { name: "Check the text" }));
    await waitFor(() => {
      expect(state.tested).toEqual([
        {
          direction: "output",
          text: `Write to ${MAIL}.`,
          rules: [{ id: "email", matcher: { pii: ["EMAIL"] }, action: "redact", directions: "both" }],
        },
      ]);
    });
    const result = await screen.findByRole("group", { name: "Text after the guardrail" });
    const marks = [...result.querySelectorAll("mark")].map((mark) => mark.textContent);
    expect(marks).toEqual(["[REDACTED:EMAIL]"]);
    expect(result).toHaveTextContent("Write to [REDACTED:EMAIL].");
    expect(screen.getByRole("list", { name: "What was found" })).toHaveTextContent("Redacted: EMAIL 1.");
    // The address is in the box the admin typed in, and nowhere else.
    const where = [...document.body.querySelectorAll("*")].filter(
      (el) =>
        el.children.length === 0 && el.tagName !== "TEXTAREA" && el.textContent.includes(MAIL),
    );
    expect(where).toEqual([]);
  });

  test("a text that is blocked or flagged is said so", async () => {
    const state = keeps();
    state.testAnswer = {
      redacted_text: "swordfish",
      outcome: {
        blocked_by: { id: 0, name: "Test rules" },
        flags: [{ guardrail_id: 0, guardrail_name: "Test rules", rule_id: "ticket" }],
        redactions: {},
      },
    };
    await page("/guardrails/2");
    await screen.findByDisplayValue("house-rules");
    await paste(screen.getByLabelText("Text to check"), "swordfish TICKET-1");
    await userEvent.click(screen.getByRole("button", { name: "Check the text" }));
    const found = await screen.findByRole("list", { name: "What was found" });
    expect(found).toHaveTextContent("Blocked by Test rules.");
    expect(found).toHaveTextContent("Flagged: ticket.");
  });

  test("rules that do not compile are the gateway's to say", async () => {
    keeps();
    override("post", "/api/guardrails/test", () =>
      refuse(validationFailed({ "rules[0]": "regex parse error: unclosed group" })),
    );
    await page("/guardrails/new");
    await paste(await nameField(), "n");
    await paste(screen.getByLabelText("Text to check"), "a");
    await userEvent.click(screen.getByRole("button", { name: "Check the text" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("rules[0]: regex parse error: unclosed group");
  });

  test("is off while the text is empty or a rule is incomplete", async () => {
    keeps();
    await page("/guardrails/new");
    await nameField();
    const button = screen.getByRole("button", { name: "Check the text" });
    expect(button).toBeDisabled();
    await paste(screen.getByLabelText("Text to check"), "a");
    expect(button).toBeEnabled();
    await userEvent.click(within(rule(1)).getByRole("checkbox", { name: /^Email address/ }));
    expect(button).toBeDisabled();
    expect(screen.getByText("Fix the rules marked above to try them.")).toBeInTheDocument();
  });

  test("an external guardrail is called only when it is saved, and the text is sent to it", async () => {
    const state = keeps();
    await page("/guardrails/3");
    await screen.findByDisplayValue("acme-scanner");
    expect(screen.getByText(/This sends the text to the guardrail's URL/)).toBeInTheDocument();
    await paste(screen.getByLabelText("Text to check"), "hello");
    await userEvent.click(screen.getByRole("button", { name: "Send to the guardrail" }));
    await waitFor(() => {
      expect(state.tested).toEqual([
        { direction: "input", text: "hello", guardrail_id: 3, call_external: true },
      ]);
    });
    expect(await screen.findByRole("list", { name: "What was found" })).toBeInTheDocument();
  });

  test("a new external guardrail cannot be tried before it is saved", async () => {
    keeps();
    await page("/guardrails/new");
    await paste(await nameField(), "n");
    await userEvent.click(screen.getByRole("radio", { name: "External webhook" }));
    await paste(screen.getByLabelText("Text to check"), "hello");
    expect(screen.getByRole("button", { name: "Send to the guardrail" })).toBeDisabled();
    expect(screen.getByText(/Save the guardrail to try it/)).toBeInTheDocument();
  });

  test("a failed call of an external guardrail is said as a failure", async () => {
    const state = keeps();
    state.testAnswer = {
      redacted_text: "hello",
      outcome: {
        blocked_by: null,
        flags: [{ guardrail_id: 3, guardrail_name: "acme-scanner", rule_id: "external_error:timeout" }],
        redactions: {},
      },
    };
    await page("/guardrails/3");
    await screen.findByDisplayValue("acme-scanner");
    await paste(screen.getByLabelText("Text to check"), "hello");
    await userEvent.click(screen.getByRole("button", { name: "Send to the guardrail" }));
    expect(await screen.findByRole("list", { name: "What was found" })).toHaveTextContent(
      "acme-scanner could not be asked: timeout.",
    );
  });

  test("the typed text is not kept in the caches", async () => {
    keeps();
    const app = await page("/guardrails/new");
    await paste(await nameField(), "n");
    await paste(screen.getByLabelText("Text to check"), `mail ${MAIL}`);
    await userEvent.click(screen.getByRole("button", { name: "Check the text" }));
    await screen.findByRole("group", { name: "Text after the guardrail" });
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
  });
});

describe("the error type", () => {
  test("a refusal keeps what the gateway said", () => {
    const error = new ApiError(409, "guardrail_exists", errors.guardrail_exists.body.error.message);
    expect(error.fields).toEqual({});
  });
});
