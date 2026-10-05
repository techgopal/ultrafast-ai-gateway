import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { pipelineErrors } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { startGateway } from "@/test/gateway";
import {
  eventStream,
  networkFailure,
  ok,
  override,
  refusePipeline,
} from "@/test/handlers";
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
  settle,
  stored,
  toasts,
} from "@/test/pages";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

beforeAll(installSelect);
afterEach(forgetToasts);

function page(options: { user?: fixtures.Me; width?: number } = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/playground", ...options });
}

const modelPicker = () => screen.findByRole("combobox", { name: "Model or route" });
const message = () => screen.getByRole("textbox", { name: "Message" });
const sendButton = () => screen.getByRole("button", { name: "Send" });

/** Records the bodies the playground sends, and answers each with `answer`. */
function chats(answer: () => Response = () => eventStream(fixtures.playgroundChunks)) {
  const sent: unknown[] = [];
  override("post", "/api/playground/chat", async ({ request }) => {
    sent.push(await request.json());
    return answer();
  });
  return sent;
}

async function say(text: string): Promise<void> {
  await userEvent.click(message());
  await userEvent.paste(text);
  await userEvent.click(sendButton());
}

/** A stream the test feeds by hand, to look at the page between two chunks. */
function handFed() {
  const encoder = new TextEncoder();
  let controller: ReadableStreamDefaultController<Uint8Array> | undefined;
  const body = new ReadableStream<Uint8Array>({
    start(c) {
      controller = c;
    },
  });
  return {
    response: () => new Response(body, { headers: { "content-type": "text/event-stream" } }),
    push: (text: string) => {
      controller?.enqueue(encoder.encode(text));
    },
    close: () => {
      controller?.close();
    },
  };
}

const delta = (text: string) =>
  `data: ${JSON.stringify({ model: "gpt-4o-mini", choices: [{ index: 0, delta: { content: text }, finish_reason: null }] })}\n\n`;

