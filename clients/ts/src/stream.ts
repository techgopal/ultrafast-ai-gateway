import { HostFailure, asBytes, timed } from "./http.js";
import type { StreamEvent, Usage } from "./types.js";
import type { Wasm } from "./wasm.js";

interface WireUsage {
  input_tokens: number;
  output_tokens: number;
}

export const usageOf = (u: WireUsage | null): Usage | null =>
  u ? { inputTokens: u.input_tokens, outputTokens: u.output_tokens } : null;

type WireEvent =
  | { type: "delta"; text: string }
  | { type: "tool_call_start"; index: number; id: string; name: string }
  | { type: "tool_call_delta"; index: number; arguments: string }
  | { type: "done"; finish_reason: string | null; usage: WireUsage | null };

function* events(json: string): Generator<StreamEvent> {
  for (const e of JSON.parse(json) as WireEvent[]) {
    yield e.type === "done" ? { type: "done", finishReason: e.finish_reason, usage: usageOf(e.usage) } : e;
  }
}

/**
 * Events in order, then at most one error (thrown as the wasm module's error
 * JSON string or a `HostFailure`; the caller turns it into an
 * `UltrafastError`): a decoder error, a broken connection, a silent stretch
 * longer than `idleMs`, or a close before `done`. Never a silent end.
 * Whatever way it ends, the body is cancelled and the request aborted.
 */
export async function* decode(
  w: Wasm,
  kind: string,
  resp: Response,
  idleMs: number,
  ctl: AbortController,
): AsyncGenerator<StreamEvent> {
  const decoder = new w.StreamDecoder(kind);
  const reader = resp.body?.getReader();
  try {
    if (reader) {
      for (;;) {
        const next = await timed(
          reader.read().catch((e: unknown) => {
            const aborted = (e as { name?: unknown } | null)?.name;
            throw aborted === "AbortError" || aborted === "TimeoutError"
              ? new HostFailure("timeout", "the request timed out")
              : new HostFailure("network", "network error: the connection broke during the stream");
          }),
          idleMs,
          ctl,
          "the stream went quiet",
        );
        if (next.done) break;
        yield* events(decoder.feed(asBytes(next.value)));
        const held = decoder.takeError();
        if (held !== undefined) throw held;
        if (decoder.isDone()) return;
      }
    }
    yield* events(decoder.finish());
    const held = decoder.takeError();
    if (held !== undefined) throw held;
    if (!decoder.isDone()) throw new HostFailure("malformed", "the stream ended before it was complete");
  } finally {
    decoder.free();
    reader?.cancel().catch(() => undefined);
    ctl.abort();
  }
}
