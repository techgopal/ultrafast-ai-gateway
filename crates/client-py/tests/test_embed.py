import json

import ultrafast
from conftest import KEY, OPENAI_EMB, Script


def test_embed_through_a_gateway(serve):
    s = serve(Script.json(200, OPENAI_EMB))
    c = ultrafast.Client(ultrafast.gateway(s.url, KEY))
    r = c.embed("te3", ["a", "b"], dimensions=1, tags={"team": "x"})
    assert isinstance(r, ultrafast.EmbeddingsResponse)
    assert r.model == "te3"
    assert r.vectors == [[0.5], [0.25]]
    assert r.prompt_tokens == 4
    req = s.only()
    assert req["path"] == "/v1/embeddings"
    assert req["headers"]["x-uf-tags"] == '{"team":"x"}'
    body = json.loads(req["body"])
    assert body["input"] == ["a", "b"] and body["dimensions"] == 1


def test_a_single_string_input(serve):
    s = serve(Script.json(200, OPENAI_EMB))
    ultrafast.Client(ultrafast.gateway(s.url, KEY)).embed("te3", "a")
    assert json.loads(s.only()["body"])["input"] == ["a"]


def test_embed_to_a_provider_sends_no_tags(serve):
    s = serve(Script.json(200, OPENAI_EMB))
    ultrafast.Client(ultrafast.openai(KEY, base_url=s.url + "/v1")).embed("te3", ["a"], tags={"t": "v"})
    assert "x-uf-tags" not in s.only()["headers"]