describe("the playground page", () => {
  test("it has one main and one h1, and its fields are labelled", async () => {
    await page();
    await modelPicker();
    expectOneMain();
    expectOneH1("Playground");
    expectLabelsNameControls(document.body);
  });

  test("an admin may call every enabled model and every route", async () => {
    await page();
    expect(await optionsOf(await modelPicker())).toEqual([
      "local-llm/llama3.1:8b",
      "openai/gpt-4o",
      "openai/gpt-4o-mini",
      ...fixtures.routeList.map((route) => `${route.name} (route)`).sort(),
    ]);
  });

  test("a member is offered what the gateway lists for them", async () => {
    startGateway({ signedIn: true, me: fixtures.me.lena });
    override("get", "/api/models", () =>
      ok("get", "/api/models", 200, { models: fixtures.callableModels }),
    );
    override("get", "/api/routes", () =>
      ok("get", "/api/routes", 200, { routes: fixtures.routesForMember }),
    );
    await page({ user: fixtures.me.lena });
    expect(await optionsOf(await modelPicker())).toEqual([
      "openai/gpt-4o",
      "openai/gpt-4o-mini",
      ...fixtures.routesForMember.map((route) => `${route.name} (route)`).sort(),
    ]);
  });

  test("with nothing to call it says so, and offers no form", async () => {
    override("get", "/api/models", () => ok("get", "/api/models", 200, { models: [] }));
    override("get", "/api/routes", () => ok("get", "/api/routes", 200, { routes: [] }));
    await page();
    expect(await screen.findByText("Nothing to call")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Message" })).toBeNull();
    expectOneH1("Playground");
  });

  test("a disabled model is not offered", async () => {
    override("get", "/api/models", () =>
      ok("get", "/api/models", 200, { models: [fixtures.models.openaiDisabled] }),
    );
    override("get", "/api/routes", () => ok("get", "/api/routes", 200, { routes: [] }));
    await page();
    expect(await screen.findByText("Nothing to call")).toBeInTheDocument();
  });

  test("the models failing to load say so and can be asked again", async () => {
    override("get", "/api/models", () => networkFailure());
    await page();
    expect(await screen.findByText("Could not reach the gateway.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expectOneH1("Playground");
  });
});

describe("sending", () => {
  test("the message goes with the chosen model, streamed, and the answer is shown", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("Say hello");
    expect(await screen.findByText("Hello")).toBeInTheDocument();
    expect(sent).toEqual([
      {
        model: "openai/gpt-4o-mini",
        stream: true,
        messages: [{ role: "user", content: "Say hello" }],
      },
    ]);
    const thread = screen.getByRole("list", { name: "Conversation" });
    expect(within(thread).getAllByRole("listitem").map((li) => li.getAttribute("data-role"))).toEqual([
      "user",
      "assistant",
    ]);
    expect(message()).toHaveValue("");
  });

  test("the answer is shown as it arrives, and the page waits for it", async () => {
    const feed = handFed();
    chats(feed.response);
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    expect(await screen.findByText("Waiting for the answer")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();

    feed.push(delta("Hel"));
    expect(await screen.findByText("Hel")).toBeInTheDocument();
    // A chunk cut in the middle of an event shows nothing until it is whole.
    feed.push(delta("lo").slice(0, 20));
    await settle();
    expect(screen.queryByText("Hello")).toBeNull();
    feed.push(delta("lo").slice(20));
    expect(await screen.findByText("Hello")).toBeInTheDocument();
    feed.push("data: [DONE]\n\n");
    feed.close();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Send" })).toBeInTheDocument();
    });
    expect(screen.getByText("Hello")).toBeInTheDocument();
  });

  test("the usage and the cost of the call are shown from the price of the model", async () => {
    chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    // 2 in at $0.15 and 3 out at $0.60 per million: 2.1 micros, to a whole micro.
    expect(await screen.findByText("Tokens: 2 in, 3 out. Cost: $0.000002.")).toBeInTheDocument();
  });

  test("the cost of a call stays with the model it was called with when another is chosen", async () => {
    chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    expect(await screen.findByText("Tokens: 2 in, 3 out. Cost: $0.000002.")).toBeInTheDocument();
    await choose(await modelPicker(), "local-llm/llama3.1:8b");
    expect(screen.getByText("Tokens: 2 in, 3 out. Cost: $0.000002.")).toBeInTheDocument();
    expect(screen.queryByText(/the model has no price/)).toBeNull();
  });

  test("a model with no price has tokens and no cost", async () => {
    chats();
    await page();
    await choose(await modelPicker(), "local-llm/llama3.1:8b");
    await say("hi");
    expect(
      await screen.findByText("Tokens: 2 in, 3 out. Cost: not known, the model has no price."),
    ).toBeInTheDocument();
  });

  test("a route is priced by the model that answered, when only one has that name", async () => {
    chats();
    await page();
    await choose(await modelPicker(), `${fixtures.routes.support.name} (route)`);
    await say("hi");
    expect(await screen.findByText("Tokens: 2 in, 3 out. Cost: $0.000002.")).toBeInTheDocument();
  });

  test("the thread is sent with every message, the system prompt first, and the parameters", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await userEvent.click(screen.getByLabelText("System prompt"));
    await userEvent.paste("Be brief.");
    await userEvent.click(screen.getByLabelText("Max tokens"));
    await userEvent.paste("64");
    await userEvent.click(screen.getByLabelText("Temperature"));
    await userEvent.paste("0.5");
    await userEvent.click(screen.getByLabelText("Top P"));
    await userEvent.paste("0.9");
    await userEvent.click(screen.getByLabelText("Stop sequences"));
    await userEvent.paste("END, ###");
    await say("one");
    await screen.findByText("Hello");
    await say("two");
    await waitFor(() => {
      expect(sent).toHaveLength(2);
    });
    expect(sent[1]).toEqual({
      model: "openai/gpt-4o-mini",
      stream: true,
      max_tokens: 64,
      temperature: 0.5,
      top_p: 0.9,
      stop: ["END", "###"],
      messages: [
        { role: "system", content: "Be brief." },
        { role: "user", content: "one" },
        { role: "assistant", content: "Hello" },
        { role: "user", content: "two" },
      ],
    });
  });

  test("Enter with Ctrl sends", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await userEvent.click(message());
    await userEvent.type(message(), "typed key by key{Control>}{Enter}{/Control}");
    await waitFor(() => {
      expect(sent).toHaveLength(1);
    });
    expect(sent[0]).toMatchObject({ messages: [{ role: "user", content: "typed key by key" }] });
  });

  test("Send waits for a message, and a blank one is not sent", async () => {
    const sent = chats();
    await page();
    await modelPicker();
    expect(sendButton()).toBeDisabled();
    await userEvent.click(message());
    await userEvent.paste("   ");
    expect(sendButton()).toBeDisabled();
    expect(sent).toEqual([]);
  });

  test("a parameter that is not valid is refused on its field and nothing is sent", async () => {
    const sent = chats();
    await page();
    await modelPicker();
    await userEvent.click(screen.getByLabelText("Temperature"));
    await userEvent.paste("hot");
    await say("hi");
    const field = screen.getByLabelText("Temperature");
    expect(field).toHaveAccessibleDescription(/Enter a number from 0 to 2\./);
    expect(field).toBeInvalid();
    await settle();
    expect(sent).toEqual([]);
    expect(message()).toHaveValue("hi");
  });

  test("nothing of the conversation is kept by the browser, or when the page is left and opened again", async () => {
    chats();
    const app = await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("a secret plan");
    await screen.findByText("Hello");
    expect(stored()).not.toContain("secret plan");
    expect(stored()).not.toContain("Hello");
    await app.router.navigate({ to: "/logs" });
    await app.router.navigate({ to: "/playground" });
    await modelPicker();
    expect(screen.queryByText("a secret plan")).toBeNull();
    expect(screen.queryByText("Hello")).toBeNull();
  });

  test("New conversation empties the thread", async () => {
    chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    await screen.findByText("Hello");
    await userEvent.click(screen.getByRole("button", { name: "New conversation" }));
    expect(screen.queryByText("Hello")).toBeNull();
    expect(screen.getByText(/Nothing has been said yet/)).toBeInTheDocument();
    expect(screen.queryByText(/Tokens:/)).toBeNull();
  });

  test("a model that is no longer offered is not sent", async () => {
    const sent = chats();
    const app = await page();
    await choose(await modelPicker(), "openai/gpt-4o");
    expect(document.querySelector('button[role="combobox"]')).toHaveTextContent("openai/gpt-4o");
    override("get", "/api/models", () =>
      ok("get", "/api/models", 200, { models: [fixtures.models.openaiMini] }),
    );
    override("get", "/api/routes", () => ok("get", "/api/routes", 200, { routes: [] }));
    await app.queryClient.invalidateQueries();
    await waitFor(() => {
      expect(document.querySelector('button[role="combobox"]')).toHaveTextContent("openai/gpt-4o-mini");
    });
    await say("hi");
    await waitFor(() => {
      expect(sent.length).toBeGreaterThan(0);
    });
    expect(sent.at(-1)).toMatchObject({ model: "openai/gpt-4o-mini" });
  });
});

