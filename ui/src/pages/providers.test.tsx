import type { QueryClient } from "@tanstack/react-query";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HttpResponse } from "msw";
import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";
import { queryKeys } from "@/api/queries";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, noContent, ok, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  cached,
  counted,
  descriptionOf,
  expectNoSecret,
  expectNotAvailable,
  expectOneMain,
  expectSessionEndsOnPage,
  forbid,
  forgetToasts,
  href,
  installPointerCapture,
  listenToConsole,
  SESSION_ENDED,
  settle,
  shown,
  toasts,
} from "@/test/pages";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

const { withCredential, withoutCredential } = fixtures.providers;

/** A made-up API key of a provider. It works nowhere. */
const API_KEY = "sk-made-up-0123456789abcdef-works-nowhere";

const NAME_HINT = "lowercase letters, digits, - and _";
const V1_HINT = "The base URL of an OpenAI-compatible provider usually ends in /v1.";
const DELETE = "Calls to models of this provider will fail at once.";
const ENTER_AGAIN = "The API key was cleared. Enter it again.";

/** The known base URLs of the brief, with the kind each one is of. */
const KNOWN = [
  ["OpenAI", "https://api.openai.com/v1", "OpenAI-compatible"],
  ["Anthropic", "https://api.anthropic.com", "Anthropic"],
  ["Groq", "https://api.groq.com/openai/v1", "OpenAI-compatible"],
  ["Mistral", "https://api.mistral.ai/v1", "OpenAI-compatible"],
  ["OpenRouter", "https://openrouter.ai/api/v1", "OpenAI-compatible"],
  ["Ollama", "http://localhost:11434/v1", "OpenAI-compatible"],
] as const;

// The tests drive dialogs step by step. On a busy machine one can take longer
// than the 5 seconds a test has by default.
vi.setConfig({ testTimeout: 20_000 });

beforeAll(installPointerCapture);
afterEach(forgetToasts);

type Options = { user?: fixtures.Me; width?: number; queryClient?: QueryClient };

function page(options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/providers", ...options });
}

function providersAre(list: readonly fixtures.Provider[]) {
  return counted("get", "/api/providers", () =>
    ok("get", "/api/providers", 200, { providers: [...list] }),
  );
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Providers" });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

function rowOf(name: string): HTMLElement {
  const row = screen.getByRole("cell", { name }).closest("tr");
  if (row === null) throw new Error(`no row for ${name}`);
  return row;
}

function names(scope: HTMLElement): string[] {
  return within(scope)
    .queryAllByRole("button")
    .map((control) => control.textContent);
}

function read(body: unknown, name: string): unknown {
  return typeof body === "object" && body !== null ? Reflect.get(body, name) : undefined;
}

/** A gateway that keeps the providers and changes them as it is told. */
function keeps(start: readonly fixtures.Provider[] = fixtures.providerList) {
  const state = {
    providers: [...start],
    lists: 0,
    created: [] as unknown[],
    patched: [] as { id: string | undefined; body: unknown }[],
    deleted: [] as (string | undefined)[],
  };
  override("get", "/api/providers", () => {
    state.lists += 1;
    return ok("get", "/api/providers", 200, { providers: state.providers });
  });
  override("post", "/api/providers", async ({ request }) => {
    const body: unknown = await request.json();
    state.created.push(body);
    const provider: fixtures.Provider = {
      id: 9,
      name: String(read(body, "name")),
      kind: String(read(body, "kind")),
      base_url: String(read(body, "base_url")),
      has_credential: typeof read(body, "api_key") === "string",
    };
    state.providers = [...state.providers, provider];
    return ok("post", "/api/providers", 201, provider);
  });
  override("patch", "/api/providers/{id}", async ({ request, params }) => {
    const body: unknown = await request.json();
    state.patched.push({ id: params.id, body });
    const was = state.providers.find((one) => String(one.id) === params.id);
    if (was === undefined) return refuse(errors.not_found);
    const url = read(body, "base_url");
    const key = read(body, "api_key");
    const now: fixtures.Provider = {
      ...was,
      base_url: typeof url === "string" ? url : was.base_url,
      has_credential: key === undefined ? was.has_credential : key !== null,
    };
    state.providers = state.providers.map((one) => (one.id === was.id ? now : one));
    return ok("patch", "/api/providers/{id}", 200, now);
  });
  override("delete", "/api/providers/{id}", ({ params }) => {
    state.deleted.push(params.id);
    state.providers = state.providers.filter((one) => String(one.id) !== params.id);
    return noContent();
  });
  return state;
}

async function closed(): Promise<void> {
  await waitFor(() => {
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
}

async function openAdd(): Promise<HTMLElement> {
  await userEvent.click(await screen.findByRole("button", { name: "Add provider" }));
  return screen.findByRole("dialog", { name: "Add provider" });
}

async function openEdit(provider: fixtures.Provider): Promise<HTMLElement> {
  await table();
  await userEvent.click(within(rowOf(provider.name)).getByRole("button", { name: "Edit" }));
  return screen.findByRole("dialog", { name: "Edit provider" });
}

async function askToDelete(provider: fixtures.Provider): Promise<HTMLElement> {
  await table();
  await userEvent.click(within(rowOf(provider.name)).getByRole("button", { name: "Delete" }));
  return screen.findByRole("alertdialog", { name: `Delete ${provider.name}?` });
}

function known(dialog: HTMLElement, name: string): Promise<void> {
  const group = within(dialog).getByRole("group", { name: "Known providers" });
  return userEvent.click(within(group).getByRole("button", { name }));
}

function kind(dialog: HTMLElement, name: "OpenAI-compatible" | "Anthropic"): HTMLElement {
  const group = within(dialog).getByRole("radiogroup", { name: "Kind" });
  return within(group).getByRole("radio", { name });
}

/** Puts the text into the field at once, as a paste does. */
async function enter(field: HTMLElement, text: string): Promise<void> {
  await userEvent.click(field);
  await userEvent.paste(text);
}

function add(dialog: HTMLElement): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: "Add provider" }));
}

