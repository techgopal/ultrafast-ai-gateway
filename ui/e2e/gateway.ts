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
// it also when the test process dies without a chance to do so.
import { spawn, type ChildProcess } from "node:child_process";
import { accessSync, constants, rmSync } from "node:fs";
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

export interface Account {
  email: string;
  password: string;
}

export interface GatewayOptions {
  /** The first admin, from `UF_ADMIN_EMAIL` and `UF_ADMIN_PASSWORD`. None: the gateway needs setup. */
  admin?: Account | undefined;
}

export interface Gateway {
  /** For example `http://127.0.0.1:41234`. */
  origin: string;
  port: number;
  /** What the gateway wrote to its output. It logs warnings and errors only. */
  output: () => string;
  /** Stops the gateway, waits for it, and removes its data directory. */
  stop: () => Promise<void>;
}

/** The binary that is started. */
export function binaryPath(): string {
  const given = process.env[BINARY_VARIABLE];
  const path = given === undefined || given === "" ? OWN_BINARY : resolve(given);
  if (path === DEPLOYED || path.startsWith(DEPLOYED + sep)) {
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
function environment(dataDir: string, port: number, admin: Account | undefined) {
  const env: Record<string, string> = {
    UF_DATA_DIR: dataDir,
    UF_HOST: "127.0.0.1",
    UF_PORT: String(port),
    UF_INSECURE_COOKIES: "true",
    // Warnings and errors only: the gateway logs the admin's email at `info`.
    RUST_LOG: "warn",
    NO_COLOR: "1",
  };
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
// does when the test process ends in any way. It stops only that process:
// on Linux it checks the binary of the process id first.
const WATCHDOG = `
const { readFileSync } = require("node:fs");
const [pid, binary] = [Number(process.argv[1]), process.argv[2]];
function ours() {
  try { return readFileSync("/proc/" + pid + "/cmdline", "utf8").split("\\0")[0] === binary; }
  catch { return process.platform !== "linux"; }
}
function stop() {
  if (ours()) { try { process.kill(pid, "SIGKILL"); } catch {} }
  process.exit(0);
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

function hasExited(child: ChildProcess): boolean {
  return child.exitCode !== null || child.signalCode !== null;
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

/** Starts a gateway on a free port with a new data directory. */
export async function startGateway(options: GatewayOptions = {}): Promise<Gateway> {
  const binary = binaryPath();
  const dataDir = await mkdtemp(join(tmpdir(), "uf-e2e-"));
  let lastOutput = "";
  try {
    for (let attempt = 1; attempt <= START_ATTEMPTS; attempt += 1) {
      const port = await freePort();
      const origin = `http://127.0.0.1:${port}`;
      const child = spawn(binary, ["serve"], {
        env: environment(dataDir, port, options.admin),
        stdio: ["ignore", "pipe", "pipe"],
      });
      running.set(child, dataDir);
      let output = "";
      const keep = (chunk: Buffer) => {
        output += chunk.toString("utf8");
      };
      child.stdout.on("data", keep);
      child.stderr.on("data", keep);
      const pid = child.pid;
      const watchdog =
        pid === undefined
          ? null
          : spawn(process.execPath, ["-e", WATCHDOG, String(pid), binary], {
              stdio: ["pipe", "ignore", "ignore"],
            });
      const stop = async () => {
        await stopProcess(child);
        running.delete(child);
        watchdog?.stdin.end();
        if (watchdog !== null) await exited(watchdog);
      };

      if (await ready(child, origin)) {
        // The answer is of this gateway: its data directory is new, so it
        // needs setup exactly when no admin was given.
        const setup = await answers(`${origin}/api/setup`);
        const body: unknown = setup?.ok === true ? await setup.json() : null;
        const needsSetup =
          typeof body === "object" && body !== null && "needs_setup" in body
            ? body.needs_setup
            : null;
        // The line is written before the gateway listens; its pipe may be read a little later.
        const insecure = await within(READY_WITHIN_MS, () => output.includes(INSECURE_COOKIES_LINE));
        if (needsSetup !== (options.admin === undefined) || !insecure) {
          await stop();
          throw new Error(
            `The gateway on ${origin} is not the one started here, or runs without --insecure-cookies.`,
          );
        }
        let stopped: Promise<void> | null = null;
        return {
          origin,
          port,
          output: () => output,
          stop: () => {
            stopped ??= stop().then(() => rm(dataDir, { recursive: true, force: true }));
            return stopped;
          },
        };
      }
      await stop();
      lastOutput = output;
      // Another process may have taken the port in between: try another one.
      if (!output.includes("could not listen")) break;
    }
  } catch (error) {
    await rm(dataDir, { recursive: true, force: true });
    throw error;
  }
  await rm(dataDir, { recursive: true, force: true });
  throw new Error(`The gateway did not start. Its output:\n${lastOutput}`);
}