describe("stopping", () => {
  test("Stop keeps what was said, ends the call and says nothing is wrong", async () => {
    const feed = handFed();
    chats(feed.response);
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    feed.push(delta("Hel"));
    await screen.findByText("Hel");
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Send" })).toBeInTheDocument();
    });
    expect(screen.getByText("Hel")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText("Waiting for the answer")).toBeNull();
    // What was said is part of the thread that the next call sends.
    const sent = chats();
    await say("go on");
    await waitFor(() => {
      expect(sent).toHaveLength(1);
    });
    expect(sent[0]).toMatchObject({
      messages: [
        { role: "user", content: "hi" },
        { role: "assistant", content: "Hel" },
        { role: "user", content: "go on" },
      ],
    });
  });

  test("Stop and Send are two elements: a click that ends the call is not a click on Send", async () => {
    const feed = handFed();
    const sent = chats(feed.response);
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    const stop = await screen.findByRole("button", { name: "Stop" });
    // The call ends in the middle of the click, as the browser's does: the
    // page is changed before the click is over.
    stop.addEventListener("click", () => {
      expect(stop.getAttribute("type")).toBe("button");
    });
    await userEvent.click(stop);
    const send = await screen.findByRole("button", { name: "Send" });
    expect(send).not.toBe(stop);
    expect(stop.isConnected).toBe(false);
    expect(send).toHaveAttribute("type", "submit");
    await settle();
    // Only the one call was made.
    expect(sent).toHaveLength(1);
  });

  test("Stop before anything came puts the message back in the box", async () => {
    const feed = handFed();
    chats(feed.response);
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    await screen.findByText("Waiting for the answer");
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() => {
      expect(message()).toHaveValue("hi");
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByText(/Nothing has been said yet/)).toBeInTheDocument();
  });
});

