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
  function runPython(header: string, secret: string, now: number): number {
    const program = [
      "import time",
      `time.time = lambda: ${String(now / 1000)}`,
      "rejected = 0",
      "def reject():",
      "    global rejected",
      "    rejected += 1",
      `header = ${JSON.stringify(header)}`,
      `secret = ${JSON.stringify(secret)}`,
      `raw_body = ${JSON.stringify(BODY)}.encode()`,
      PYTHON_EXAMPLE,
      "print(rejected)",
    ].join("\n");
    const run = spawnSync("python3", ["-c", program], { encoding: "utf8" });
    expect(run.stderr).toBe("");
    return Number(run.stdout.trim());
  }
  test("accepts the known answer, and rejects what is wrong", () => {
    expect(runPython(HEADER, SECRET, NOW_MS)).toBe(0);
    expect(runPython(HEADER, "whsec_other", NOW_MS)).toBe(1);
    expect(runPython(HEADER, SECRET, NOW_MS + 3_600_000)).toBe(1);
  });
});
