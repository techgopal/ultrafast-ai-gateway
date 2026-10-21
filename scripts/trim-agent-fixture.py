#!/usr/bin/env python3
"""Trim a request recorded by scripts/agent-recorder.py into a test fixture.

    trim-agent-fixture.py <recording.json> <fixture.json>

Keeps the shape (field names, tool names and schemas, block types, order) and
drops everything that could identify a person or a machine: every `text`,
`instructions` and tool-output string becomes a short placeholder, tool
descriptions are cut to their first line, `metadata.user_id` becomes "u", and
every UUID becomes a numbered placeholder. Fails if a path, e-mail address or
host name is still in the result.
"""
import json
import re
import sys

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
seen = {}


def uuid(m):
    seen.setdefault(m.group(0), len(seen) + 1)
    return "00000000-0000-0000-0000-%012d" % seen[m.group(0)]


def clean(v, key=None, parent_type=None):
    if isinstance(v, dict):
        return {k: clean(x, k, v.get("type")) for k, x in v.items()}
    if isinstance(v, list):
        return [clean(x, key, parent_type) for x in v]
    if isinstance(v, str):
        if key in ("text", "instructions", "output", "input") and parent_type != "custom":
            return "t"
        if key == "description":
            return v.strip().split("\n")[0][:80]
        if key == "format" or key == "definition":
            return v[:80]
        return UUID.sub(uuid, v)
    return v


def main():
    rec = json.load(open(sys.argv[1]))
    body = json.loads(rec["body"])
    out = clean(body)
    if "metadata" in out and "user_id" in out["metadata"]:
        out["metadata"]["user_id"] = "u"
    text = json.dumps(out, indent=2)
    for bad in (r"/home/", r"/Users/", r"[\w.+-]+@[\w-]+\.[\w.]+", r"gopal", r"\.cache", r"localhost", r"127\.0\.0\.1"):
        if re.search(bad, text):
            sys.exit(f"identifying text left in the fixture: {bad}")
    open(sys.argv[2], "w").write(text + "\n")


if __name__ == "__main__":
    main()