describe("errors in place", () => {
  test.each([
    ["a model the user may not call", pipelineErrors.forbidden, "You do not have access to model 'openai/gpt-4o'."],
    ["a model that is not there", pipelineErrors.unknown, "Unknown model 'nothing'."],
    [
      "a rate limit, with the wait",
      pipelineErrors.rateLimited,
      "rate limit 'requests per minute' of user 'lena@example.com' reached Try again in 30 seconds.",
    ],
    [
      "a spent budget, with the wait",
      pipelineErrors.budget,
      "budget 'monthly' of user 'lena@example.com' reached Try again in 120 minutes.",
    ],
    ["no provider", pipelineErrors.unavailable, "No provider could serve this request."],
  ])("%s", async (_, refusal, text) => {
    chats(() => refusePipeline(refusal));
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(text);
    // The message goes back to the box; nothing of it stays in the thread.
    expect(message()).toHaveValue("hi");
    expect(screen.getByText(/Nothing has been said yet/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
    expect(toasts()).toEqual([]);
  });

  test("an error is gone with the next call", async () => {
    chats(() => refusePipeline(pipelineErrors.forbidden));
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    await screen.findByRole("alert");
    chats();
    await userEvent.click(sendButton());
    await screen.findByText("Hello");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("a gateway that cannot be reached", async () => {
    chats(() => networkFailure());
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
    expect(message()).toHaveValue("hi");
  });

  test("an error in the middle of the stream keeps what was said and says what broke", async () => {
    chats(() =>
      eventStream([
        delta("Hel"),
        `data: ${JSON.stringify({ error: { message: "The provider failed.", type: "upstream_error" } })}\n\n`,
      ]),
    );
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    expect(await screen.findByRole("alert")).toHaveTextContent("The provider failed.");
    expect(screen.getByText("Hel")).toBeInTheDocument();
  });

  test("a body that is not an error of the gateway is not shown", async () => {
    chats(() => new Response("<html>oops</html>", { status: 502 }));
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    const alert = await screen.findByRole("alert");
    expect(alert).not.toHaveTextContent("<");
    expect(alert).toHaveTextContent("The gateway answered with status 502");
  });
});

describe("the end of the session", () => {
  test("a call that finds the session ended signs out, and the page says nothing of it", async () => {
    startGateway({ signedIn: true, me: fixtures.me.lena });
    override("post", "/api/playground/chat", unauthenticated);
    const app = await page({ user: fixtures.me.lena });
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/playground")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText("Sign in to continue.")).toBeNull();
    expect(toasts()).toEqual([]);
  });
});

describe("Copy as curl", () => {
  async function withClipboard(run: (written: () => string | undefined) => Promise<void>): Promise<void> {
    const before = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    let text: string | undefined;
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
      await run(() => text);
    } finally {
      if (before === undefined) Reflect.deleteProperty(navigator, "clipboard");
      else Object.defineProperty(navigator, "clipboard", before);
    }
  }

  test("the command calls /v1 with a placeholder for the key and the settings on the page", async () => {
    await withClipboard(async (written) => {
      await page();
      await choose(await modelPicker(), "openai/gpt-4o-mini");
      await userEvent.click(screen.getByLabelText("System prompt"));
      await userEvent.paste("Be brief.");
      await userEvent.click(screen.getByLabelText("Max tokens"));
      await userEvent.paste("64");
      await userEvent.click(message());
      await userEvent.paste("Say hello");
      await userEvent.click(screen.getByRole("button", { name: "Copy as curl" }));
      await waitFor(() => {
        expect(toasts()).toEqual(["Copied."]);
      });
      const command = written() ?? "";
      expect(command).toContain(`curl ${window.location.origin}/v1/chat/completions`);
      expect(command).toContain("-H 'Authorization: Bearer <your key>'");
      const body = /-d '(.*)'$/s.exec(command)?.[1] ?? "";
      expect(JSON.parse(body)).toEqual({
        model: "openai/gpt-4o-mini",
        stream: true,
        max_tokens: 64,
        messages: [
          { role: "system", content: "Be brief." },
          { role: "user", content: "Say hello" },
        ],
      });
      // Nothing of the session is in it.
      expect(command).not.toMatch(/csrf|cookie|uf_session/i);
      // The message is still in the box: copying sends nothing.
      expect(message()).toHaveValue("Say hello");
    });
  });

  test("without a message the command has a sample one", async () => {
    await withClipboard(async (written) => {
      await page();
      await choose(await modelPicker(), "openai/gpt-4o-mini");
      await userEvent.click(screen.getByRole("button", { name: "Copy as curl" }));
      await waitFor(() => {
        expect(written()).toBeDefined();
      });
      expect(written()).toContain('"content":"Hello"');
    });
  });

  test("a clipboard that refuses is said in a toast", async () => {
    const before = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: () => Promise.reject(new Error("no")) },
    });
    try {
      await page();
      await modelPicker();
      await userEvent.click(screen.getByRole("button", { name: "Copy as curl" }));
      await waitFor(() => {
        expect(toasts()).toEqual(["Could not copy. Select the command and copy it by hand."]);
      });
    } finally {
      if (before === undefined) Reflect.deleteProperty(navigator, "clipboard");
      else Object.defineProperty(navigator, "clipboard", before);
    }
  });
});

