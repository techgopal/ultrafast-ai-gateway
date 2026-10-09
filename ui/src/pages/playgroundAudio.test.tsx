import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, beforeEach, describe, expect, test } from "vitest";
import { pipelineErrors } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { startGateway } from "@/test/gateway";
import { audioAnswer, ok, override, refusePipeline } from "@/test/handlers";
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

const made: string[] = [];
const revoked: string[] = [];
beforeEach(() => {
  made.length = 0;
  revoked.length = 0;
  let next = 0;
  Object.defineProperty(URL, "createObjectURL", {
    configurable: true,
    writable: true,
    value: () => {
      next += 1;
      const url = `blob:test/${String(next)}`;
      made.push(url);
      return url;
    },
  });
  Object.defineProperty(URL, "revokeObjectURL", {
    configurable: true,
    writable: true,
    value: (url: string) => {
      revoked.push(url);
    },
  });
  return () => {
    Reflect.deleteProperty(URL, "createObjectURL");
    Reflect.deleteProperty(URL, "revokeObjectURL");
  };
});

function page(options: { user?: fixtures.Me } = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/playground", ...options });
}

async function audio(): Promise<void> {
  await page();
  await userEvent.click(await screen.findByRole("button", { name: "Audio" }));
}

const transcribe = () => screen.getByRole("button", { name: "Transcribe" });
const speak = () => screen.getByRole("button", { name: "Speak" });
const sttPicker = () => screen.getByRole("combobox", { name: "Transcription model or route" });
const ttsPicker = () => screen.getByRole("combobox", { name: "Speech model or route" });
const fileField = () => screen.getByLabelText("Audio file");
const text = () => screen.getByRole("textbox", { name: "Text to speak" });

const recording = () => new File(["abc"], "talk.mp3", { type: "audio/mpeg" });

interface Form {
  fields: [string, string][];
  file: { name: string; type: string; size: number } | null;
}

/** Records the forms the playground sends for transcripts. */
function transcripts(answer?: () => Response) {
  const sent: Form[] = [];
  override("post", "/api/playground/transcriptions", async ({ request }) => {
    const form = await request.formData();
    const out: Form = { fields: [], file: null };
    for (const [name, value] of form.entries()) {
      if (typeof value === "string") out.fields.push([name, value]);
      else out.file = { name: value.name, type: value.type, size: value.size };
    }
    sent.push(out);
    return answer === undefined ? Response.json({ text: fixtures.playgroundTranscript }) : answer();
  });
  return sent;
}

function speeches(answer: () => Response = audioAnswer) {
  const sent: unknown[] = [];
  override("post", "/api/playground/speech", async ({ request }) => {
    sent.push(await request.json());
    return answer();
  });
  return sent;
}

async function clipboard<T>(run: () => Promise<T>): Promise<{ result: T; text: () => string | undefined }> {
  let copied: string | undefined;
  const before = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: {
      writeText: (value: string) => {
        copied = value;
        return Promise.resolve();
      },
    },
  });
  try {
    return { result: await run(), text: () => copied };
  } finally {
    if (before === undefined) Reflect.deleteProperty(navigator, "clipboard");
    else Object.defineProperty(navigator, "clipboard", before);
  }
}

