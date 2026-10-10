import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { errors } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { eventStream, ok, override, refuse } from "@/test/handlers";
import {
  choose,
  expectLabelsNameControls,
  forgetToasts,
  installSelect,
  optionsOf,
  toasts,
} from "@/test/pages";
import { renderWithApp, type AppRenderResult } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

function page(route = "/playground"): Promise<AppRenderResult> {
  return renderWithApp(null, { route });
}

const modelPicker = () => screen.findByRole("combobox", { name: "Model or route" });
const templatePicker = () => screen.findByRole("combobox", { name: "Prompt template" });
const message = () => screen.getByRole("textbox", { name: "Message" });
const sendButton = () => screen.getByRole("button", { name: "Send" });

function chats() {
  const sent: Record<string, unknown>[] = [];
  override("post", "/api/playground/chat", async ({ request }) => {
    sent.push((await request.json()) as Record<string, unknown>);
    return eventStream(fixtures.playgroundChunks);
  });
  return sent;
}

async function fill(label: string, text: string): Promise<void> {
  const field = await screen.findByLabelText(label);
  await userEvent.click(field);
  await userEvent.paste(text);
}

describe("the prompt template picker", () => {
  test("is not there when there are no templates", async () => {
    override("get", "/api/prompts", () => ok("get", "/api/prompts", 200, { prompts: [] }));
    await page();
    await modelPicker();
    expect(screen.queryByRole("combobox", { name: "Prompt template" })).toBeNull();
  });

  test("offers no template, and then every template by name", async () => {
    await page();
    expect(await optionsOf(await templatePicker())).toEqual(["No template", "greet", "summarize"]);
    expect(await templatePicker()).toHaveTextContent("No template");
    expect(screen.queryByRole("combobox", { name: "Template version" })).toBeNull();
  });

  test("a template shows its versions and a field for each variable of the version", async () => {
    await page();
    await choose(await templatePicker(), "summarize");
    const version = await screen.findByRole("combobox", { name: "Template version" });
    expect(version).toHaveTextContent("Latest (version 3)");
    expect(await optionsOf(version)).toEqual(["Latest (version 3)", "Version 3", "Version 2", "Version 1"]);
    expect(await screen.findByLabelText("Variable: audience")).toBeInTheDocument();
    expect(screen.getByLabelText("Variable: text")).toBeInTheDocument();
    // Another version has other variables.
    await choose(version, "Version 1");
    await waitFor(() => {
      expect(screen.queryByLabelText("Variable: audience")).toBeNull();
    });
    expect(screen.getByLabelText("Variable: text")).toBeInTheDocument();
  });

  test("the call names the template and gives the variables; the latest version is not named", async () => {
    const sent = chats();
    await page();
    await choose(await templatePicker(), "summarize");
    await fill("Variable: audience", "kids");
    await fill("Variable: text", "A long article.");
    await userEvent.click(message());
    await userEvent.paste("Go");
    await userEvent.click(sendButton());
    expect(await screen.findByText("Hello")).toBeInTheDocument();
    // The template names a model, and the page offers it first: no model is sent.
    expect(sent).toEqual([
      {
        stream: true,
        prompt: { id: "summarize", variables: { audience: "kids", text: "A long article." } },
        messages: [{ role: "user", content: "Go" }],
      },
    ]);
  });

  test("a version that is chosen is named, and only its own variables are sent", async () => {
    const sent = chats();
    await page();
    await choose(await templatePicker(), "summarize");
    await fill("Variable: audience", "kids");
    await fill("Variable: text", "Short.");
    await choose(await screen.findByRole("combobox", { name: "Template version" }), "Version 1");
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await userEvent.click(sendButton());
    await waitFor(() => {
      expect(sent).toHaveLength(1);
    });
    expect(sent[0]).toEqual({
      model: "openai/gpt-4o-mini",
      stream: true,
      prompt: { id: "summarize", version: 1, variables: { text: "Short." } },
      messages: [],
    });
  });

  test("the model of the template is offered first, and only while a template that names one is chosen", async () => {
    await page();
    const models = await modelPicker();
    expect(models).not.toHaveTextContent("Template's model");
    await choose(await templatePicker(), "summarize");
    await waitFor(() => {
      expect(models).toHaveTextContent("Template's model (openai/gpt-4o)");
    });
    expect((await optionsOf(models))[0]).toBe("Template's model (openai/gpt-4o)");
    // greet names no model: the choice is gone and a model is sent.
    await choose(await templatePicker(), "greet");
    await waitFor(() => {
      expect(models).not.toHaveTextContent("Template's model");
    });
  });

  test("a template can be called without a message of your own", async () => {
    const sent = chats();
    await page();
    await modelPicker();
    // Without a template an empty message is not sent.
    expect(sendButton()).toBeDisabled();
    await choose(await templatePicker(), "greet");
    await fill("Variable: name", "Ada");
    await waitFor(() => {
      expect(sendButton()).toBeEnabled();
    });
    await userEvent.click(sendButton());
    expect(await screen.findByText("Hello")).toBeInTheDocument();
    expect(sent[0]).toMatchObject({ prompt: { id: "greet", variables: { name: "Ada" } }, messages: [] });
  });

  test("a variable left empty is sent as an empty text", async () => {
    const sent = chats();
    await page();
    await choose(await templatePicker(), "greet");
    await screen.findByLabelText("Variable: name");
    await waitFor(() => {
      expect(sendButton()).toBeEnabled();
    });
    await userEvent.click(sendButton());
    await waitFor(() => {
      expect(sent).toHaveLength(1);
    });
    expect(sent[0]?.prompt).toEqual({ id: "greet", variables: { name: "" } });
  });

  test("going back to No template sends no prompt", async () => {
    const sent = chats();
    await page();
    await choose(await templatePicker(), "greet");
    await screen.findByLabelText("Variable: name");
    await choose(await templatePicker(), "No template");
    expect(screen.queryByLabelText("Variable: name")).toBeNull();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await userEvent.click(message());
    await userEvent.paste("Hi");
    await userEvent.click(sendButton());
    await waitFor(() => {
      expect(sent).toHaveLength(1);
    });
    expect(sent[0]).toEqual({ model: "openai/gpt-4o-mini", stream: true, messages: [{ role: "user", content: "Hi" }] });
  });

  test("Open in Playground chooses the template and the version", async () => {
    const sent = chats();
    await page("/playground?prompt=summarize&version=2");
    expect(await templatePicker()).toHaveTextContent("summarize");
    expect(await screen.findByRole("combobox", { name: "Template version" })).toHaveTextContent("Version 2");
    await fill("Variable: text", "x");
    await userEvent.click(sendButton());
    await waitFor(() => {
      expect(sent).toHaveLength(1);
    });
    expect(sent[0]?.prompt).toEqual({ id: "summarize", version: 2, variables: { text: "x" } });
  });

  test("a version that is asked for is not sent as the latest while the versions are being read", async () => {
    const sent = chats();
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    override("get", "/api/prompts/{id}", async () => {
      await gate;
      return ok("get", "/api/prompts/{id}", 200, fixtures.promptViews.summarize);
    });
    await page("/playground?prompt=summarize&version=2");
    // The latest version is read, so its variables are here; the list of versions is not.
    await fill("Variable: text", "x");
    await fill("Variable: audience", "kids");
    await userEvent.click(message());
    await userEvent.paste("Go");
    expect(sendButton()).toBeDisabled();
    release();
    await waitFor(() => {
      expect(sendButton()).toBeEnabled();
    });
    await userEvent.click(sendButton());
    await waitFor(() => {
      expect(sent).toHaveLength(1);
    });
    expect(sent[0]?.prompt).toEqual({ id: "summarize", version: 2, variables: { text: "x" } });
  });

  test("a link to a template that is not there, or a version that is not, chooses what is", async () => {
    await page("/playground?prompt=gone&version=2");
    expect(await templatePicker()).toHaveTextContent("No template");
    expect(screen.queryByRole("combobox", { name: "Template version" })).toBeNull();
  });

  test("a version that is not there is the latest", async () => {
    await page("/playground?prompt=summarize&version=9");
    expect(await screen.findByRole("combobox", { name: "Template version" })).toHaveTextContent("Latest (version 3)");
  });

  test("a template that cannot be read blocks Send and says so", async () => {
    override("get", "/api/prompts/{id}/versions/{version}", () => refuse(errors.internal_error));
    await page();
    await choose(await templatePicker(), "summarize");
    expect(await screen.findByText("The variables of this template could not be read.")).toBeInTheDocument();
    expect(sendButton()).toBeDisabled();
  });

  test("the list failing leaves the playground as it was", async () => {
    override("get", "/api/prompts", () => refuse(errors.internal_error));
    await page();
    await modelPicker();
    expect(screen.queryByRole("combobox", { name: "Prompt template" })).toBeNull();
    expect(message()).toBeInTheDocument();
  });

  test("Copy as curl has the prompt", async () => {
    let text = "";
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: (value: string) => {
          text = value;
          return Promise.resolve();
        },
      },
    });
    try {
      await page();
      await choose(await templatePicker(), "greet");
      await choose(await modelPicker(), "openai/gpt-4o-mini");
      await fill("Variable: name", "Ada");
      await userEvent.click(screen.getByRole("button", { name: "Copy as curl" }));
      await waitFor(() => {
        expect(toasts()).toEqual(["Copied."]);
      });
      const body = /-d '(.*)'$/s.exec(text)?.[1] ?? "";
      expect(JSON.parse(body)).toEqual({
        model: "openai/gpt-4o-mini",
        stream: true,
        prompt: { id: "greet", variables: { name: "Ada" } },
        messages: [],
      });
    } finally {
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  test("the labels name the controls", async () => {
    await page();
    await choose(await templatePicker(), "summarize");
    await screen.findByLabelText("Variable: text");
    expectLabelsNameControls(document.body);
  });
});