describe("on a narrow screen", () => {
  test("390 px: one main, one h1, the form and the thread", async () => {
    chats();
    await page({ width: 390 });
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("hi");
    await screen.findByText("Hello");
    expectOneMain();
    expectOneH1("Playground");
    expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(390);
  });
});

const WEATHER = JSON.stringify([
  {
    type: "function",
    function: { name: "weather", parameters: { type: "object", properties: { city: { type: "string" } } } },
  },
]);

const toolEvent = (calls: object[], finish: string | null = null) =>
  `data: ${JSON.stringify({ model: "gpt-4o-mini", choices: [{ index: 0, delta: { tool_calls: calls }, finish_reason: finish }] })}\n\n`;

/** An answer that calls `weather` twice, the pieces of the two interleaved. */
const toolCallStream: readonly string[] = [
  toolEvent([{ index: 0, id: "call_a", type: "function", function: { name: "weather", arguments: "" } }]),
  toolEvent([{ index: 0, function: { arguments: '{"city":' } }]),
  toolEvent([{ index: 1, id: "call_b", type: "function", function: { name: "weather", arguments: '{"city"' } }]),
  toolEvent([{ index: 0, function: { arguments: '"Oslo"}' } }]),
  toolEvent([{ index: 1, function: { arguments: ':"Rome"}' } }], "tool_calls"),
  'data: {"model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}\n\n',
  "data: [DONE]\n\n",
];

async function openTools(): Promise<void> {
  await userEvent.click(await screen.findByRole("button", { name: "Tools" }));
}

async function defineTools(text: string): Promise<void> {
  await openTools();
  await userEvent.click(screen.getByRole("textbox", { name: "Tools" }));
  await userEvent.paste(text);
}

const png = (name = "cat.png") => new File(["x"], name, { type: "image/png" });

