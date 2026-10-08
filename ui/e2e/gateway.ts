// Starts the real gateway binary for the browser tests. It builds nothing:
// the console and the binary are built before (`pnpm --dir ui build`, then
// `cargo build --release -p ultrafast-gateway`, which embeds the console).
//
// Each gateway gets a port that the system gives out and a data directory of
// its own, made here and removed when it stops. Its environment is written
// out here in full: nothing is inherited, so no `UF_*` variable of the shell
// (such as a master key, or the data directory of a gateway that is in use)
// reaches it. It is stopped, and waited for, when the test ends, when it
// failed to start, and when the test process exits; a watchdog process stops
// it and removes its data directory also when the test process dies without
// a chance to do so. `launcher.spec.ts` checks these.
import { spawn, type ChildProcess } from "node:child_process";
import { accessSync, constants, realpathSync, rmSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { homedir, tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

/** The port of the gateway that is in use on this machine. Never taken here. */
export const IN_USE_PORT = 3900;
/** The binary to start in place of `target/release/ultrafast` of this repository. */
export const BINARY_VARIABLE = "UF_E2E_BINARY";

/** The directory of the deployed gateway: no binary under it is started. */
const DEPLOYED = join(homedir(), ".local", "share", "ultrafast-gateway");
const OWN_BINARY = fileURLToPath(new URL("../../target/release/ultrafast", import.meta.url));

const READY_WITHIN_MS = 30_000;
const STOP_WITHIN_MS = 10_000;
const START_ATTEMPTS = 3;
/** The log line of `--insecure-cookies`, which the tests need on plain HTTP. */
const INSECURE_COOKIES_LINE = "session cookies are sent without Secure";
/** The log line with the one-time setup code, of a gateway that starts without users. */
const SETUP_CODE_LINE = /Setup code: ([0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}); open the console/;

export interface Account {
  email: string;
  password: string;
}

export interface GatewayOptions {
  /** The first admin, from `UF_ADMIN_EMAIL` and `UF_ADMIN_PASSWORD`. None: the gateway needs setup. */
  admin?: Account | undefined;
  /**
   * `UF_PUBLIC_URL`: the address people reach the gateway at, which single
   * sign-on needs. `true`: the gateway's own origin, which is known only once
   * its port is (the usual case). A text: that address. None: not set.
   */
  publicUrl?: boolean | string | undefined;
}

export interface Gateway {
  /** For example `http://127.0.0.1:41234`. */
  origin: string;
  port: number;
  /** The process of the gateway. */
  pid: number;
  /** Its data directory, which is removed when it stops. */
  dataDir: string;
  /**
   * What the gateway wrote to its output. It logs warnings and errors only,
   * and the setup code of a gateway that needs setup.
   */
  output: () => string;
  /** The setup code the gateway logged; `null` when it started with an admin. */
  setupCode: () => string | null;
  /** Stops the gateway, waits for it, and removes its data directory. */
  stop: () => Promise<void>;
}

/** The path with every link on it followed; the path as it is when it leads nowhere. */
function followed(path: string): string {
  try {
    return realpathSync(path);
  } catch {
    return path;
  }
}

/** Whether the path is in the directory of the deployed gateway, also by a link. */
function isDeployed(path: string): boolean {
  const real = followed(path);
  return [DEPLOYED, followed(DEPLOYED)].some(
    (deployed) =>
      path === deployed ||
      path.startsWith(deployed + sep) ||
      real === deployed ||
      real.startsWith(deployed + sep),
  );
}

/**
 * The binary that is started, as it is named: the watchdog knows the gateway
 * by that name in its command line.
 */
export function binaryPath(): string {
  const given = process.env[BINARY_VARIABLE];
  const path = given === undefined || given === "" ? OWN_BINARY : resolve(given);
  if (isDeployed(path)) {
    throw new Error(`${BINARY_VARIABLE} names a binary of the deployed gateway; it is not started.`);
  }
  try {
    accessSync(path, constants.X_OK);
  } catch {
    throw new Error(
      `No gateway binary at ${path}. Build it first: ` +
        "pnpm --dir ui build && cargo build --release -p ultrafast-gateway",
    );
  }
  return path;
}

/** A port that the system gives out for 127.0.0.1, never the one in use. */
export async function freePort(): Promise<number> {
  for (;;) {
    const port = await new Promise<number>((done, fail) => {
      const server = createServer();
      server.unref();
      server.on("error", fail);
      server.listen(0, "127.0.0.1", () => {
        const address = server.address();
        const given = typeof address === "object" && address !== null ? address.port : 0;
        server.close(() => {
          done(given);
        });
      });
    });
    if (port !== 0 && port !== IN_USE_PORT) return port;
  }
}

/** The whole environment of the gateway. */
function environment(
  dataDir: string,
  port: number,
  admin: Account | undefined,
  publicUrl: string | undefined,
) {
  const env: Record<string, string> = {
    UF_DATA_DIR: dataDir,
    UF_HOST: "127.0.0.1",
    UF_PORT: String(port),
    UF_INSECURE_COOKIES: "true",
    // Warnings and errors only: the gateway logs the admin's email at `info`.
    // The setup code has a target of its own: setup needs it.
    RUST_LOG: "warn,ultrafast::setup=info",
    NO_COLOR: "1",
  };
  if (publicUrl !== undefined) env.UF_PUBLIC_URL = publicUrl;
  if (admin !== undefined) {
    env.UF_ADMIN_EMAIL = admin.email;
    env.UF_ADMIN_PASSWORD = admin.password;
  }
  return env;
}

// Stops the gateway when the test process ends without the test doing it.
const running = new Map<ChildProcess, string>();
process.once("exit", () => {
  for (const [child, dataDir] of running) {
    child.kill("SIGKILL");
    rmSync(dataDir, { recursive: true, force: true });
  }
});

// A separate process that stops the gateway when its input closes, which it
// does when the test process ends in any way, also by SIGKILL, when the test
// process can do nothing itself. It runs in a process group of its own, so
// that it outlives a SIGKILL of the test's whole group (a cancelled CI job,
// `kill -9 -<group>`), which kills the gateway with the test process. It
// stops only that process: on Linux it checks the binary of the process id
// first. Then it removes the data directory, which is one this file made
// (`uf-e2e-*`).
const WATCHDOG = `
const { readFileSync, rmSync } = require("node:fs");
const { basename } = require("node:path");
const [pid, binary, dataDir] = [Number(process.argv[1]), process.argv[2], process.argv[3]];
function alive() {
  try { process.kill(pid, 0); return true; } catch { return false; }
}
function ours() {
  try { return readFileSync("/proc/" + pid + "/cmdline", "utf8").split("\\0")[0] === binary; }
  catch { return process.platform !== "linux" && alive(); }
}
let stopping = false;
function stop() {
  if (stopping) return;
  stopping = true;
  if (ours()) { try { process.kill(pid, "SIGKILL"); } catch {} }
  const until = Date.now() + 5000;
  (function removeWhenGone() {
    if (ours() && Date.now() < until) { setTimeout(removeWhenGone, 50); return; }
    if (basename(dataDir).startsWith("uf-e2e-")) rmSync(dataDir, { recursive: true, force: true });
    process.exit(0);
  })();
}
process.on("SIGINT", () => {});
process.on("SIGTERM", stop);
process.stdin.on("close", stop);
process.stdin.on("end", stop);
process.stdin.resume();
`;

function exited(child: ChildProcess): Promise<void> {
  if (hasExited(child)) return Promise.resolve();
  return new Promise((done) => {
    child.once("exit", () => {
      done();
    });
  });
}

async function stopProcess(child: ChildProcess): Promise<void> {
  if (hasExited(child)) return;
  child.kill("SIGTERM");
  const late = setTimeout(() => {
    child.kill("SIGKILL");
  }, STOP_WITHIN_MS);
  await exited(child);
  clearTimeout(late);
}

async function answers(url: string): Promise<Response | null> {
  try {
    return await fetch(url, { signal: AbortSignal.timeout(2_000) });
  } catch {
    return null;
  }
}

/** Whether `condition` holds within `ms`, asked every 50 ms. */
async function within(ms: number, condition: () => boolean): Promise<boolean> {
  const until = Date.now() + ms;
  while (!condition()) {
    if (Date.now() >= until) return false;
    await new Promise((done) => setTimeout(done, 50));
  }
  return true;
}

/** Whether the process has ended, or never started. */
function hasExited(child: ChildProcess): boolean {
  return child.pid === undefined || child.exitCode !== null || child.signalCode !== null;
}

/** Waits until the gateway answers `/health`, or has exited. True when it answers. */
async function ready(child: ChildProcess, origin: string): Promise<boolean> {
  const until = Date.now() + READY_WITHIN_MS;
  while (!hasExited(child) && Date.now() < until) {
    const health = await answers(`${origin}/health`);
    if (health?.ok === true && !hasExited(child)) return true;
    await new Promise((done) => setTimeout(done, 100));
  }
  return false;
}

/** What one try to start a gateway came to. */
type Try = { gateway: Gateway } | { output: string; portTaken: boolean };

/** One try: a new data directory and a free port. Anything that fails stops and removes both. */
async function startOnce(binary: string, options: GatewayOptions): Promise<Try> {
  const dataDir = await mkdtemp(join(tmpdir(), "uf-e2e-"));
  const port = await freePort();
  const origin = `http://127.0.0.1:${port}`;
  const child = spawn(binary, ["serve"], {
    env: environment(
      dataDir,
      port,
      options.admin,
      options.publicUrl === true ? origin : options.publicUrl || undefined,
    ),
    stdio: ["ignore", "pipe", "pipe"],
  });
  running.set(child, dataDir);
  let output = "";
  const keep = (chunk: Buffer) => {
    output += chunk.toString("utf8");
  };
  child.stdout.on("data", keep);
  child.stderr.on("data", keep);
  child.on("error", (error) => {
    output += `${error.message}\n`;
  });
  const { pid } = child;
  const watchdog =
    pid === undefined
      ? null
      : spawn(process.execPath, ["-e", WATCHDOG, String(pid), binary, dataDir], {
          stdio: ["pipe", "ignore", "ignore"],
          detached: true,
        });
  const stop = async () => {
    await stopProcess(child);
    running.delete(child);
    watchdog?.stdin.end();
    if (watchdog !== null) await exited(watchdog);
    await rm(dataDir, { recursive: true, force: true });
  };

  try {
    if (pid === undefined || !(await ready(child, origin))) {
      await stop();
      return { output, portTaken: output.includes("could not listen") };
    }
    // The answer is of this gateway: its data directory is new, so it
    // needs setup exactly when no admin was given.
    const setup = await answers(`${origin}/api/setup`);
    const body: unknown = setup?.ok === true ? await setup.json() : null;
    const needsSetup =
      typeof body === "object" && body !== null && "needs_setup" in body ? body.needs_setup : null;
    // The line is written before the gateway listens; its pipe may be read a little later.
    const insecure = await within(READY_WITHIN_MS, () => output.includes(INSECURE_COOKIES_LINE));
    // A gateway that needs setup logs its code before it listens.
    const coded =
      options.admin !== undefined ||
      (await within(READY_WITHIN_MS, () => SETUP_CODE_LINE.test(output)));
    if (needsSetup !== (options.admin === undefined) || !insecure || !coded) {
      throw new Error(
        `The gateway on ${origin} is not the one started here, or runs without --insecure-cookies.`,
      );
    }
  } catch (error) {
    await stop();
    throw error;
  }
  let stopped: Promise<void> | null = null;
  return {
    gateway: {
      origin,
      port,
      pid,
      dataDir,
      output: () => output,
      setupCode: () => SETUP_CODE_LINE.exec(output)?.[1] ?? null,
      stop: () => {
        stopped ??= stop();
        return stopped;
      },
    },
  };
}

/** Starts a gateway on a free port with a new data directory. */
export async function startGateway(options: GatewayOptions = {}): Promise<Gateway> {
  const binary = binaryPath();
  let lastOutput = "";
  for (let attempt = 1; attempt <= START_ATTEMPTS; attempt += 1) {
    const tried = await startOnce(binary, options);
    if ("gateway" in tried) return tried.gateway;
    lastOutput = tried.output;
    // Another process may have taken the port in between: try another one.
    if (!tried.portTaken) break;
  }
  throw new Error(`The gateway did not start. Its output:\n${lastOutput}`);
}
