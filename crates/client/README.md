# ultrafast-client

Rust client for an Ultrafast gateway, or for a provider directly. Built on
`ultrafast-translate` (request building and response parsing are the code
the gateway uses; error classification and stream decoding are shared with the
other clients) and `reqwest` with rustls.

```rust
use futures::StreamExt;
use ultrafast_client::{ChatRequest, Client, EmbeddingsRequest, Target};

async fn run() -> Result<(), ultrafast_client::Error> {
    let client = Client::new(Target::gateway("http://127.0.0.1:3000", "uf-key"));

    let reply = client
        .chat(ChatRequest::new("gpt-4o").system("Be brief.").user("Hello").tag("team", "search"))
        .await?;
    println!("{}", reply.content);

    let mut events = Box::pin(client.chat_stream(ChatRequest::new("gpt-4o").user("Hello")).await?);
    while let Some(event) = events.next().await {
        println!("{:?}", event?);
    }

    let vectors = client.embed(EmbeddingsRequest::new("text-embedding-3-small", ["a", "b"])).await?;
    Ok(())
}
```

## Targets

| Constructor | Notes |
| --- | --- |
| `Target::gateway(base_url, key)` | OpenAI wire format against `{base_url}/v1`; a trailing `/v1` on `base_url` is ignored. Sends `tags`. |
| `Target::openai(key)` | `https://api.openai.com/v1`; `.with_base_url(..)` replaces it. |
| `Target::openai_compatible(base_url, key)` | Groq, Mistral, OpenRouter, Ollama; `base_url` includes `/v1`. |
| `Target::anthropic(key)` | |
| `Target::gemini(key)` | |
| `Target::azure(endpoint, key)` | The request's model is the deployment; `.with_api_version(..)`. |

Anthropic has no embeddings API: `embed` returns an `invalid_request` error
without sending anything.

## Tools and images

```rust
use ultrafast_client::{ChatRequest, Tool, ToolChoice};

let req = ChatRequest::new("gpt-4o")
    .user("What is in this picture, and what is the weather there?")
    .image("https://example.com/photo.png")? // or a base64 `data:image/png;base64,...` URL
    .tool(Tool { name: "weather".into(), description: Some("Current weather".into()),
                 parameters: serde_json::json!({"type": "object"}) })
    .tool_choice(ToolChoice::Auto)
    .parallel_tool_calls(true);
let reply = client.chat(req).await?;
for call in &reply.tool_calls { /* call.id, call.name, call.arguments (JSON text) */ }

// Send the result back:
let next = ChatRequest::new("gpt-4o")
    .user("...")
    .assistant_tool_calls("", reply.tool_calls.clone())
    .tool_result(&reply.tool_calls[0].id, r#"{"temp_c": 18}"#);
```

`image` returns an error for a URL that is neither http(s) nor a base64 data
URL, or an image type other than png, jpeg, gif and webp. Streams carry
`StreamEvent::ToolCallStart` and `ToolCallDelta`. Providers that cannot do a
thing (images on a text-only model, say) answer with an `InvalidRequest` error.
The same requests work against a gateway and against a provider directly.

## Errors

Every call returns `Error { kind, message, status, retryable, retry_after }`.
`kind` is one of `auth`, `permission`, `not_found`, `invalid_request`,
`rate_limited`, `upstream`, `network`, `timeout`, `malformed`. `retryable` is
true for `rate_limited`, `upstream`, `network` and `timeout`. `retry_after`
is set from a `Retry-After` header on 429 and 503 answers, in seconds (an HTTP date is not read), capped at 24 h. A redirect is refused as `invalid_request` and never followed.
The client does not retry, route, cache or break circuits.

A stream yields its events and then, if something went wrong, one error: a
provider error event, a broken connection, a silence longer than the timeout,
or a close before the final event. It never just stops short.

## Tags

`tags` are sent as the `x-uf-tags` header (JSON, at most 1 KiB) to a gateway
target and never to a provider. Larger tags are refused before sending.

## Safety

The key is never in `Debug`, `Display` or an error message (any echo of it in
a server's message is replaced). Redirects are not followed, so a provider
key header cannot be forwarded to another host; if you pass your own
`reqwest::Client` with `with_http`, keep it that way.

`with_timeout` bounds a whole `chat`/`embed` call (default 120 s); for
`chat_stream` it bounds the wait for the answer and the silence between two
chunks.

## Test

```
cargo test -p ultrafast-client
```

`tests/fixtures.rs` runs the parity fixtures in `clients/fixtures/*.json`
(request to wire request, answer to parsed result or error, stream chunks to
events) against a local mock server. The Python and TypeScript clients run the
same files, so all three agree on the wire format, error kinds, retry rules and
stream events.