describe("images", () => {
  test("an attached image is sent as content parts, with its file name as alt text", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    const input = await screen.findByLabelText("Attach image");
    expect(input).toHaveAttribute("accept", "image/png,image/jpeg,image/gif,image/webp");
    await userEvent.upload(input, png());
    expect(await screen.findByRole("img", { name: "cat.png" })).toBeInTheDocument();
    await say("what is this");
    await screen.findByText("Hello");
    expect(sent).toEqual([
      {
        model: "openai/gpt-4o-mini",
        stream: true,
        messages: [
          {
            role: "user",
            content: [
              { type: "text", text: "what is this" },
              { type: "image_url", image_url: { url: "data:image/png;base64,eA==" } },
            ],
          },
        ],
      },
    ]);
    // The image stays in the thread, and the composer is empty again.
    expect(within(screen.getByRole("list", { name: "Conversation" })).getByRole("img", { name: "cat.png" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove cat.png" })).toBeNull();
  });

  test("an image can be removed before sending", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await userEvent.upload(await screen.findByLabelText("Attach image"), png());
    await userEvent.click(await screen.findByRole("button", { name: "Remove cat.png" }));
    expect(screen.queryByRole("img", { name: "cat.png" })).toBeNull();
    await say("hi");
    await screen.findByText("Hello");
    expect(sent[0]).toMatchObject({ messages: [{ role: "user", content: "hi" }] });
  });

  test("an image over 5 MB is not attached and is said so", async () => {
    await page();
    await modelPicker();
    const big = new File([new Uint8Array(5 * 1024 * 1024 + 1)], "big.png", { type: "image/png" });
    await userEvent.upload(screen.getByLabelText("Attach image"), big);
    expect(await screen.findByText("Images over 5 MB are not sent.")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "big.png" })).toBeNull();
  });

  test("an image of exactly 5 MB is attached", async () => {
    await page();
    await modelPicker();
    const edge = new File([new Uint8Array(5 * 1024 * 1024)], "edge.png", { type: "image/png" });
    await userEvent.upload(screen.getByLabelText("Attach image"), edge);
    expect(await screen.findByRole("img", { name: "edge.png" })).toBeInTheDocument();
  });
});

