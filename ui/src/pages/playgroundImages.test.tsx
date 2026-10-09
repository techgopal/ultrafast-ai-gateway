import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { pipelineErrors } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { startGateway } from "@/test/gateway";
import { ok, override, refusePipeline } from "@/test/handlers";
import {
  choose,
  expectLabelsNameControls,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  href,
  installSelect,
  optionsOf,
  SESSION_ENDED,
  toasts,
} from "@/test/pages";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

function page(options: { user?: fixtures.Me } = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/playground", ...options });
}

/** Opens the page in Images mode. */
async function images(): Promise<void> {
  await page();
  await userEvent.click(await screen.findByRole("button", { name: "Images" }));
}

const prompt = () => screen.getByRole("textbox", { name: "Prompt" });
const generate = () => screen.getByRole("button", { name: "Generate" });
const picker = () => screen.getByRole("combobox", { name: "Model or route" });

/** Records the bodies the playground sends for images, and answers each with `answer`. */
function generations(
  answer: () => Response = () => ok("post", "/api/playground/images", 200, fixtures.playgroundImages),
) {
  const sent: unknown[] = [];
  override("post", "/api/playground/images", async ({ request }) => {
    sent.push(await request.json());
    return answer();
  });
  return sent;
}

async function describeImage(text: string): Promise<void> {
  await userEvent.click(prompt());
  await userEvent.paste(text);
}

