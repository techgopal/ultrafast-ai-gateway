// Builds the debug gateway once, unless UF_E2E_BINARY names one.
import { spawnSync } from "node:child_process";
import { BINARY_VARIABLE, REPOSITORY } from "./gateway.js";

export default function setup(): void {
  const given = process.env[BINARY_VARIABLE];
  if (given !== undefined && given !== "") return;
  const built = spawnSync("cargo", ["build", "-p", "ultrafast-gateway"], {
    cwd: REPOSITORY,
    stdio: "inherit",
  });
  if (built.status !== 0) throw new Error("cargo build -p ultrafast-gateway failed");
}
