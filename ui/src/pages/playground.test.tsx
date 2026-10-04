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