function save(dialog: HTMLElement): Promise<void> {
  return userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
}

/** The key is in none of: the document, the caches, the router, the browser. */
function expectNoKey(app: AppRenderResult): void {
  expectNoSecret(app, API_KEY);
}

describe("the list of providers", () => {
  test("the list shows the name, the kind, the base URL and whether a credential is set", async () => {
    await page();
    const providers = await table();
    expect(screen.getByRole("heading", { level: 1, name: "Providers" })).toBeInTheDocument();
    expect(within(providers).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Name",
      "Kind",
      "Base URL",
      "Credential",
      "Actions",
    ]);
    const first = within(rowOf(withCredential.name)).getAllByRole("cell");
    expect(first.slice(0, 4).map((cell) => cell.textContent)).toEqual([
      withCredential.name,
      "OpenAI-compatible",
      withCredential.base_url,
      "Set",
    ]);
    const second = within(rowOf(withoutCredential.name)).getAllByRole("cell");
    expect(second.slice(0, 4).map((cell) => cell.textContent)).toEqual([
      withoutCredential.name,
      "OpenAI-compatible",
      withoutCredential.base_url,
      "None",
    ]);
    expectOneMain();
  });

  test("providers list hides admin controls from others", async () => {
    const first = await page();
    await table();
    expect(screen.getByRole("button", { name: "Add provider" })).toBeInTheDocument();
    for (const provider of fixtures.providerList) {
      expect(names(rowOf(provider.name))).toEqual(["Edit", "Delete"]);
    }
    first.unmount();

    for (const me of [fixtures.me.arjun, fixtures.me.lena, fixtures.me.priya]) {
      const app = await page({ user: me });
      const providers = await table();
      // Everyone sees the list.
      for (const provider of fixtures.providerList) {
        expect(rowOf(provider.name)).toHaveTextContent(provider.base_url);
      }
      expect(rowOf(withCredential.name)).toHaveTextContent("Set");
      expect(rowOf(withoutCredential.name)).toHaveTextContent("None");
      // Only admins see Add, Edit and Delete.
      expect(screen.queryByRole("button", { name: "Add provider" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
      expect(within(providers).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
        "Name",
        "Kind",
        "Base URL",
        "Credential",
      ]);
      app.unmount();
    }
  });

  test("the kinds are named, and one the console does not know is shown as it is", async () => {
    // The API description has the kind as a text; a later gateway may know more.
    override("get", "/api/providers", () =>
      HttpResponse.json({
        providers: [
          withCredential,
          { ...withCredential, id: 3, name: "claude", kind: "anthropic" },
          { ...withCredential, id: 4, name: "aws", kind: "bedrock" },
        ],
      }),
    );
    await page();
    await table();
    expect(within(rowOf("claude")).getByText("Anthropic")).toHaveAttribute("data-slot", "badge");
    const unknown = within(rowOf("aws")).getByText("bedrock");
    expect(unknown).toHaveAttribute("data-slot", "badge");
    expect(unknown).toHaveAttribute("data-variant", "outline");
  });

  test("the credential is said in words", async () => {
    await page();
    await table();
    // Never by a colour alone, and never the credential itself: the API does not give it.
    expect(within(rowOf(withCredential.name)).getByText("Set")).toBeInTheDocument();
    expect(within(rowOf(withoutCredential.name)).getByText("None")).toBeInTheDocument();
  });

  test("loading shows skeleton rows", async () => {
    const door = gate();
    override("get", "/api/providers", async () => {
      await door.opened;
      return ok("get", "/api/providers", 200, { providers: fixtures.providerList });
    });
    await page();
    const providers = screen.getByRole("table", { name: "Providers" });
    expect(providers).toHaveAttribute("aria-busy", "true");
    expect(providers.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(screen.queryByText(withCredential.base_url)).toBeNull();
    act(() => {
      door.open();
    });
    await table();
    expect(providers.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0);
  });

  test("an empty list says so", async () => {
    providersAre([]);
    await page();
    expect(await screen.findByRole("heading", { name: "No providers" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.getByRole("button", { name: "Add provider" })).toBeInTheDocument();
  });

  test("a failed list call shows the error with Retry, and Retry asks again", async () => {
    const failing = counted("get", "/api/providers", () => refuse(errors.internal_error));
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    expect(screen.queryByRole("table")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(failing.calls).toBe(1);
    expect(screen.queryByRole("button", { name: "Add provider" })).toBeNull();

    const again = providersAre(fixtures.providerList);
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await table();
    expect(again.calls).toBe(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "Add provider" })).toBeInTheDocument();
  });

  test("a gateway that cannot be reached is an error with Retry", async () => {
    override("get", "/api/providers", networkFailure);
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  test("at width 390 the rows are cards with their labels and actions", async () => {
    await page({ width: 390 });
    const cards = await screen.findByRole("list", { name: "Providers" });
    await waitFor(() => {
      expect(cards).toHaveAttribute("aria-busy", "false");
    });
    expect(screen.queryByRole("table")).toBeNull();
    const items = within(cards).getAllByRole("listitem");
    expect(items).toHaveLength(fixtures.providerList.length);
    const [first] = items;
    if (first === undefined) throw new Error("no card");
    expect([...first.querySelectorAll("dt")].map((label) => label.textContent)).toEqual([
      "Name",
      "Kind",
      "Base URL",
      "Credential",
    ]);
    expect(names(first)).toEqual(["Edit", "Delete"]);
    for (const control of screen.getAllByRole("button", { name: /Add provider|Edit|Delete/ })) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
  });

  test("the list call answers 403: not available", async () => {
    forbid("/api/providers");
    await page();
    await expectNotAvailable();
    expectOneMain();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("button", { name: "Add provider" })).toBeNull();
  });

  test("the session ends while the list is open", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    await table();
    await expectSessionEndsOnPage(app, {
      path: "/api/providers",
      queryKey: queryKeys.providers.list(),
      at: "/providers",
    });
    expect(shown()).not.toContain(withCredential.base_url);
  });
});

describe("adding a provider", () => {
  test("a provider is added, and the page says how to call it", async () => {
    const written = listenToConsole();
    const state = keeps();
    const app = await page();
    await table();
    expect(screen.queryByRole("status")).toBeNull();
    const dialog = await openAdd();
    await userEvent.type(within(dialog).getByLabelText("Name"), "groq");
    await known(dialog, "Groq");
    await enter(within(dialog).getByLabelText("API key"), API_KEY);
    await add(dialog);
    await closed();
    expect(state.created).toEqual([
      {
        name: "groq",
        kind: "openai",
        base_url: "https://api.groq.com/openai/v1",
        api_key: API_KEY,
      },
    ]);

    // Model names are `<provider name>/<model>`.
    const notice = await screen.findByRole("status");
    expect(notice).toHaveTextContent("Provider added");
    expect(notice).toHaveTextContent("groq/<model>");
    expect(toasts()).toEqual([]);
    // The list was asked for again, and has the provider.
    await waitFor(() => {
      expect(rowOf("groq")).toHaveTextContent("Set");
    });
    expect(state.lists).toBe(2);
    expectNoKey(app);
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });

    await userEvent.click(within(notice).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("status")).toBeNull();
    // Nothing was written to the console about the key.
    expect(written()).not.toContain(API_KEY);
  });

  test("without an API key none is sent", async () => {
    const state = keeps();
    await page();
    const dialog = await openAdd();
    await userEvent.type(within(dialog).getByLabelText("Name"), "ollama");
    await known(dialog, "Ollama");
    await add(dialog);
    await closed();
    expect(state.created).toEqual([
      { name: "ollama", kind: "openai", base_url: "http://localhost:11434/v1" },
    ]);
    const [body] = state.created;
    expect(Object.keys(body as object)).not.toContain("api_key");
    await waitFor(() => {
      expect(rowOf("ollama")).toHaveTextContent("None");
    });
  });

  test("an Anthropic provider is sent with its kind", async () => {
    const state = keeps();
    await page();
    const dialog = await openAdd();
    await userEvent.type(within(dialog).getByLabelText("Name"), "claude");
    await userEvent.click(kind(dialog, "Anthropic"));
    await userEvent.type(within(dialog).getByLabelText("Base URL"), "https://llm.example.test");
    await add(dialog);
    await closed();
    expect(state.created).toEqual([
      { name: "claude", kind: "anthropic", base_url: "https://llm.example.test" },
    ]);
  });

  test("known base URLs fill the field", async () => {
    await page();
    const dialog = await openAdd();
    const url = within(dialog).getByLabelText("Base URL");
    expect(url).toHaveValue("");
    expect(kind(dialog, "OpenAI-compatible")).toBeChecked();
    const group = within(dialog).getByRole("group", { name: "Known providers" });
    expect(names(group)).toEqual(KNOWN.map(([name]) => name));

    await known(dialog, "Anthropic");
    expect(url).toHaveValue("https://api.anthropic.com");
    expect(kind(dialog, "Anthropic")).toBeChecked();
    // Choosing Groq sets the URL and the kind to OpenAI-compatible.
    await known(dialog, "Groq");
    expect(url).toHaveValue("https://api.groq.com/openai/v1");
    expect(kind(dialog, "OpenAI-compatible")).toBeChecked();
    expect(kind(dialog, "Anthropic")).not.toBeChecked();

    for (const [name, address, of] of KNOWN) {
      await known(dialog, name);
      expect(url).toHaveValue(address);
      expect(kind(dialog, of)).toBeChecked();
    }
    // The choice fills the field; the field can still be changed.
    await userEvent.clear(url);
    await userEvent.type(url, "http://llm.internal.example.test/v1");
    expect(url).toHaveValue("http://llm.internal.example.test/v1");
  });

  test("the hints of the form", async () => {
    await page();
    const dialog = await openAdd();
    expect(descriptionOf(within(dialog).getByLabelText("Name"))).toBe(NAME_HINT);
    const url = within(dialog).getByLabelText("Base URL");
    // For OpenAI-compatible the base URL usually ends in /v1.
    expect(descriptionOf(url)).toBe(V1_HINT);
    await userEvent.click(kind(dialog, "Anthropic"));
    expect(descriptionOf(url)).toBe("");
    await userEvent.click(kind(dialog, "OpenAI-compatible"));
    expect(descriptionOf(url)).toBe(V1_HINT);
  });

  describe("api key field is cleared", () => {
    test("the field hides the key, and Show shows it", async () => {
      await page();
      const dialog = await openAdd();
      const key = within(dialog).getByLabelText("API key");
      // The browser is asked not to remember it.
      expect(key).toHaveAttribute("type", "password");
      expect(key).toHaveAttribute("autocomplete", "off");
      await enter(key, API_KEY);
      expect(key).toHaveValue(API_KEY);
      const show = within(dialog).getByRole("button", { name: "Show the API key" });
      expect(show).toHaveAttribute("aria-pressed", "false");
      await userEvent.click(show);
      expect(key).toHaveAttribute("type", "text");
      expect(key).toHaveAttribute("autocomplete", "off");
      expect(key).toHaveValue(API_KEY);
      const hide = within(dialog).getByRole("button", { name: "Hide the API key" });
      expect(hide).toHaveAttribute("aria-pressed", "true");
      await userEvent.click(hide);
      expect(key).toHaveAttribute("type", "password");
    });

    test("after cancel", async () => {
      const app = await page();
      await table();
      const dialog = await openAdd();
      await enter(within(dialog).getByLabelText("API key"), API_KEY);
      await userEvent.click(within(dialog).getByRole("button", { name: "Show the API key" }));
      expect(shown()).toContain(API_KEY);
      await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
      await closed();
      expectNoKey(app);
      const again = await openAdd();
      const key = within(again).getByLabelText("API key");
      expect(key).toHaveValue("");
      // Hidden again, too.
      expect(key).toHaveAttribute("type", "password");
      expectNoKey(app);
    });

    test("after a submit that was refused: the field is empty, and says so", async () => {
      const posts = counted("post", "/api/providers", () => refuse(errors.provider_exists));
      const app = await page();
      const dialog = await openAdd();
      await userEvent.type(within(dialog).getByLabelText("Name"), "openai");
      await known(dialog, "OpenAI");
      const key = within(dialog).getByLabelText("API key");
      await enter(key, API_KEY);
      await add(dialog);
      await within(dialog).findByText(errors.provider_exists.body.error.message);
      expect(read(posts.bodies[0], "api_key")).toBe(API_KEY);
      expect(key).toHaveValue("");
      expect(descriptionOf(key)).toBe(ENTER_AGAIN);
      // Neither the form nor the mutation that failed keeps it.
      expectNoKey(app);
      // The rest of the form is kept.
      expect(within(dialog).getByLabelText("Name")).toHaveValue("openai");
      expect(within(dialog).getByLabelText("Base URL")).toHaveValue("https://api.openai.com/v1");
      // Typed again, the field says what it said before.
      await enter(key, "sk");
      expect(descriptionOf(key)).not.toContain(ENTER_AGAIN);
      expect(descriptionOf(key)).not.toBe("");
    });

    test("after a refused submit without a key the field does not ask for one again", async () => {
      override("post", "/api/providers", () => refuse(errors.provider_exists));
      await page();
      const dialog = await openAdd();
      await userEvent.type(within(dialog).getByLabelText("Name"), "openai");
      await known(dialog, "OpenAI");
      await add(dialog);
      await within(dialog).findByText(errors.provider_exists.body.error.message);
      expect(descriptionOf(within(dialog).getByLabelText("API key"))).not.toContain(ENTER_AGAIN);
    });

    test("after a submit that succeeded", async () => {
      const state = keeps();
      const app = await page();
      await table();
      const dialog = await openAdd();
      await userEvent.type(within(dialog).getByLabelText("Name"), "groq");
      await known(dialog, "Groq");
      await enter(within(dialog).getByLabelText("API key"), API_KEY);
      await add(dialog);
      await closed();
      expect(read(state.created[0], "api_key")).toBe(API_KEY);
      expectNoKey(app);
      await waitFor(() => {
        expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
      });
      const again = await openAdd();
      expect(within(again).getByLabelText("API key")).toHaveValue("");
      expect(within(again).getByLabelText("Name")).toHaveValue("");
      expectNoKey(app);
    });
  });

  test("while the request runs the key is in the request only, and nowhere after it", async () => {
    const door = gate();
    override("post", "/api/providers", async () => {
      await door.opened;
      return refuse(errors.internal_error);
    });
    const app = await page();
    const dialog = await openAdd();
    await userEvent.type(within(dialog).getByLabelText("Name"), "groq");
    await known(dialog, "Groq");
    await enter(within(dialog).getByLabelText("API key"), API_KEY);
    await add(dialog);
    await within(dialog).findByRole("button", { name: "Adding the provider" });
    act(() => {
      door.open();
    });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.internal_error.body.error.message,
    );
    // The mutation that failed does not keep what it sent.
    expect(cached(app.queryClient)).not.toContain(API_KEY);
    expectNoKey(app);
  });

  test("provider errors", async () => {
    // `provider_exists` on name.
    const posts = counted("post", "/api/providers", () => refuse(errors.provider_exists));
    await page();
    const dialog = await openAdd();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, withCredential.name);
    await known(dialog, "OpenAI");
    await add(dialog);
    await waitFor(() => {
      expect(descriptionOf(name)).toContain(errors.provider_exists.body.error.message);
    });
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(name).toHaveFocus();
    expect(within(dialog).getAllByRole("alert")).toHaveLength(1);
    expect(dialog).toBeInTheDocument();
    expect(name).toHaveValue(withCredential.name);
    expect(toasts()).toEqual([]);
    expect(screen.queryByRole("status")).toBeNull();

    // 422: each field says what is wrong with it.
    override("post", "/api/providers", () =>
      refuse(
        validationFailed({
          name: fieldMessages.providerName,
          kind: fieldMessages.providerKind,
          base_url: fieldMessages.baseUrl,
          api_key: fieldMessages.apiKey,
        }),
      ),
    );
    await userEvent.type(name, "!");
    expect(name).not.toHaveAttribute("aria-invalid");
    await add(dialog);
    await waitFor(() => {
      expect(descriptionOf(name)).toContain(fieldMessages.providerName);
    });
    expect(
      descriptionOf(within(dialog).getByRole("radiogroup", { name: "Kind" })),
    ).toBe(fieldMessages.providerKind);
    expect(descriptionOf(within(dialog).getByLabelText("Base URL"))).toContain(
      fieldMessages.baseUrl,
    );
    expect(descriptionOf(within(dialog).getByLabelText("API key"))).toContain(
      fieldMessages.apiKey,
    );
    expect(within(dialog).getAllByRole("alert")).toHaveLength(4);
    expect(posts.calls).toBe(1);
    expect(toasts()).toEqual([]);
  });

  test("a dialog that is opened again is empty and shows no old error", async () => {
    override("post", "/api/providers", () => refuse(errors.provider_exists));
    const app = await page();
    await table();
    const opener = screen.getByRole("button", { name: "Add provider" });
    const dialog = await openAdd();
    await userEvent.type(within(dialog).getByLabelText("Name"), "openai");
    await known(dialog, "Anthropic");
    await add(dialog);
    await within(dialog).findByText(errors.provider_exists.body.error.message);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    await waitFor(() => {
      expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    });
    await waitFor(() => {
      expect(opener).toHaveFocus();
    });
    const again = await openAdd();
    expect(within(again).getByLabelText("Name")).toHaveValue("");
    expect(within(again).getByLabelText("Base URL")).toHaveValue("");
    expect(kind(again, "OpenAI-compatible")).toBeChecked();
    expect(within(again).queryByRole("alert")).toBeNull();
  });

  test("while the provider is added the button is disabled and says so", async () => {
    const door = gate();
    const posts = counted("post", "/api/providers", async () => {
      await door.opened;
      return ok("post", "/api/providers", 201, withCredential);
    });
    await page();
    const dialog = await openAdd();
    const name = within(dialog).getByLabelText("Name");
    await userEvent.type(name, "groq");
    await known(dialog, "Groq");
    await add(dialog);
    const running = await within(dialog).findByRole("button", { name: "Adding the provider" });
    expect(running).toBeDisabled();
    await userEvent.type(name, "{Enter}");
    expect(posts.calls).toBe(1);
    act(() => {
      door.open();
    });
    await closed();
    expect(posts.calls).toBe(1);
  });

  test("the session has ended when the provider is added: signed out, and the form says nothing", async () => {
    startGateway({ signedIn: true });
    const posts = counted("post", "/api/providers", unauthenticated);
    const app = await page();
    const dialog = await openAdd();
    await userEvent.type(within(dialog).getByLabelText("Name"), "groq");
    await known(dialog, "Groq");
    await enter(within(dialog).getByLabelText("API key"), API_KEY);
    await add(dialog);
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/providers")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(posts.calls).toBe(1);
    expect(screen.getByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(errors.unauthenticated.body.error.message)).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getQueryCache().getAll()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    expectNoKey(app);
  });

  test("the session ends while the form holds an API key", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    const dialog = await openAdd();
    await enter(within(dialog).getByLabelText("API key"), API_KEY);
    expect(shown()).toContain(API_KEY);

    await aCallFindsTheSessionEnded("/api/keys");

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expectNoKey(app);
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fproviders");
    });
    expectNoKey(app);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
  });

  test("at width 390 the controls of the dialog are high enough to touch", async () => {
    await page({ width: 390 });
    const dialog = await openAdd();
    const group = within(dialog).getByRole("group", { name: "Known providers" });
    expect(group.className.split(/\s+/)).toContain("flex-wrap");
    for (const control of [
      within(dialog).getByLabelText("Name"),
      within(dialog).getByLabelText("Base URL"),
      within(dialog).getByLabelText("API key"),
      ...within(dialog).getAllByRole("button", {
        name: /Add provider|Cancel|Show the API key|Groq|Ollama/,
      }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
    expect(dialog.className).toContain("max-w-[calc(100%-2rem)]");
  });
});

describe("editing a provider", () => {
  const OTHER_URL = "https://eu.api.openai.example.test/v1";

  function credential(dialog: HTMLElement): HTMLElement {
    return within(dialog).getByRole("radiogroup", { name: "API key" });
  }

  test("the form has the base URL, and never the credential", async () => {
    await page();
    const dialog = await openEdit(withCredential);
    // The name and the kind are said, and cannot be changed.
    expect(dialog).toHaveTextContent(withCredential.name);
    expect(within(dialog).queryByLabelText("Name")).toBeNull();
    expect(within(dialog).queryByRole("radiogroup", { name: "Kind" })).toBeNull();
    expect(within(dialog).getByLabelText("Base URL")).toHaveValue(withCredential.base_url);
    // Three choices; the current credential is kept unless another is made.
    const choices = credential(dialog);
    expect(within(choices).getAllByRole("radio")).toEqual(
      ["Keep the current key", "Replace the key", "Remove the key"].map((name) =>
        within(choices).getByRole("radio", { name }),
      ),
    );
    expect(within(choices).getByRole("radio", { name: "Keep the current key" })).toBeChecked();
    // There is no field that could show it: the API never returns it.
    expect(within(dialog).queryByLabelText("New API key")).toBeNull();
    expect(dialog.querySelectorAll("input[type=password]")).toHaveLength(0);
    await userEvent.click(within(choices).getByRole("radio", { name: "Replace the key" }));
    const key = within(dialog).getByLabelText("New API key");
    expect(key).toHaveValue("");
    expect(key).toHaveAttribute("type", "password");
    expect(key).toHaveAttribute("autocomplete", "off");
    // Another choice takes the field away again.
    await userEvent.click(within(choices).getByRole("radio", { name: "Remove the key" }));
    expect(within(dialog).queryByLabelText("New API key")).toBeNull();
  });

  describe("edit sends the three credential choices", () => {
    test("keep sends no api_key at all", async () => {
      const state = keeps();
      await page();
      const dialog = await openEdit(withCredential);
      const url = within(dialog).getByLabelText("Base URL");
      await userEvent.clear(url);
      await userEvent.type(url, OTHER_URL);
      await save(dialog);
      await closed();
      expect(state.patched).toEqual([
        { id: String(withCredential.id), body: { base_url: OTHER_URL } },
      ]);
      const [sent] = state.patched;
      expect(Object.keys(sent?.body as object)).toEqual(["base_url"]);
      expect(toasts()).toEqual(["Provider updated."]);
      // The list was asked for again.
      await waitFor(() => {
        expect(rowOf(withCredential.name)).toHaveTextContent(OTHER_URL);
      });
      expect(rowOf(withCredential.name)).toHaveTextContent("Set");
      expect(state.lists).toBe(2);
    });

    test("replace sends the new value", async () => {
      const state = keeps();
      const app = await page();
      const dialog = await openEdit(withCredential);
      await userEvent.click(within(dialog).getByRole("radio", { name: "Replace the key" }));
      await enter(within(dialog).getByLabelText("New API key"), API_KEY);
      await save(dialog);
      await closed();
      expect(state.patched).toEqual([
        {
          id: String(withCredential.id),
          body: { base_url: withCredential.base_url, api_key: API_KEY },
        },
      ]);
      expect(toasts()).toEqual(["Provider updated."]);
      expectNoKey(app);
      await waitFor(() => {
        expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
      });
    });

    test("remove sends null", async () => {
      const state = keeps();
      await page();
      const dialog = await openEdit(withCredential);
      await userEvent.click(within(dialog).getByRole("radio", { name: "Remove the key" }));
      await save(dialog);
      await closed();
      expect(state.patched).toEqual([
        { id: String(withCredential.id), body: { base_url: withCredential.base_url, api_key: null } },
      ]);
      const [sent] = state.patched;
      expect(Object.keys(sent?.body as object)).toContain("api_key");
      expect(read(sent?.body, "api_key")).toBeNull();
      await waitFor(() => {
        expect(rowOf(withCredential.name)).toHaveTextContent("None");
      });
    });
  });

  test("a provider without a key: it is left without one, or one is set", async () => {
    const state = keeps();
    await page();
    const dialog = await openEdit(withoutCredential);
    const choices = credential(dialog);
    // There is no key to keep, to replace or to remove.
    expect(within(choices).getAllByRole("radio")).toEqual(
      ["Leave without a key", "Set a key"].map((name) =>
        within(choices).getByRole("radio", { name }),
      ),
    );
    expect(within(choices).getByRole("radio", { name: "Leave without a key" })).toBeChecked();
    await userEvent.click(within(choices).getByRole("radio", { name: "Set a key" }));
    await enter(within(dialog).getByLabelText("New API key"), API_KEY);
    await save(dialog);
    await closed();
    expect(state.patched).toEqual([
      {
        id: String(withoutCredential.id),
        body: { base_url: withoutCredential.base_url, api_key: API_KEY },
      },
    ]);
    await waitFor(() => {
      expect(rowOf(withoutCredential.name)).toHaveTextContent("Set");
    });
  });

  test("the errors of the fields show on them, and the new key is cleared", async () => {
    const patches = counted("patch", "/api/providers/{id}", () =>
      refuse(validationFailed({ base_url: fieldMessages.baseUrlQuery, api_key: fieldMessages.apiKey })),
    );
    const app = await page();
    const dialog = await openEdit(withCredential);
    const url = within(dialog).getByLabelText("Base URL");
    await userEvent.type(url, "?x=1");
    await userEvent.click(within(dialog).getByRole("radio", { name: "Replace the key" }));
    const key = within(dialog).getByLabelText("New API key");
    await enter(key, API_KEY);
    await save(dialog);
    await waitFor(() => {
      expect(descriptionOf(url)).toContain(fieldMessages.baseUrlQuery);
    });
    expect(url).toHaveFocus();
    expect(descriptionOf(key)).toContain(fieldMessages.apiKey);
    expect(key).toHaveValue("");
    expect(dialog).toBeInTheDocument();
    expect(toasts()).toEqual([]);
    expect(patches.calls).toBe(1);
    expectNoKey(app);
  });

  test("a provider that is gone: the form says so, and the list is asked for again", async () => {
    const state = keeps();
    await page();
    const dialog = await openEdit(withCredential);
    state.providers = [withoutCredential];
    await save(dialog);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.not_found.body.error.message,
    );
    expect(toasts()).toEqual([]);
    await waitFor(() => {
      expect(state.lists).toBe(2);
    });
  });

  test("while the provider is saved the button is disabled and says so", async () => {
    const door = gate();
    const patches = counted("patch", "/api/providers/{id}", async () => {
      await door.opened;
      return ok("patch", "/api/providers/{id}", 200, withCredential);
    });
    await page();
    const dialog = await openEdit(withCredential);
    await save(dialog);
    expect(await within(dialog).findByRole("button", { name: "Saving" })).toBeDisabled();
    await userEvent.type(within(dialog).getByLabelText("Base URL"), "{Enter}");
    expect(patches.calls).toBe(1);
    act(() => {
      door.open();
    });
    await closed();
  });

  test("the session has ended when the provider is saved: signed out, and the form says nothing", async () => {
    startGateway({ signedIn: true });
    const patches = counted("patch", "/api/providers/{id}", unauthenticated);
    const app = await page();
    const dialog = await openEdit(withCredential);
    await userEvent.click(within(dialog).getByRole("radio", { name: "Replace the key" }));
    await enter(within(dialog).getByLabelText("New API key"), API_KEY);
    await save(dialog);
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/providers")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(patches.calls).toBe(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(app.queryClient.getMutationCache().getAll()).toEqual([]);
    expectNoKey(app);
  });
});

