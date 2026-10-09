// Starts the real gateway (a debug build) for the SDK tests: on a port the
// system gives out (never 3900), with a data directory of its own and an
// environment written out in full, so that no `UF_*` variable of the shell
// reaches it. It is stopped by its process id, and its directory removed, when
// the test ends and when the test process exits.
//
// The binary is `UF_E2E_BINARY` when set. Otherwise `startGateway` builds the
// debug binary with `cargo build -p ultrafast-gateway` (once per test file; a
// no-op when it is up to date) and starts `<target_directory>/debug/ultrafast`,
// where `target_directory` is what `cargo metadata` says, so `CARGO_TARGET_DIR`
// is honoured. Tests that need no gateway never build.
import { execFile, spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { accessSync, constants, realpathSync, rmSync } from "node:fs";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { homedir } from "node:os";
import { join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const IN_USE_PORT = 3900;
export const BINARY_VARIABLE = "UF_E2E_BINARY";
export const REPOSITORY = fileURLToPath(new URL("../../..", import.meta.url));
const DEPLOYED = join(homedir(), ".local", "share", "ultrafast-gateway");
const READY_WITHIN_MS = 30_000;
const STOP_WITHIN_MS = 10_000;

export interface Account {
  email: string;
  password: string;
}

export interface Gateway {
  origin: string;
  port: number;
  pid: number;
  admin: Account;
  output: () => string;
  stop: () => Promise<void>;
}

function followed(path: string): string {
  try {
    return realpathSync(path);
  } catch {
    return path;
  }
}

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

/** Where scratch data lives: under ~/.cache, not the small /tmp. */
const SCRATCH = join(homedir(), ".cache", "uf-admin-sdk-tests");

function run(command: string, args: string[], cwd: string): Promise<string> {
  return new Promise((done, fail) => {
    execFile(command, args, { cwd, maxBuffer: 64 * 1024 * 1024 }, (error, stdout, stderr) => {
      if (error) fail(new Error(`${command} ${args.join(" ")} failed:\n${stderr}`));
      else done(stdout);
    });
  });
}

let built: Promise<string> | null = null;

/** The binary to start: `UF_E2E_BINARY`, or the debug build, made now. */
export function binaryPath(): Promise<string> {
  built ??= (async () => {
    const given = process.env[BINARY_VARIABLE];
    let path: string;
    if (given === undefined || given === "") {
      await run("cargo", ["build", "-p", "ultrafast-gateway"], REPOSITORY);
      const metadata = JSON.parse(
        await run("cargo", ["metadata", "--no-deps", "--format-version", "1"], REPOSITORY),
      ) as { target_directory: string };
      path = join(metadata.target_directory, "debug", "ultrafast");
    } else {
      path = resolve(given);
    }
    if (isDeployed(path)) {
      throw new Error(`${BINARY_VARIABLE} names a binary of the deployed gateway; it is not started.`);
    }
    try {
      accessSync(path, constants.X_OK);
    } catch {
      throw new Error(`No gateway binary at ${path}. Build it: cargo build -p ultrafast-gateway`);
    }
    return path;
  })();
  return built;
}

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

const running = new Map<ChildProcess, string>();
function killAll(): void {
  for (const [child, dataDir] of running) {
    child.kill("SIGKILL");
    rmSync(dataDir, { recursive: true, force: true });
  }
  running.clear();
}
process.once("exit", killAll);
// A stopped test run takes its gateways with it, then goes on as it would have.
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.once(signal, () => {
    killAll();
    process.kill(process.pid, signal);
  });
}

function hasExited(child: ChildProcess): boolean {
  return child.pid === undefined || child.exitCode !== null || child.signalCode !== null;
}

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

async function healthy(origin: string): Promise<boolean> {
  try {
    const answer = await fetch(`${origin}/health`, { signal: AbortSignal.timeout(2_000) });
    return answer.ok;
  } catch {
    return false;
  }
}

/** Starts a gateway with a first admin. Retries when another process took the port. */
export async function startGateway(): Promise<Gateway> {
  const binary = await binaryPath();
  const admin: Account = {
    email: "admin@example.com",
    password: `pw-${randomBytes(12).toString("hex")}`,
  };
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    await mkdir(SCRATCH, { recursive: true });
    const dataDir = await mkdtemp(join(SCRATCH, "uf-e2e-"));
    const port = await freePort();
    const origin = `http://127.0.0.1:${port}`;
    const child = spawn(binary, ["serve"], {
      env: {
        UF_DATA_DIR: dataDir,
        UF_HOST: "127.0.0.1",
        UF_PORT: String(port),
        UF_INSECURE_COOKIES: "true",
        UF_ADMIN_EMAIL: admin.email,
        UF_ADMIN_PASSWORD: admin.password,
        RUST_LOG: "warn",
        NO_COLOR: "1",
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    running.set(child, dataDir);
    let output = "";
    const keep = (chunk: Buffer) => {
      output += chunk.toString("utf8");
    };
    child.stdout.on("data", keep);
    child.stderr.on("data", keep);
    const stop = async () => {
      await stopProcess(child);
      running.delete(child);
      await rm(dataDir, { recursive: true, force: true });
    };
    const until = Date.now() + READY_WITHIN_MS;
    let up = false;
    while (!hasExited(child) && Date.now() < until) {
      if (await healthy(origin)) {
        up = true;
        break;
      }
      await new Promise((done) => setTimeout(done, 100));
    }
    if (up && child.pid !== undefined) {
      let stopped: Promise<void> | null = null;
      return {
        origin,
        port,
        pid: child.pid,
        admin,
        output: () => output,
        stop: () => (stopped ??= stop()),
      };
    }
    await stop();
    if (!output.includes("could not listen")) {
      throw new Error(`The gateway did not start. Its output:\n${output}`);
    }
  }
  throw new Error("The gateway did not start: no free port.");
}
