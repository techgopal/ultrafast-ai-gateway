// Audio mode of the playground: a file sent to `/api/playground/transcriptions`
// for its transcript, and a text sent to `/api/playground/speech` to be played
// back. The audio plays from a `blob:` URL made in the browser; it is never
// fetched from anywhere else.
import { useEffect, useMemo, useRef, useState } from "react";
import { playgroundSpeech, playgroundTranscribe } from "@/api/client";
import { failureText } from "@/pages/PlaygroundRun";
import type { components } from "@/api/schema";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { FilterSelect, type Choice } from "@/components/FilterSelect";
import { useToast } from "@/components/toast";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { refOf } from "@/lib/models";
import {
  checkAudioFile,
  checkSpeechInput,
  speechCurlOf,
  speechRequestBody,
  transcriptionCurlOf,
  transcriptionForm,
  transcriptOf,
  VOICES,
  type Voice,
} from "@/lib/playground";
import { DONE } from "@/pages/Playground";

type Model = components["schemas"]["ModelView"];

export const NO_TRANSCRIPT = "The answer holds no transcript to show here.";
export const NO_FILE = "Choose an audio file to transcribe.";
export const NO_TEXT = "Write the text to speak.";

const VOICE_CHOICES: Choice[] = VOICES.map((voice) => ({ value: voice, label: voice }));

/** The models and routes on offer, as choices. */
function useChoices(models: readonly Model[], routes: readonly string[]): Choice[] {
  return useMemo<Choice[]>(
    () => [
      ...models
        .filter((model) => model.enabled)
        .map((model) => ({ value: refOf(model), label: refOf(model) }))
        .sort((a, b) => a.label.localeCompare(b.label)),
      ...[...routes].sort().map((name) => ({ value: name, label: `${name} (route)` })),
    ],
    [models, routes],
  );
}

/** A choice that is no longer offered is never sent. */
function effective(choices: readonly Choice[], chosen: string): string {
  return choices.some((choice) => choice.value === chosen) ? chosen : (choices[0]?.value ?? "");
}

function Failure({ text }: { text: string | null }) {
  return text === null ? null : (
    <Alert variant="destructive">
      <AlertDescription>
        <p>{text}</p>
      </AlertDescription>
    </Alert>
  );
}

function Transcribe({ choices }: { choices: readonly Choice[] }) {
  const toast = useToast();
  const [chosen, setChosen] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [fileError, setFileError] = useState<string | undefined>(undefined);
  const [language, setLanguage] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<string | null>(null);
  const controller = useRef<AbortController | null>(null);
  const target = effective(choices, chosen);

  useEffect(
    () => () => {
      controller.current?.abort();
    },
    [],
  );

  async function run() {
    if (running || target === "") return;
    if (file === null) {
      setFileError(NO_FILE);
      return;
    }
    const refused = checkAudioFile(file);
    setFileError(refused);
    if (refused !== undefined) return;
    const abort = new AbortController();
    controller.current = abort;
    setRunning(true);
    setError(null);
    try {
      const answer = await playgroundTranscribe(transcriptionForm(target, language, file), abort.signal);
      setTranscript(transcriptOf(answer) ?? "");
    } catch (caught) {
      if (!abort.signal.aborted) {
        setTranscript(null);
        setError(failureText(caught));
      }
    }
    if (controller.current === abort) controller.current = null;
    setRunning(false);
  }

  async function copyCurl() {
    if (target === "") return;
    try {
      await navigator.clipboard.writeText(transcriptionCurlOf(window.location.origin, target, language));
      toast(DONE.copied);
    } catch {
      toast(DONE.notCopied, "error");
    }
  }

  return (
    <section aria-labelledby="transcribe-title" className="flex min-w-0 flex-col gap-4">
      <h2 id="transcribe-title" className="text-base font-medium">
        Transcribe
      </h2>
      <form
        aria-label="Transcription"
        noValidate
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          void run();
        }}
      >
        <div className="flex flex-col gap-2">
          <p className="text-sm font-medium">Transcription model or route</p>
          <FilterSelect label="Transcription model or route" value={target} choices={choices} onChange={setChosen} />
          <p className="text-sm text-muted-foreground">
            Pick a model that transcribes audio. The gateway refuses files above its audio cap.
          </p>
        </div>
        <Field label="Audio file" name="file" error={fileError}>
          {({ id, name, ...described }) => (
            <Input
              {...described}
              id={id}
              name={name}
              type="file"
              accept="audio/*,video/mp4,video/webm,.m4a,.mp3,.mp4,.mpeg,.mpga,.wav,.webm,.ogg,.flac"
              className={control}
              onChange={(event) => {
                const chosenFile = event.target.files?.[0] ?? null;
                setFile(chosenFile);
                setFileError(chosenFile === null ? undefined : checkAudioFile(chosenFile));
              }}
            />
          )}
        </Field>
        <Field label="Language" name="language" hint="Optional. A code such as en.">
          {({ id, name, ...described }) => (
            <Input
              {...described}
              id={id}
              name={name}
              autoComplete="off"
              className={control}
              value={language}
              onChange={(event) => {
                setLanguage(event.target.value);
              }}
            />
          )}
        </Field>
        <div className="flex flex-wrap gap-2">
          <Button type="submit" className={control} disabled={target === "" || running}>
            Transcribe
          </Button>
          <Button type="button" variant="outline" className={control} onClick={() => void copyCurl()}>
            Copy transcription as curl
          </Button>
        </div>
      </form>
      <Failure text={error} />
      {/* Always on the page, so that a screen reader announces the text when it appears. */}
      <p role="status" className="min-h-5 text-sm text-muted-foreground">
        {running ? "Transcribing..." : ""}
      </p>
      {transcript === null || running ? null : transcript === "" ? (
        <p className="text-sm text-muted-foreground">{NO_TRANSCRIPT}</p>
      ) : (
        <div role="region" aria-labelledby="transcript-title" className="flex flex-col gap-1">
          <h3 id="transcript-title" className="text-sm font-medium">
            Transcript
          </h3>
          <p className="whitespace-pre-wrap rounded-md border p-3 text-sm">{transcript}</p>
        </div>
      )}
    </section>
  );
}

