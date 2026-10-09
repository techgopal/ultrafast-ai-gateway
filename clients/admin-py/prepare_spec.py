"""Writes the copy of openapi/admin.json that the generator reads.

openapi-python-client 0.29.1 drops the `Matcher` variant `{"keywords": {...}}`
("duplicate models with name MatcherType0Keywords") and the generated package
then fails to import. Hoisting the inline `keywords` object into a named schema
avoids it. The document in the repository is not changed; the proper fix is a
named schema in the gateway's utoipa types.
"""

import copy
import json
import sys

source, target = sys.argv[1], sys.argv[2]
with open(source, encoding="utf-8") as handle:
    document = json.load(handle)

schemas = document["components"]["schemas"]
for variant in schemas["Matcher"]["oneOf"]:
    inline = variant.get("properties", {}).get("keywords")
    if inline is not None and "$ref" not in inline:
        schemas["KeywordsMatcher"] = copy.deepcopy(inline)
        variant["properties"]["keywords"] = {"$ref": "#/components/schemas/KeywordsMatcher"}

with open(target, "w", encoding="utf-8") as handle:
    json.dump(document, handle, indent=2, sort_keys=True)
    handle.write("\n")
