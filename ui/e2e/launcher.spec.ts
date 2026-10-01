// The launcher of the browser tests (gateway.ts) keeps its promises: the
// gateway gets only the environment written out for it, and nothing of it is
// left when it stops, or when the test process is killed: no gateway, no
// watchdog, no data directory. Linux only, as CI: it reads /proc.
import { expect, test } from "@playwright/test";
import { spawn } from "node:child_process";
import { existsSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { createInterface } from "node:readline";
import { binaryPath, startGateway } from "./gateway";

/** Whether the process runs: it exists and is not a zombie. */
function running(pid: number): boolean {
  try {
    const stat = readFileSync(`/proc/${String(pid)}/stat`, "utf8");
    return stat.slice(stat.lastIndexOf(")") + 2, stat.lastIndexOf(")") + 3) !== "Z";
  } catch {
    return false;
  }
}

/** The processes whose command line holds the text: the watchdog has the data directory in its own. */
function processesWith(text: string): number[] {
  return readdirSync("/proc")
    .filter((name) => /^\d+$/.test(name))
    .flatMap((name) => {
      try {
        return readFileSync(`/proc/${name}/cmdline`, "utf8").includes(text) ? [Number(name)] : [];
      } catch {
        return [];
      }
    });
}

/** The names of the variables of the process's environment; their values are not read out. */
function environmentNames(pid: number): string[] {
  return readFileSync(`/proc/${String(pid)}/environ`, "utf8")
    .split("\0")
    .filter((entry) => entry !== "")
    .map((entry) => entry.slice(0, entry.indexOf("=")))
    .sort();
}

function variable(pid: number, name: string): string | undefined {
  const entry = readFileSync(`/proc/${String(pid)}/environ`, "utf8")
    .split("\0")
    .find((one) => one.startsWith(`${name}=`));
  return entry?.slice(name.length + 1);
}

/** Stops what a failed test may have left: only our binary, and only a directory of the launcher. */
function cleanUp(pid: number | undefined, dataDir: string | undefined): void {
  if (pid !== undefined && running(pid)) {
    const command = readFileSync(`/proc/${String(pid)}/cmdline`, "utf8").split("\0")[0];
    if (command === binaryPath()) process.kill(pid, "SIGKILL");
  }
  if (dataDir?.includes("uf-e2e-") === true) rmSync(dataDir, { recursive: true, force: true });
}

test.beforeEach(() => {
  test.skip(test.info().project.name !== "desktop", "The launcher is the same in every project.");
  test.skip(process.platform !== "linux", "These checks read /proc.");
});

test("the gateway gets only its own environment, and leaves nothing when it stops", async () => {
  // Variables of the shell that must not reach it.
  const before = { dir: process.env.UF_DATA_DIR, key: process.env.UF_MASTER_KEY };
  process.env.UF_DATA_DIR = "/nonexistent/data-of-another-gateway";
  process.env.UF_MASTER_KEY = "0".repeat(64);
  let gateway: Awaited<ReturnType<typeof startGateway>> | undefined;
  try {
    gateway = await startGateway();
  } finally {
    for (const [name, value] of [
      ["UF_DATA_DIR", before.dir],
      ["UF_MASTER_KEY", before.key],
    ] as const) {
      if (value === undefined) Reflect.deleteProperty(process.env, name);
      else process.env[name] = value;
    }
  }
  const { pid, dataDir, port } = gateway;
  try {
    expect(port).not.toBe(3900);
    expect(environmentNames(pid)).toEqual([
      "NO_COLOR",
      "RUST_LOG",
      "UF_DATA_DIR",
      "UF_HOST",
      "UF_INSECURE_COOKIES",
      "UF_PORT",
    ]);
    expect(variable(pid, "UF_DATA_DIR")).toBe(dataDir);
    expect(variable(pid, "UF_HOST")).toBe("127.0.0.1");
    expect(variable(pid, "UF_PORT")).toBe(String(port));
    expect(dataDir).toContain("uf-e2e-");
    expect(existsSync(dataDir)).toBe(true);
    expect(running(pid)).toBe(true);
    expect(processesWith(dataDir)).toHaveLength(1);

    await gateway.stop();
    expect(running(pid)).toBe(false);
    expect(existsSync(dataDir)).toBe(false);
    expect(processesWith(dataDir)).toEqual([]);
  } finally {
    cleanUp(pid, dataDir);
  }
});

test("when the test process is killed, its gateway stops and its data directory goes", async () => {
  // A test process of its own, which starts a gateway, says which, and waits to be killed.
  const launcher = new URL("./gateway.ts", import.meta.url).href;
  const script = [
    `import { startGateway } from ${JSON.stringify(launcher)};`,
    "const gateway = await startGateway();",
    "console.log(JSON.stringify({ pid: gateway.pid, dataDir: gateway.dataDir }));",
    "setInterval(() => {}, 60_000);",
  ].join("\n");
  const testProcess = spawn(process.execPath, ["--input-type=module", "-e", script], {
    stdio: ["ignore", "pipe", "inherit"],
  });
  let started: { pid: number; dataDir: string } | undefined;
  try {
    const lines = createInterface({ input: testProcess.stdout });
    for await (const line of lines) {
      started = JSON.parse(line) as { pid: number; dataDir: string };
      break;
    }
    expect(started).toBeDefined();
    if (started === undefined) return;
    const { pid, dataDir } = started;
    expect(running(pid)).toBe(true);
    expect(existsSync(dataDir)).toBe(true);
    expect(processesWith(dataDir)).toHaveLength(1);

    testProcess.kill("SIGKILL");
    await expect.poll(() => running(pid), { message: "the gateway stops" }).toBe(false);
    await expect.poll(() => existsSync(dataDir), { message: "the directory goes" }).toBe(false);
    await expect.poll(() => processesWith(dataDir), { message: "the watchdog ends" }).toEqual([]);
  } finally {
    testProcess.kill("SIGKILL");
    cleanUp(started?.pid, started?.dataDir);
  }
});
