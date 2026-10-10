// The launcher of the browser tests (gateway.ts) keeps its promises: the
// gateway gets only the environment written out for it, and nothing of it is
// left when it stops, or when the test process is killed: no gateway, no
// watchdog, no data directory. Linux only, as CI: it reads /proc.
import { expect, test } from "@playwright/test";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { BINARY_VARIABLE, binaryPath, databaseUrl, startGateway } from "./gateway";

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
    // On PostgreSQL (UF_E2E_DATABASE_URL) the gateway also gets its database and master key.
    expect(environmentNames(pid)).toEqual(
      databaseUrl() === null
        ? ["NO_COLOR", "RUST_LOG", "UF_DATA_DIR", "UF_HOST", "UF_INSECURE_COOKIES", "UF_PORT"]
        : [
            "NO_COLOR",
            "RUST_LOG",
            "UF_DATABASE_URL",
            "UF_DATA_DIR",
            "UF_HOST",
            "UF_INSECURE_COOKIES",
            "UF_MASTER_KEY",
            "UF_PORT",
          ],
    );
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

test("publicUrl sets UF_PUBLIC_URL to the gateway's own address, or to the text given, and is otherwise left out", async () => {
  const own = await startGateway({ publicUrl: true });
  const given = await startGateway({ publicUrl: "https://gateway.example.test" });
  const none = await startGateway({ publicUrl: false });
  try {
    expect(variable(own.pid, "UF_PUBLIC_URL")).toBe(own.origin);
    expect(variable(given.pid, "UF_PUBLIC_URL")).toBe("https://gateway.example.test");
    expect(environmentNames(none.pid)).not.toContain("UF_PUBLIC_URL");
  } finally {
    await Promise.all([own.stop(), given.stop(), none.stop()]);
  }
});

/**
 * A test process of its own, which starts a gateway, says which, and waits to
 * be killed. `group`: in a process group of its own, as the job of a terminal
 * or of CI is, which one signal can kill whole.
 */
async function aTestProcess(group: boolean) {
  const launcher = new URL("./gateway.ts", import.meta.url).href;
  const script = [
    `import { startGateway } from ${JSON.stringify(launcher)};`,
    "const gateway = await startGateway();",
    "console.log(JSON.stringify({ pid: gateway.pid, dataDir: gateway.dataDir }));",
    "setInterval(() => {}, 60_000);",
  ].join("\n");
  const child = spawn(process.execPath, ["--input-type=module", "-e", script], {
    stdio: ["ignore", "pipe", "inherit"],
    detached: group,
  });
  let started: { pid: number; dataDir: string } | undefined;
  const lines = createInterface({ input: child.stdout });
  for await (const line of lines) {
    started = JSON.parse(line) as { pid: number; dataDir: string };
    break;
  }
  return { child, started };
}

/** Nothing of the gateway is left: not it, not its directory, not its watchdog. */
async function expectNothingLeft(pid: number, dataDir: string): Promise<void> {
  await expect.poll(() => running(pid), { message: "the gateway stops" }).toBe(false);
  await expect.poll(() => existsSync(dataDir), { message: "the directory goes" }).toBe(false);
  await expect.poll(() => processesWith(dataDir), { message: "the watchdog ends" }).toEqual([]);
}

test("when the test process is killed, its gateway stops and its data directory goes", async () => {
  const { child, started } = await aTestProcess(false);
  try {
    expect(started).toBeDefined();
    if (started === undefined) return;
    const { pid, dataDir } = started;
    expect(running(pid)).toBe(true);
    expect(existsSync(dataDir)).toBe(true);
    expect(processesWith(dataDir)).toHaveLength(1);

    child.kill("SIGKILL");
    await expectNothingLeft(pid, dataDir);
  } finally {
    child.kill("SIGKILL");
    cleanUp(started?.pid, started?.dataDir);
  }
});

test("when the whole process group of the test is killed, the watchdog still cleans up", async () => {
  const { child, started } = await aTestProcess(true);
  // The group is the test process's own: its id is the process's.
  const group = child.pid;
  try {
    expect(started).toBeDefined();
    expect(group).toBeGreaterThan(1);
    if (started === undefined || group === undefined || group <= 1) return;
    const { pid, dataDir } = started;
    expect(running(pid)).toBe(true);
    expect(processesWith(dataDir)).toHaveLength(1);

    // As a cancelled CI job or `kill -9 -<group>` does: the test process and
    // the gateway it started die at once, and nothing of theirs runs after.
    process.kill(-group, "SIGKILL");
    await expectNothingLeft(pid, dataDir);
  } finally {
    if (group !== undefined && group > 1) {
      try {
        process.kill(-group, "SIGKILL");
      } catch {
        // The group is gone already.
      }
    }
    cleanUp(started?.pid, started?.dataDir);
  }
});

test("a binary that is the deployed gateway's by a link is not started", () => {
  const deployed = join(homedir(), ".local", "share", "ultrafast-gateway");
  test.skip(!existsSync(deployed), "No gateway is deployed on this machine.");
  // A link that leads into the directory of the deployed gateway. Nothing is
  // read from there: the path is only resolved.
  const dir = mkdtempSync(join(tmpdir(), "uf-link-"));
  const link = join(dir, "ultrafast");
  symlinkSync(deployed, link);
  const before = process.env[BINARY_VARIABLE];
  process.env[BINARY_VARIABLE] = link;
  try {
    expect(() => binaryPath()).toThrow(/deployed gateway/);
  } finally {
    if (before === undefined) Reflect.deleteProperty(process.env, BINARY_VARIABLE);
    else process.env[BINARY_VARIABLE] = before;
    rmSync(dir, { recursive: true, force: true });
  }
});
