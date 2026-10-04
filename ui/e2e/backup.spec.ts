import { readFile } from "node:fs/promises";
import { expect, goTo, signInFromStart, test } from "./fixtures";
import { startMockProvider } from "./mock-provider";

test("the backup downloaded from Settings is a SQLite file with the tables and the data, and no master key", async ({
  page,
  admin,
  apiAs,
  gateway,
}) => {
  const mock = await startMockProvider(["e2e-model"]);
  try {
    const api = await apiAs(admin);
    await api.addProvider("alpha", mock.baseUrl, mock.apiKey);
    await api.activeUser("Mia");

    await signInFromStart(page, admin);
    await goTo(page, "Settings");
    const section = page.getByRole("region", { name: "Backup" });
    await expect(section).toContainText("useless without it");
    const downloading = page.waitForEvent("download");
    await section.getByRole("link", { name: "Download backup" }).click();
    const download = await downloading;
    expect(download.suggestedFilename()).toMatch(/^ultrafast-\d{8}-\d{6}\.db$/);
    const bytes = await readFile(await download.path());

    expect(bytes.subarray(0, 16).toString("latin1")).toBe("SQLite format 3\0");
    const text = bytes.toString("latin1");
    // The tables of the gateway are in it (the schema is stored as text),
    for (const table of [
      "providers",
      "models",
      "routes",
      "users",
      "virtual_keys",
      "request_logs",
      "audit_log",
      "settings",
      "sessions",
    ]) {
      expect(text, `table ${table}`).toContain(`CREATE TABLE ${table}`);
    }
    // and the data: the admin and the provider.
    expect(text).toContain(admin.email);
    expect(text).toContain("alpha");
    // The provider's credential is stored encrypted: not as the key.
    expect(text).not.toContain(mock.apiKey);
    // The master key is in the data directory of the gateway, not in a backup.
    const masterKey = (await readFile(`${gateway.dataDir}/master.key`, "utf8")).trim();
    expect(masterKey).toMatch(/^[0-9a-f]{64}$/);
    expect(text).not.toContain(masterKey);
    // And it was audited.
    const audit = (await api.get("/api/audit")) as { entries: { action: string }[] };
    expect(audit.entries.map((e) => e.action)).toContain("backup.download");
  } finally {
    await mock.close();
  }
});