describe("tools", () => {
  test("the tools and the choice are sent", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await defineTools(WEATHER);
    expect(await optionsOf(screen.getByRole("combobox", { name: "Tool choice" }))).toEqual([
      "auto",
      "none",
      "required",
      "weather",
    ]);
    await choose(screen.getByRole("combobox", { name: "Tool choice" }), "weather");
    await say("hi");
    await screen.findByText("Hello");
    expect(sent[0]).toMatchObject({
      tools: JSON.parse(WEATHER) as unknown,
      tool_choice: { type: "function", function: { name: "weather" } },
    });
  });

  test("without tools the choice offers auto and none only, and none is sent", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await openTools();
    expect(await optionsOf(screen.getByRole("combobox", { name: "Tool choice" }))).toEqual(["auto", "none"]);
    await choose(screen.getByRole("combobox", { name: "Tool choice" }), "none");
    await say("hi");
    await screen.findByText("Hello");
    expect(sent[0]).not.toHaveProperty("tools");
    expect(sent[0]).not.toHaveProperty("tool_choice");
  });

  test("a choice that is no longer offered is not sent", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await defineTools(WEATHER);
    await choose(screen.getByRole("combobox", { name: "Tool choice" }), "required");
    await userEvent.clear(screen.getByRole("textbox", { name: "Tools" }));
    await say("hi");
    await screen.findByText("Hello");
    expect(sent[0]).not.toHaveProperty("tool_choice");
  });

  test("tools that are not valid JSON functions block Send, say why, and keep the message", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await defineTools('[{"type":"function"}]');
    await say("hi");
    const field = screen.getByRole("textbox", { name: "Tools" });
    expect(field).toHaveAccessibleDescription(/Tools must be a JSON array of functions, each with a type of function and a name\./);
    expect(field).toBeInvalid();
    await settle();
    expect(sent).toEqual([]);
    expect(message()).toHaveValue("hi");
  });

  test("streamed tool calls are shown whole, and the results go back as tool messages", async () => {
    let turn = 0;
    const sent = chats(() => {
      turn += 1;
      return eventStream(turn === 1 ? toolCallStream : fixtures.playgroundChunks);
    });
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await defineTools(WEATHER);
    await say("weather in Oslo and Rome?");
    const calls = await screen.findAllByRole("group", { name: /^Tool call weather/ });
    expect(calls).toHaveLength(2);
    expect(calls[0]?.querySelector("pre")?.textContent).toBe('{\n  "city": "Oslo"\n}');
    expect(calls[1]?.querySelector("pre")?.textContent).toBe('{\n  "city": "Rome"\n}');
    // The next message waits for the results.
    expect(sendButton()).toBeDisabled();

    // Nothing is sent without a result for every call.
    await userEvent.click(within(calls[0] as HTMLElement).getByLabelText(/^Tool result for weather/));
    await userEvent.paste("3 degrees");
    await userEvent.click(screen.getByRole("button", { name: "Send results" }));
    expect(await screen.findByText("Give a result for every call.")).toBeInTheDocument();
    expect(sent).toHaveLength(1);

    await userEvent.click(within(calls[1] as HTMLElement).getByLabelText(/^Tool result for weather/));
    await userEvent.paste("20 degrees");
    await userEvent.click(screen.getByRole("button", { name: "Send results" }));
    expect(await screen.findByText("Hello")).toBeInTheDocument();
    expect(sent).toHaveLength(2);
    const second = sent[1] as { messages: unknown[] };
    expect(second.messages).toEqual([
      { role: "user", content: "weather in Oslo and Rome?" },
      {
        role: "assistant",
        content: null,
        tool_calls: [
          { id: "call_a", type: "function", function: { name: "weather", arguments: '{"city":"Oslo"}' } },
          { id: "call_b", type: "function", function: { name: "weather", arguments: '{"city":"Rome"}' } },
        ],
      },
      { role: "tool", content: "3 degrees", tool_call_id: "call_a" },
      { role: "tool", content: "20 degrees", tool_call_id: "call_b" },
    ]);
    // The results are answered: no more fields for them, and Send works again.
    expect(screen.queryByRole("button", { name: "Send results" })).toBeNull();
    await userEvent.click(message());
    await userEvent.paste("thanks");
    expect(sendButton()).toBeEnabled();
  });

  test("a call can be copied", async () => {
    const written: string[] = [];
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: (value: string) => {
          written.push(value);
          return Promise.resolve();
        },
      },
    });
    try {
      chats(() => eventStream(toolCallStream));
      await page();
      await choose(await modelPicker(), "openai/gpt-4o-mini");
      await say("go");
      const calls = await screen.findAllByRole("group", { name: /^Tool call weather/ });
      await userEvent.click(within(calls[0] as HTMLElement).getByRole("button", { name: "Copy arguments" }));
      await waitFor(() => {
        expect(toasts()).toEqual(["Copied."]);
      });
      expect(written).toEqual(['{\n  "city": "Oslo"\n}']);
    } finally {
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  test("the thread at 390 px with tool calls fits the screen", async () => {
    chats(() => eventStream(toolCallStream));
    await page({ width: 390 });
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("go");
    await screen.findAllByRole("group", { name: /^Tool call weather/ });
    expectOneMain();
    expectOneH1("Playground");
    expectLabelsNameControls(document.body);
    expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(390);
  });

  test("Copy as curl has the tools and the image parts, the image data cut short", async () => {
    let command = "";
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: (value: string) => {
          command = value;
          return Promise.resolve();
        },
      },
    });
    try {
      await page();
      await choose(await modelPicker(), "openai/gpt-4o-mini");
      await defineTools(WEATHER);
      await userEvent.upload(screen.getByLabelText("Attach image"), png());
      await screen.findByRole("img", { name: "cat.png" });
      await userEvent.click(message());
      await userEvent.paste("look");
      await userEvent.click(screen.getByRole("button", { name: "Copy as curl" }));
      await waitFor(() => {
        expect(command).not.toBe("");
      });
      expect(command).toContain('"tools":[{"type":"function"');
      expect(command).toContain('"image_url":{"url":"data:image/png;base64,…"}');
      expect(command).not.toContain("eA==");
      expect(command).toContain("# image data omitted");
    } finally {
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });
});