describe("Images mode", () => {
  test("Chat is the mode the page opens in, and Images is one click away", async () => {
    await page();
    expect(await screen.findByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("textbox", { name: "Prompt" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Images" }));
    expect(screen.getByRole("button", { name: "Images" })).toHaveAttribute("aria-pressed", "true");
    expect(prompt()).toBeInTheDocument();
    // The chat is not on screen, and its form is not offered.
    expect(screen.queryByRole("textbox", { name: "Message" })).toBeNull();
  });

  test("it has one main and one h1, and its fields are labelled", async () => {
    await images();
    expectOneMain();
    expectOneH1("Playground");
    expectLabelsNameControls(document.body);
  });

  test("it offers the models and routes of the chat", async () => {
    await images();
    expect((await optionsOf(picker())).slice(0, 2)).toEqual(["local-llm/llama3.1:8b", "openai/gpt-4o"]);
  });

  test("the prompt, the size and the number go to the gateway, and the images come back", async () => {
    const sent = generations();
    await images();
    await choose(picker(), "openai/gpt-4o-mini");
    await choose(screen.getByRole("combobox", { name: "Size" }), "1536 x 1024 (landscape)");
    await userEvent.clear(screen.getByRole("textbox", { name: "Number of images" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Number of images" }), "2");
    await describeImage("a red fox");
    await userEvent.click(generate());
    const shown = await screen.findAllByRole("img", { name: /Generation \d/ });
    expect(shown).toHaveLength(1);
    expect(shown[0]).toHaveAttribute("src", `data:image/png;base64,${fixtures.TINY_PNG}`);
    expect(sent).toEqual([{ model: "openai/gpt-4o-mini", prompt: "a red fox", n: 2, size: "1536x1024" }]);
    expect(screen.getByText(/Tokens: 12 in, 800 out\./)).toBeInTheDocument();
  });

  test("one image of 1024 x 1024 is asked for unless the page says otherwise", async () => {
    const sent = generations();
    await images();
    await describeImage("a cat");
    await userEvent.click(generate());
    await screen.findAllByRole("img", { name: /Generation \d/ });
    expect(sent).toEqual([{ model: "local-llm/llama3.1:8b", prompt: "a cat", n: 1, size: "1024x1024" }]);
  });

  test("the provider's default size sends none", async () => {
    const sent = generations();
    await images();
    await choose(screen.getByRole("combobox", { name: "Size" }), "Provider default");
    await describeImage("a cat");
    await userEvent.click(generate());
    await screen.findAllByRole("img", { name: /Generation \d/ });
    expect(sent).toEqual([{ model: "local-llm/llama3.1:8b", prompt: "a cat", n: 1 }]);
  });

  test("a conversation and a prompt stay when the mode is switched", async () => {
    await page();
    await userEvent.click(await screen.findByRole("button", { name: "Images" }));
    await describeImage("a cat");
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    expect(screen.getByRole("textbox", { name: "Message" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Images" }));
    expect(prompt()).toHaveValue("a cat");
  });

  test("only images that came as base64 are shown, never one the browser would fetch", async () => {
    generations(() =>
      ok("post", "/api/playground/images", 200, {
        created: 1,
        data: [
          fixtures.freeForm({ url: "https://elsewhere.example/x.png" }),
          fixtures.freeForm({ b64_json: fixtures.TINY_PNG }),
        ],
      }),
    );
    await images();
    await describeImage("a cat");
    await userEvent.click(generate());
    expect(await screen.findAllByRole("img", { name: /Generation \d/ })).toHaveLength(1);
    expect(document.querySelector("img[src^='https:']")).toBeNull();
  });

  test.each(["0", "5", "x", ""])("a number of images of '%s' is refused on the field", async (typed) => {
    const sent = generations();
    await images();
    const count = screen.getByRole("textbox", { name: "Number of images" });
    await userEvent.clear(count);
    if (typed !== "") await userEvent.type(count, typed);
    await describeImage("a cat");
    await userEvent.click(generate());
    expect(await screen.findByText(/must be a whole number from 1 to 4/)).toBeInTheDocument();
    expect(sent).toEqual([]);
  });

  test("Generate waits for a prompt", async () => {
    await images();
    expect(generate()).toBeDisabled();
    await describeImage("a cat");
    expect(generate()).toBeEnabled();
  });

  test.each([
    ["a model the user may not call", pipelineErrors.forbidden, "You do not have access to model 'openai/gpt-4o'."],
    ["a model that cannot draw", pipelineErrors.unavailable, "No provider could serve this request."],
    ["a guardrail that blocks the prompt", pipelineErrors.guardrail, "Blocked by guardrail 'house-rules'."],
  ])("%s is said in place and the prompt stays", async (_, refusal, text) => {
    generations(() => refusePipeline(refusal));
    await images();
    await describeImage("a cat");
    await userEvent.click(generate());
    expect(await screen.findByRole("alert")).toHaveTextContent(text);
    expect(prompt()).toHaveValue("a cat");
    expect(screen.queryByRole("img", { name: /Generation \d/ })).toBeNull();
    expect(toasts()).toEqual([]);
  });

  test("a second call replaces the images and clears an error", async () => {
    generations(() => refusePipeline(pipelineErrors.forbidden));
    await images();
    await describeImage("a cat");
    await userEvent.click(generate());
    await screen.findByRole("alert");
    generations();
    await userEvent.click(generate());
    await screen.findAllByRole("img", { name: /Generation \d/ });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("a call that finds the session ended signs out", async () => {
    startGateway({ signedIn: true, me: fixtures.me.lena });
    override("post", "/api/playground/images", unauthenticated);
    const app = await page({ user: fixtures.me.lena });
    await userEvent.click(await screen.findByRole("button", { name: "Images" }));
    await describeImage("a cat");
    await userEvent.click(generate());
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/playground")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("Copy as curl is the command for /v1/images/generations with the settings on the page", async () => {
    let text: string | undefined;
    const before = Object.getOwnPropertyDescriptor(navigator, "clipboard");
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
      await images();
      await describeImage("it's a fox");
      await userEvent.click(screen.getByRole("button", { name: "Copy as curl" }));
      await waitFor(() => {
        expect(toasts()).toEqual(["Copied."]);
      });
      expect(text).toContain(`curl ${window.location.origin}/v1/images/generations`);
      expect(text).toContain("-H 'Authorization: Bearer <your key>'");
      expect(text).not.toMatch(/csrf|cookie|uf_session/i);
      const body = /-d '(.*)'$/s.exec(text ?? "")?.[1]?.replace(/'"'"'/g, "'") ?? "";
      expect(JSON.parse(body)).toEqual({
        model: "local-llm/llama3.1:8b",
        prompt: "it's a fox",
        n: 1,
        size: "1024x1024",
      });
      // Nothing was sent, and the prompt is still there.
      expect(prompt()).toHaveValue("it's a fox");
    } finally {
      if (before === undefined) Reflect.deleteProperty(navigator, "clipboard");
      else Object.defineProperty(navigator, "clipboard", before);
    }
  });
});
