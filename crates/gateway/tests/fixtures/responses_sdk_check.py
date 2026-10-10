"""Reads what the gateway sent on /v1/responses and parses it with the official
`openai` SDK: the stream through `client.responses.stream` (the SDK's own stream
state machine) and every event and the response through its pydantic models.

usage: responses_sdk_check.py <stream.sse> <response.json> <expected text> <expected calls>
"""
import json
import sys

try:
    import httpx
except ImportError:  # recent SDK releases ship httpx under another name
    import httpx2 as httpx
from openai import OpenAI
from openai.types.responses import Response, ResponseStreamEvent
from pydantic import TypeAdapter

sse_path, json_path, want_text, want_calls = sys.argv[1:5]
sse = open(sse_path, "rb").read()
body = json.load(open(json_path))

# Every raw event validates against the SDK's models, and sequence numbers count.
adapter = TypeAdapter(ResponseStreamEvent)
raw = []
for block in sse.decode().split("\n\n"):
    if not block.strip():
        continue
    data = next(l[6:] for l in block.split("\n") if l.startswith("data: "))
    raw.append(adapter.validate_python(json.loads(data)))
assert [e.sequence_number for e in raw] == list(range(len(raw))), "sequence_number gap"

# The SDK's stream parser, fed the gateway's bytes.
seen = []
client = OpenAI(
    api_key="k",
    base_url="http://gateway.test/v1",
    http_client=httpx.Client(
        transport=httpx.MockTransport(
            lambda request: httpx.Response(
                200, content=sse, headers={"content-type": "text/event-stream"}
            )
        )
    ),
)
with client.responses.stream(model="p/m", input="x") as stream:
    for event in stream:
        seen.append(event.type)
    final = stream.get_final_response()
assert "response.output_text.delta" in seen or want_text == "", seen
assert final.status in ("completed", "incomplete"), final.status
text = "".join(
    c.text for item in final.output if item.type == "message" for c in item.content
)
assert text == want_text, (text, want_text)
calls = [(i.name, i.arguments) for i in final.output if i.type == "function_call"]
assert json.dumps(calls) == want_calls, (calls, want_calls)

# The non-streamed object validates as a Response.
whole = Response.model_validate(body)
assert whole.object == "response" and whole.id.startswith("resp_")
print("ok", len(raw), "events")