describe("deleting a provider", () => {
  test("delete confirms", async () => {
    const state = keeps();
    await page();
    const dialog = await askToDelete(withCredential);
    expect(dialog).toHaveAccessibleDescription(DELETE);
    expect(state.deleted).toEqual([]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await closed();
    expect(state.deleted).toEqual([String(withCredential.id)]);
    expect(toasts()).toEqual(["Provider deleted."]);
    // The list was asked for again.
    await waitFor(() => {
      expect(screen.queryByRole("cell", { name: withCredential.name })).toBeNull();
    });
    expect(rowOf(withoutCredential.name)).toBeInTheDocument();
    expect(state.lists).toBe(2);
  });

  test("cancel changes nothing", async () => {
    const state = keeps();
    await page();
    const dialog = await askToDelete(withCredential);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await closed();
    expect(state.deleted).toEqual([]);
    expect(toasts()).toEqual([]);
    expect(rowOf(withCredential.name)).toBeInTheDocument();
  });

  test("a refusal stays in the dialog, and a provider that is gone is asked for again", async () => {
    const state = keeps();
    const removes = counted("delete", "/api/providers/{id}", () => refuse(errors.not_found));
    await page();
    const dialog = await askToDelete(withCredential);
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.not_found.body.error.message,
    );
    expect(removes.calls).toBe(1);
    expect(toasts()).toEqual([]);
    await waitFor(() => {
      expect(state.lists).toBe(2);
    });
  });

  test("a deletion whose answer came for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("delete", "/api/providers/{id}", async () => {
      await door.opened;
      return noContent();
    });
    const app = await page();
    const dialog = await askToDelete(withCredential);
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await aCallFindsTheSessionEnded("/api/keys");
    door.open();
    await settle();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/providers")}`);
  });
});