describe("Audio mode", () => {
  test("Audio is one click from Chat and shows both tools", async () => {
    await page();
    expect(await screen.findByRole("button", { name: "Audio" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByRole("button", { name: "Transcribe" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Audio" }));
    expect(screen.getByRole("button", { name: "Audio" })).toHaveAttribute("aria-pressed", "true");
    expect(await screen.findByRole("button", { name: "Transcribe" })).toBeInTheDocument();
    expect(speak()).toBeInTheDocument();
  });

  test("it has one main and one h1, and its fields are labelled", async () => {
    await audio();
    expectOneMain();
    expectOneH1("Playground");
    expectLabelsNameControls(document.body);
  });

  test("both pickers offer the models and routes of the chat", async () => {
    await audio();
    expect((await optionsOf(sttPicker())).slice(0, 2)).toEqual(["local-llm/llama3.1:8b", "openai/gpt-4o"]);
    expect((await optionsOf(ttsPicker())).slice(0, 2)).toEqual(["local-llm/llama3.1:8b", "openai/gpt-4o"]);
  });
});

describe("Transcribe", () => {
  test("the file, the model and the language go as a form, and the transcript is shown", async () => {
    const sent = transcripts();
    await audio();
    await choose(sttPicker(), "openai/gpt-4o-mini");
    await userEvent.type(screen.getByRole("textbox", { name: "Language" }), "en");
    await userEvent.upload(fileField(), recording());
    await userEvent.click(transcribe());
    expect(await screen.findByRole("region", { name: "Transcript" })).toHaveTextContent(fixtures.playgroundTranscript);
    expect(sent).toEqual([
      {
        fields: [
          ["model", "openai/gpt-4o-mini"],
          ["language", "en"],
        ],
        // The test transport renames the part; the name is pinned in lib/playground.test.ts.
        file: { name: "blob", type: "audio/mpeg", size: 3 },
      },
    ]);
  });

  test("a language left empty is not sent", async () => {
    const sent = transcripts();
    await audio();
    await userEvent.upload(fileField(), recording());
    await userEvent.click(transcribe());
    await screen.findByRole("region", { name: "Transcript" });
    expect(sent[0]?.fields).toEqual([["model", "local-llm/llama3.1:8b"]]);
  });

  test("without a file nothing is sent and the field says so", async () => {
    const sent = transcripts();
    await audio();
    await userEvent.click(transcribe());
    expect(await screen.findByText("Choose an audio file to transcribe.")).toBeInTheDocument();
    expect(sent).toEqual([]);
  });

  test("a file over the default cap is refused on the field and not sent", async () => {
    const sent = transcripts();
    await audio();
    const big = new File(["x"], "big.mp3", { type: "audio/mpeg" });
    Object.defineProperty(big, "size", { value: 26 * 1024 * 1024 });
    await userEvent.upload(fileField(), big);
    expect(await screen.findByText(/larger than 25 MiB/)).toBeInTheDocument();
    await userEvent.click(transcribe());
    expect(sent).toEqual([]);
  });

  test("the cap is the gateway's own: a smaller one refuses, a larger one lets a file through", async () => {
    const sent = transcripts();
    override("get", "/api/playground/config", () =>
      ok("get", "/api/playground/config", 200, { max_audio_bytes: 2 * 1024 * 1024 }),
    );
    await audio();
    const file = new File(["x"], "mid.mp3", { type: "audio/mpeg" });
    Object.defineProperty(file, "size", { value: 3 * 1024 * 1024 });
    // The cap is read when the page opens: wait for the refusal to be the gateway's.
    await userEvent.upload(fileField(), file);
    expect(await screen.findByText("The file is larger than 2 MiB, the most the gateway takes.")).toBeInTheDocument();
    await userEvent.click(transcribe());
    expect(sent).toEqual([]);
  });

  test("a larger cap than the default lets a 30 MiB file through to the gateway", async () => {
    const sent = transcripts();
    override("get", "/api/playground/config", () =>
      ok("get", "/api/playground/config", 200, { max_audio_bytes: 64 * 1024 * 1024 }),
    );
    await audio();
    const file = new File(["x"], "long.mp3", { type: "audio/mpeg" });
    Object.defineProperty(file, "size", { value: 30 * 1024 * 1024 });
    await waitFor(async () => {
      await userEvent.upload(fileField(), file);
      expect(screen.queryByText(/larger than/)).toBeNull();
    });
    await userEvent.click(transcribe());
    await waitFor(() => {
      expect(sent).toHaveLength(1);
    });
  });

  test("the progress is announced from a status that is always on the page", async () => {
    transcripts();
    await audio();
    const [status] = screen.getAllByRole("status");
    expect(status).toBeEmptyDOMElement();
    await userEvent.upload(fileField(), recording());
    await userEvent.click(transcribe());
    await screen.findByRole("region", { name: "Transcript" });
    // The same element, not one that comes and goes.
    expect(screen.getAllByRole("status")).toContain(status);
  });

  test("an answer with no text says so", async () => {
    transcripts(() => Response.json({}));
    await audio();
    await userEvent.upload(fileField(), recording());
    await userEvent.click(transcribe());
    expect(await screen.findByText("The answer holds no transcript to show here.")).toBeInTheDocument();
  });

  test.each([
    ["a file over the cap", pipelineErrors.tooLarge, "The audio file is too large."],
    ["a model the user may not call", pipelineErrors.forbidden, "You do not have access to model 'openai/gpt-4o'."],
    ["a guardrail that blocks the transcript", pipelineErrors.guardrail, "Blocked by guardrail 'house-rules'."],
  ])("%s is said in place and the file stays", async (_, refusal, said) => {
    transcripts(() => refusePipeline(refusal));
    await audio();
    await userEvent.upload(fileField(), recording());
    await userEvent.click(transcribe());
    expect(await screen.findByRole("alert")).toHaveTextContent(said);
    expect((fileField() as HTMLInputElement).files?.[0]?.name).toBe("talk.mp3");
    expect(screen.queryByRole("region", { name: "Transcript" })).toBeNull();
    expect(toasts()).toEqual([]);
  });

  test("a call that finds the session ended signs out", async () => {
    startGateway({ signedIn: true, me: fixtures.me.lena });
    override("post", "/api/playground/transcriptions", unauthenticated);
    const app = await page({ user: fixtures.me.lena });
    await userEvent.click(await screen.findByRole("button", { name: "Audio" }));
    await userEvent.upload(fileField(), recording());
    await userEvent.click(transcribe());
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/playground")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("Copy transcription as curl is the command for /v1/audio/transcriptions, with a placeholder for the file", async () => {
    const sent = transcripts();
    await audio();
    const { text: copied } = await clipboard(async () => {
      await userEvent.type(screen.getByRole("textbox", { name: "Language" }), "de");
      await userEvent.click(screen.getByRole("button", { name: "Copy transcription as curl" }));
      await waitFor(() => {
        expect(toasts()).toEqual(["Copied."]);
      });
    });
    const command = copied() ?? "";
    expect(command).toContain(`curl ${window.location.origin}/v1/audio/transcriptions`);
    expect(command).toContain("-H 'Authorization: Bearer <your key>'");
    expect(command).toContain("-F file=@audio.mp3");
    expect(command).toContain("-F model='local-llm/llama3.1:8b'");
    expect(command).toContain("-F language='de'");
    expect(command).not.toMatch(/csrf|cookie|uf_session/i);
    expect(sent).toEqual([]);
  });
});

describe("Speak", () => {
  test("the text, the model and the voice go to the gateway, and the audio is played from a blob", async () => {
    const sent = speeches();
    await audio();
    await choose(ttsPicker(), "openai/gpt-4o-mini");
    await choose(screen.getByRole("combobox", { name: "Voice" }), "nova");
    await userEvent.click(text());
    await userEvent.paste("Good morning");
    await userEvent.click(speak());
    const player = await screen.findByLabelText("Speech");
    expect(player.tagName).toBe("AUDIO");
    expect(player).toHaveAttribute("src", "blob:test/1");
    expect(sent).toEqual([{ model: "openai/gpt-4o-mini", input: "Good morning", voice: "nova" }]);
  });

  test("Speak waits for a text", async () => {
    await audio();
    expect(speak()).toBeDisabled();
    await userEvent.click(text());
    await userEvent.paste("hi");
    expect(speak()).toBeEnabled();
  });

  test("a text over 4096 characters is refused on the field and not sent", async () => {
    const sent = speeches();
    await audio();
    await userEvent.click(text());
    await userEvent.paste("a".repeat(4097));
    await userEvent.click(speak());
    expect(await screen.findByText("The text must be at most 4096 characters.")).toBeInTheDocument();
    expect(sent).toEqual([]);
  });

  test("a second call gives the first audio back", async () => {
    speeches();
    await audio();
    await userEvent.click(text());
    await userEvent.paste("hi");
    await userEvent.click(speak());
    await screen.findByLabelText("Speech");
    await userEvent.click(speak());
    await waitFor(() => {
      expect(screen.getByLabelText("Speech")).toHaveAttribute("src", "blob:test/2");
    });
    expect(revoked).toContain("blob:test/1");
  });

  test.each([
    ["a model that cannot speak", pipelineErrors.unavailable, "No provider could serve this request."],
    ["a guardrail that blocks the text", pipelineErrors.guardrail, "Blocked by guardrail 'house-rules'."],
  ])("%s is said in place and the text stays", async (_, refusal, said) => {
    speeches(() => refusePipeline(refusal));
    await audio();
    await userEvent.click(text());
    await userEvent.paste("hi");
    await userEvent.click(speak());
    expect(await screen.findByRole("alert")).toHaveTextContent(said);
    expect(text()).toHaveValue("hi");
    expect(screen.queryByLabelText("Speech")).toBeNull();
    expect(made).toEqual([]);
  });

  test("a call that finds the session ended signs out", async () => {
    startGateway({ signedIn: true, me: fixtures.me.lena });
    override("post", "/api/playground/speech", unauthenticated);
    const app = await page({ user: fixtures.me.lena });
    await userEvent.click(await screen.findByRole("button", { name: "Audio" }));
    await userEvent.click(text());
    await userEvent.paste("hi");
    await userEvent.click(speak());
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/playground")}`);
    });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("Copy speech as curl is the command for /v1/audio/speech with the settings on the page", async () => {
    const sent = speeches();
    await audio();
    await userEvent.click(text());
    await userEvent.paste("it's me");
    const { text: copied } = await clipboard(async () => {
      await userEvent.click(screen.getByRole("button", { name: "Copy speech as curl" }));
      await waitFor(() => {
        expect(toasts()).toEqual(["Copied."]);
      });
    });
    const command = copied() ?? "";
    expect(command).toContain(`curl ${window.location.origin}/v1/audio/speech`);
    expect(command).toContain("-H 'Authorization: Bearer <your key>'");
    expect(command).not.toMatch(/csrf|cookie|uf_session/i);
    const body = /-d '(.*)'/.exec(command)?.[1]?.replace(/'"'"'/g, "'") ?? "";
    expect(JSON.parse(body)).toEqual({ model: "local-llm/llama3.1:8b", input: "it's me", voice: "alloy" });
    expect(sent).toEqual([]);
    expect(text()).toHaveValue("it's me");
  });
});
