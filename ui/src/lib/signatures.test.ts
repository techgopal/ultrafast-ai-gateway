// @vitest-environment node
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import { describe, expect, test } from "vitest";
import { NODE_EXAMPLE, PYTHON_EXAMPLE } from "@/lib/signatures";

// crates/gateway/src/alerts/sign.rs, known_answer.
const SECRET = "whsec_test-secret";
const BODY = '{"a":1}';
const T = "1700000000";
const SIGNATURE = "f7c40776aa58d1eee88648e741f2488ecc8e2cc810cd3801942f335e82a1da57";
const HEADER = `t=${T},v1=${SIGNATURE}`;
const NOW_MS = 1_700_000_100_000;

function runNode(header: string, secret: string, now: number): number {
  let rejected = 0;
  const clock = { now: () => now };
  runInNewContext(NODE_EXAMPLE, {
    require: createRequire(import.meta.url),
    Date: clock,
    Buffer,
    header,
    secret,
    rawBody: BODY,
    reject: () => {
      rejected += 1;
    },
  });
  return rejected;
}

describe("the Node example", () => {
  test("accepts the known answer", () => {
    expect(runNode(HEADER, SECRET, NOW_MS)).toBe(0);
  });
  test("rejects another secret, another signature and an old time", () => {
    expect(runNode(HEADER, "whsec_other", NOW_MS)).toBe(1);
    expect(runNode(`t=${T},v1=${"0".repeat(64)}`, SECRET, NOW_MS)).toBe(1);
    expect(runNode(HEADER, SECRET, NOW_MS + 3_600_000)).toBe(1);
  });
});

const python = spawnSync("python3", ["--version"]).status === 0;

describe.skipIf(!python)("the Python example", () => {
  // The example runs in a namespace of its own that holds only what a
  // receiver has (`header`, `raw_body`, `secret`, `reject`): its imports are
  // its own. `time` is replaced in `sys.modules`, so an `import time` in the
  // example gets the fixed clock and a missing one is a NameError.
  function runPython(example: string, header: string, secret: string, now: number) {
    const program = [
      "import sys, types, time as real_time",
      "clock = types.ModuleType('time')",
      "clock.__dict__.update(real_time.__dict__)",
      `clock.time = lambda: ${String(now / 1000)}`,
      "sys.modules['time'] = clock",
      "rejected = []",
      "namespace = {",
      `    'header': ${JSON.stringify(header)},`,
      `    'secret': ${JSON.stringify(secret)},`,
      `    'raw_body': ${JSON.stringify(BODY)}.encode(),`,
      "    'reject': lambda: rejected.append(1),",
      "}",
      `exec(compile(${JSON.stringify(example)}, 'example', 'exec'), namespace)`,
      "print(len(rejected))",
    ].join("\n");
    return spawnSync("python3", ["-c", program], { encoding: "utf8" });
  }
  function rejections(header: string, secret: string, now: number): number {
    const run = runPython(PYTHON_EXAMPLE, header, secret, now);
    expect(run.stderr).toBe("");
    return Number(run.stdout.trim());
  }
  test("accepts the known answer, and rejects what is wrong", () => {
    expect(rejections(HEADER, SECRET, NOW_MS)).toBe(0);
    expect(rejections(HEADER, "whsec_other", NOW_MS)).toBe(1);
    expect(rejections(HEADER, SECRET, NOW_MS + 3_600_000)).toBe(1);
  });
  test("an example that forgets an import fails here", () => {
    const imports = ["hmac", "hashlib", "time"];
    for (const name of imports) {
      const kept = imports.filter((other) => other !== name).join(", ");
      const broken = PYTHON_EXAMPLE.replace(/^import .*$/m, `import ${kept}`);
      expect(broken).not.toBe(PYTHON_EXAMPLE);
      const run = runPython(broken, HEADER, SECRET, NOW_MS);
      expect(run.stderr, name).toContain("NameError");
    }
  });
});