describe("fix round 1", () => {
  test("invalid tools with the section collapsed open it, show the error and take the focus", async () => {
    const sent = chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await defineTools("nope");
    const toggle = screen.getByRole("button", { name: "Tools" });
    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await say("hi");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("textbox", { name: "Tools" })).toHaveFocus();
    expect(screen.getByText(/Tools must be a JSON array/)).toBeVisible();
    expect(sent).toEqual([]);
  });

  test("the collapsed Tools button says how many tools are defined", async () => {
    await page();
    await modelPicker();
    await defineTools(WEATHER);
    expect(screen.getByRole("button", { name: "Tools (1)" })).toBeInTheDocument();
  });

  const big = (name: string) => new File([new Uint8Array(5 * 1024 * 1024)], name, { type: "image/png" });

  test("images that are too large together are refused, each file with its own error, and Remove clears them", async () => {
    await page();
    await modelPicker();
    await userEvent.upload(screen.getByLabelText("Attach image"), [big("a.png"), big("b.png"), big("c.png")]);
    expect(await screen.findByRole("img", { name: "a.png" })).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "b.png" })).toBeNull();
    const errors = screen.getAllByText("These images are too large to send together. Remove one.");
    expect(errors).toHaveLength(2);
    await userEvent.click(screen.getByRole("button", { name: "Remove a.png" }));
    expect(screen.queryByText(/too large to send together/)).toBeNull();
  });

  test("every failing file is told, not only the last", async () => {
    await page();
    await modelPicker();
    const gif = new File(["x"], "doc.pdf", { type: "application/pdf" });
    const huge = new File([new Uint8Array(5 * 1024 * 1024 + 1)], "huge.png", { type: "image/png" });
    await userEvent.upload(screen.getByLabelText("Attach image"), [gif, huge], { applyAccept: false });
    expect(await screen.findByText("Only PNG, JPEG, GIF and WebP images are sent.")).toBeInTheDocument();
    expect(screen.getByText("Images over 5 MB are not sent.")).toBeInTheDocument();
  });

  test("images of the thread count: the next one is refused and New conversation is suggested", async () => {
    chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await userEvent.upload(screen.getByLabelText("Attach image"), big("a.png"));
    await screen.findByRole("img", { name: "a.png" });
    await say("one");
    await screen.findByText("Hello");
    await userEvent.upload(screen.getByLabelText("Attach image"), big("b.png"));
    expect(await screen.findByText("These images are too large to send together. Remove one.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove b.png" })).toBeNull();
  });

  test("a successful send clears the image errors", async () => {
    chats();
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await userEvent.upload(screen.getByLabelText("Attach image"), png());
    await userEvent.upload(screen.getByLabelText("Attach image"), big("big.png"), { applyAccept: false });
    await userEvent.upload(
      screen.getByLabelText("Attach image"),
      new File([new Uint8Array(5 * 1024 * 1024 + 1)], "huge.png", { type: "image/png" }),
    );
    expect(await screen.findByText("Images over 5 MB are not sent.")).toBeInTheDocument();
    await say("hi");
    await screen.findByText("Hello");
    expect(screen.queryByText("Images over 5 MB are not sent.")).toBeNull();
  });

  test("a stream that breaks off in a tool call keeps no calls and Send stays usable", async () => {
    const feed = handFed();
    chats(feed.response);
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await defineTools(WEATHER);
    await say("go");
    feed.push(toolEvent([{ index: 0, id: "call_a", type: "function", function: { name: "weather", arguments: '{"ci' } }]));
    await settle();
    feed.close();
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
    });
    expect(screen.queryByRole("group", { name: /^Tool call/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Send results" })).toBeNull();
    await userEvent.click(message());
    await userEvent.paste("again");
    expect(sendButton()).toBeEnabled();
  });

  test("the focus goes to the first result when tool calls arrive, and to the next chip on Remove", async () => {
    chats(() => eventStream(toolCallStream));
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await defineTools(WEATHER);
    await say("go");
    const groups = await screen.findAllByRole("group", { name: /^Tool call weather/ });
    await waitFor(() => {
      expect(within(groups[0] as HTMLElement).getByLabelText(/^Tool result for weather/)).toHaveFocus();
    });
  });

  test("Remove moves the focus to the next chip, or to the attach control", async () => {
    await page();
    await modelPicker();
    await userEvent.upload(screen.getByLabelText("Attach image"), [png("a.png"), png("b.png")]);
    await screen.findByRole("img", { name: "b.png" });
    await userEvent.click(screen.getByRole("button", { name: "Remove a.png" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Remove b.png" })).toHaveFocus();
    });
    await userEvent.click(screen.getByRole("button", { name: "Remove b.png" }));
    await waitFor(() => {
      expect(screen.getByLabelText("Attach image")).toHaveFocus();
    });
  });

  test("calls of the same function are told apart by their number", async () => {
    chats(() => eventStream(toolCallStream));
    await page();
    await choose(await modelPicker(), "openai/gpt-4o-mini");
    await say("go");
    expect(await screen.findByRole("group", { name: "Tool call weather (1)" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Tool call weather (2)" })).toBeInTheDocument();
    expect(screen.getByLabelText("Tool result for weather (2)")).toBeInTheDocument();
  });
});