function Speak({ choices }: { choices: readonly Choice[] }) {
  const toast = useToast();
  const [chosen, setChosen] = useState("");
  const [voice, setVoice] = useState<Voice>("alloy");
  const [text, setText] = useState("");
  const [textError, setTextError] = useState<string | undefined>(undefined);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [audio, setAudio] = useState<string | null>(null);
  const controller = useRef<AbortController | null>(null);
  const target = effective(choices, chosen);
  const typed = text.trim();

  // The audio of an earlier call, and the one on screen when the page ends,
  // are given back.
  useEffect(
    () => () => {
      controller.current?.abort();
    },
    [],
  );
  useEffect(
    () => () => {
      if (audio !== null) URL.revokeObjectURL(audio);
    },
    [audio],
  );

  /** The body to send, or `null` when the text is not valid (the field says why). */
  function bodyOf() {
    const refused = checkSpeechInput(typed);
    setTextError(refused);
    return refused === undefined ? speechRequestBody(target, typed, voice) : null;
  }

  async function speak() {
    if (running || target === "" || typed === "") return;
    const body = bodyOf();
    if (body === null) return;
    const abort = new AbortController();
    controller.current = abort;
    setRunning(true);
    setError(null);
    try {
      const blob = await playgroundSpeech(body, abort.signal);
      setAudio(URL.createObjectURL(blob));
    } catch (caught) {
      if (!abort.signal.aborted) {
        setAudio(null);
        setError(failureText(caught));
      }
    }
    if (controller.current === abort) controller.current = null;
    setRunning(false);
  }

  async function copyCurl() {
    if (target === "") return;
    const body = bodyOf();
    if (body === null) return;
    const withText = typed === "" ? { ...body, input: "Hello from the gateway." } : body;
    try {
      await navigator.clipboard.writeText(speechCurlOf(window.location.origin, withText));
      toast(DONE.copied);
    } catch {
      toast(DONE.notCopied, "error");
    }
  }

  return (
    <section aria-labelledby="speak-title" className="flex min-w-0 flex-col gap-4">
      <h2 id="speak-title" className="text-base font-medium">
        Speak
      </h2>
      <form
        aria-label="Speech request"
        noValidate
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          void speak();
        }}
      >
        <div className="flex flex-col gap-2">
          <p className="text-sm font-medium">Speech model or route</p>
          <FilterSelect label="Speech model or route" value={target} choices={choices} onChange={setChosen} />
          <p className="text-sm text-muted-foreground">
            Pick a model that speaks. Calls count toward your limits and budgets.
          </p>
        </div>
        <div className="flex flex-col gap-2">
          <p className="text-sm font-medium">Voice</p>
          <FilterSelect
            label="Voice"
            value={voice}
            choices={VOICE_CHOICES}
            onChange={(value) => {
              setVoice(value as Voice);
            }}
          />
        </div>
        <Field label="Text to speak" name="input" error={textError}>
          {({ id, name, ...described }) => (
            <Textarea
              {...described}
              id={id}
              name={name}
              value={text}
              onChange={(event) => {
                setText(event.target.value);
              }}
            />
          )}
        </Field>
        <div className="flex flex-wrap gap-2">
          <Button type="submit" className={control} disabled={target === "" || typed === "" || running}>
            Speak
          </Button>
          <Button type="button" variant="outline" className={control} onClick={() => void copyCurl()}>
            Copy speech as curl
          </Button>
        </div>
      </form>
      <Failure text={error} />
      <p role="status" className="min-h-5 text-sm text-muted-foreground">
        {running ? "Making the audio..." : ""}
      </p>
      {audio === null || running ? null : (
        // The audio is made from the text above; there is no recording to caption.
        // eslint-disable-next-line jsx-a11y/media-has-caption
        <audio controls src={audio} aria-label="Speech" className="w-full" />
      )}
    </section>
  );
}

export function AudioMode({ models, routes }: { models: readonly Model[]; routes: readonly string[] }) {
  const choices = useChoices(models, routes);
  return (
    <div className="grid gap-8 lg:grid-cols-2">
      <Transcribe choices={choices} />
      <Speak choices={choices} />
    </div>
  );
}
