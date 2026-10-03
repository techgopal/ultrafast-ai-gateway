# Parity fixtures

One set of cases, run by the Rust (`crates/client/tests/fixtures.rs`), Python
(`crates/client-py/tests/test_fixtures.py`) and TypeScript
(`clients/ts/tests/fixtures.test.ts`) test suites. A change to a wire format,
an error rule or the stream decoder shows up as the same failing case in all
three.

| File | Each case |
| --- | --- |
| `requests.json` | a target and a request, and the HTTP request on the wire (method, path, headers, body as JSON); or an error and nothing sent |
| `responses.json` | a status, headers and body from the server, and the parsed result or the error |
| `errors.json` | an HTTP answer of 300 or more, and the error (`kind`, `retryable`, `status`, `retry_after`, `message`) |
| `streams.json` | the chunks of an event-stream body (tried in pieces and as one chunk), and the events, then the error or none |

`{base}` in a target's `base_url` is replaced by the address of a local mock
server. The key in the file is a fake. A case without `expect...message` does
not compare the message (the wording is a JSON parser's). Add a case by adding
it to the file; all three suites pick it up. Expected values are the contract:
do not regenerate them from one client's output without reading the diff.
