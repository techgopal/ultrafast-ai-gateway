import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test, vi } from "vitest";
import { errors, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, ok, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  counted,
  expectNotAvailable,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  held,
  href,
  settle,
  SESSION_ENDED,
  toasts,
} from "@/test/pages";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

afterEach(forgetToasts);

function page(options: { user?: fixtures.Me; width?: number } = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/settings", ...options });
}

/**
 * Records what the console tells the browser to download: the address of
 * every anchor it clicks. The page's own link is not counted, it is only
 * what a click on it is turned into.
 */
function downloads(): string[] {
  const started: string[] = [];
  vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
    started.push(this.getAttribute("href") ?? "");
  });
  return started;
}

const days = () => screen.findByLabelText("Keep request logs for (days)");

function paste(field: HTMLElement, text: string): Promise<void> {
  return userEvent.click(field).then(() => userEvent.paste(text));
}

describe("the settings page", () => {
  test("an admin sees the retention, with one main and one h1", async () => {
    await page();
    expect(await days()).toHaveValue("30");
    expectOneMain();
    expectOneH1("Settings");
    expect(screen.getByText("From 1 to 3650 days. Older request logs are deleted.")).toBeInTheDocument();
  });

  test("saving sends the number and says so; the page reads the settings again", async () => {
    const patch = counted("patch", "/api/settings", () =>
      ok("patch", "/api/settings", 200, { ...fixtures.settings, log_retention_days: 90 }),
    );
    const read = counted("get", "/api/settings", () =>
      ok("get", "/api/settings", 200, fixtures.settings),
    );
    await page();
    const field = await days();
    await userEvent.clear(field);
    await paste(field, "90");
    await userEvent.click(screen.getByRole("button", { name: "Save retention" }));
    await waitFor(() => {
      expect(toasts()).toEqual(["Settings saved."]);
    });
    expect(patch.bodies).toEqual([{ log_retention_days: 90 }]);
    await waitFor(() => {
      expect(read.calls).toBe(2);
    });
  });

  test("the number is typed, key by key, and sent with Enter", async () => {
    const patch = counted("patch", "/api/settings", () =>
      ok("patch", "/api/settings", 200, { ...fixtures.settings, log_retention_days: 365 }),
    );
    await page();
    const field = await days();
    await userEvent.clear(field);
    await userEvent.type(field, "365{Enter}");
    await waitFor(() => {
      expect(patch.bodies).toEqual([{ log_retention_days: 365 }]);
    });
  });

  test.each(["", "0", "3651", "1.5", "abc", "-1"])("%j is refused on the field, nothing is sent", async (text) => {
    const patch = counted("patch", "/api/settings", () =>
      ok("patch", "/api/settings", 200, fixtures.settings),
    );
    await page();
    const field = await days();
    await userEvent.clear(field);
    if (text !== "") await paste(field, text);
    await userEvent.click(screen.getByRole("button", { name: "Save retention" }));
    expect(await screen.findByText("Enter a whole number from 1 to 3650.")).toBeInTheDocument();
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(patch.calls).toBe(0);
  });

  test("the ends are accepted", async () => {
    const patch = counted("patch", "/api/settings", () =>
      ok("patch", "/api/settings", 200, fixtures.settings),
    );
    await page();
    const field = await days();
    for (const text of ["1", "3650"]) {
      await userEvent.clear(field);
      await paste(field, text);
      await userEvent.click(screen.getByRole("button", { name: "Save retention" }));
      await waitFor(() => {
        expect(patch.bodies).toHaveLength(text === "1" ? 1 : 2);
      });
    }
    expect(patch.bodies).toEqual([{ log_retention_days: 1 }, { log_retention_days: 3650 }]);
  });

  test("a field error of the gateway is shown on the field", async () => {
    override("patch", "/api/settings", () =>
      refuse(validationFailed({ log_retention_days: "must be from 1 to 3650" })),
    );
    await page();
    const field = await days();
    await userEvent.click(screen.getByRole("button", { name: "Save retention" }));
    expect(await screen.findByText("must be from 1 to 3650")).toBeInTheDocument();
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(field).toHaveValue("30");
    expect(toasts()).toEqual([]);
  });

  test("one request runs at a time, and the button says so", async () => {
    const request = held("patch", "/api/settings", errors.internal_error);
    await page();
    await days();
    const form = screen.getByRole("form", { name: "Retention" });
    act(() => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(await screen.findByRole("button", { name: "Saving" })).toBeDisabled();
    request.answer();
    expect(await screen.findByText(errors.internal_error.body.error.message)).toBeInTheDocument();
    expect(request.calls).toBe(1);
  });

  test("an answer for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("patch", "/api/settings", async () => {
      await door.opened;
      return ok("patch", "/api/settings", 200, fixtures.settings);
    });
    const app = await page();
    await days();
    await userEvent.click(screen.getByRole("button", { name: "Save retention" }));
    await aCallFindsTheSessionEnded("/api/users");
    door.open();
    await settle();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/settings")}`);
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
  });

  test("a member does not get the page, and the settings are not asked for", async () => {
    const read = counted("get", "/api/settings", () => ok("get", "/api/settings", 200, fixtures.settings));
    await page({ user: fixtures.me.lena });
    await expectNotAvailable();
    expect(read.calls).toBe(0);
  });

  test("a failed read shows the error with Retry, under the one h1", async () => {
    override("get", "/api/settings", () => refuse(errors.internal_error));
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent(errors.internal_error.body.error.message);
    expectOneH1("Settings");
    override("get", "/api/settings", () => ok("get", "/api/settings", 200, fixtures.settings));
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await days()).toHaveValue("30");
  });

  test("a gateway that cannot be reached is said", async () => {
    override("get", "/api/settings", networkFailure);
    await page();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the gateway.");
  });

  test("while it loads: the heading and a skeleton", async () => {
    const door = gate();
    override("get", "/api/settings", async () => {
      await door.opened;
      return ok("get", "/api/settings", 200, fixtures.settings);
    });
    await page();
    expect(await screen.findByRole("status", { name: "Loading the settings" })).toBeInTheDocument();
    expectOneMain();
    expectOneH1("Settings");
    act(() => {
      door.open();
    });
    await days();
  });

  test("at width 390 the field and the button are touchable", async () => {
    await page({ width: 390 });
    const field = await days();
    expect(field.className.split(/\s+/)).toContain("min-h-11");
    expect(screen.getByRole("button", { name: "Save retention" }).className.split(/\s+/)).toContain("min-h-11");
  });
});

// ---------------------------------------------------------------- sections

describe("the sections of the page", () => {
  test("the page has its sections, each with a heading, and a way to the audit log", async () => {
    await page();
    await days();
    expectOneMain();
    expectOneH1("Settings");
    const headings = within(screen.getByRole("main"))
      .getAllByRole("heading", { level: 2 })
      .map((h) => h.textContent);
    expect(headings).toEqual(["Retention", "Sign-in", "Backup", "Configuration"]);
    const nav = screen.getByRole("navigation", { name: "Settings sections" });
    expect(within(nav).getByRole("link", { name: "General" })).toHaveAttribute("aria-current", "page");
    expect(within(nav).getByRole("link", { name: "Audit log" })).toHaveAttribute(
      "href",
      "/settings#audit",
    );
  });

  test("the audit log is another view of the same page, with its own heading", async () => {
    await renderWithApp(null, { route: "/settings#audit" });
    expect(await screen.findByRole("heading", { level: 2, name: "Audit log" })).toBeInTheDocument();
    expectOneH1("Settings");
    expectOneMain();
    expect(screen.queryByRole("heading", { name: "Retention" })).toBeNull();
    const nav = screen.getByRole("navigation", { name: "Settings sections" });
    expect(within(nav).getByRole("link", { name: "Audit log" })).toHaveAttribute("aria-current", "page");
  });

  test("the old address of the audit log leads there", async () => {
    const app = await renderWithApp(null, { route: "/audit" });
    expect(await screen.findByRole("heading", { level: 2, name: "Audit log" })).toBeInTheDocument();
    expect(href(app)).toBe("/settings#audit");
  });
});

const hours = () => screen.findByLabelText("Session lifetime (hours)");

describe("sign-in", () => {
  test("the lifetime is editable, the proxies and the limits are shown and explained", async () => {
    await page();
    expect(await hours()).toHaveValue("12");
    expect(screen.getByText("From 1 to 720 hours. Applies to sign-ins from now on; sessions that exist keep theirs.")).toBeInTheDocument();
    const section = screen.getByRole("region", { name: "Sign-in" });
    expect(within(section).getByText("10.0.0.0/8")).toBeInTheDocument();
    expect(
      within(section).getByText(/Set with the --trusted-proxy flag when the gateway starts/),
    ).toBeInTheDocument();
    expect(
      within(section).getByText(
        "A sign-in is refused after 5 failed attempts for one email, or 20 from one address, within 15 minutes.",
      ),
    ).toBeInTheDocument();
    expect(within(section).getByText(/Built in; they cannot be changed here/)).toBeInTheDocument();
    // Only the lifetime is a field.
    expect(within(section).getAllByRole("textbox")).toHaveLength(1);
  });

  test("with no trusted proxy it says so", async () => {
    override("get", "/api/settings", () =>
      ok("get", "/api/settings", 200, { ...fixtures.settings, trusted_proxies: [] }),
    );
    await page();
    await hours();
    expect(screen.getByText("None. Forwarding headers are ignored.")).toBeInTheDocument();
  });

  test("saving sends the lifetime alone and says so; the settings are read again", async () => {
    const patch = counted("patch", "/api/settings", () =>
      ok("patch", "/api/settings", 200, { ...fixtures.settings, session_hours: 24 }),
    );
    const read = counted("get", "/api/settings", () =>
      ok("get", "/api/settings", 200, fixtures.settings),
    );
    await page();
    const field = await hours();
    await userEvent.clear(field);
    await paste(field, "24");
    await userEvent.click(screen.getByRole("button", { name: "Save sign-in settings" }));
    await waitFor(() => {
      expect(toasts()).toEqual(["Settings saved."]);
    });
    expect(patch.bodies).toEqual([{ session_hours: 24 }]);
    await waitFor(() => {
      expect(read.calls).toBe(2);
    });
  });

  test("the lifetime is typed, key by key, and sent with Enter", async () => {
    const patch = counted("patch", "/api/settings", () =>
      ok("patch", "/api/settings", 200, fixtures.settings),
    );
    await page();
    const field = await hours();
    await userEvent.clear(field);
    await userEvent.type(field, "48{Enter}");
    await waitFor(() => {
      expect(patch.bodies).toEqual([{ session_hours: 48 }]);
    });
  });

  test.each(["", "0", "721", "1.5", "abc", "-1"])("%j is refused on the field, nothing is sent", async (text) => {
    const patch = counted("patch", "/api/settings", () =>
      ok("patch", "/api/settings", 200, fixtures.settings),
    );
    await page();
    const field = await hours();
    await userEvent.clear(field);
    if (text !== "") await paste(field, text);
    await userEvent.click(screen.getByRole("button", { name: "Save sign-in settings" }));
    expect(await screen.findByText("Enter a whole number from 1 to 720.")).toBeInTheDocument();
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(patch.calls).toBe(0);
  });

  test("the ends are accepted", async () => {
    const patch = counted("patch", "/api/settings", () =>
      ok("patch", "/api/settings", 200, fixtures.settings),
    );
    await page();
    const field = await hours();
    for (const text of ["1", "720"]) {
      await userEvent.clear(field);
      await paste(field, text);
      await userEvent.click(screen.getByRole("button", { name: "Save sign-in settings" }));
      await waitFor(() => {
        expect(patch.bodies).toHaveLength(text === "1" ? 1 : 2);
      });
    }
    expect(patch.bodies).toEqual([{ session_hours: 1 }, { session_hours: 720 }]);
  });

  test("a field error of the gateway is shown on the field", async () => {
    override("patch", "/api/settings", () =>
      refuse(validationFailed({ session_hours: "must be from 1 to 720" })),
    );
    await page();
    const field = await hours();
    await userEvent.click(screen.getByRole("button", { name: "Save sign-in settings" }));
    expect(await screen.findByText("must be from 1 to 720")).toBeInTheDocument();
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(toasts()).toEqual([]);
  });

  test("one request runs at a time, and the button says so", async () => {
    const request = held("patch", "/api/settings", errors.internal_error);
    await page();
    await hours();
    const form = screen.getByRole("form", { name: "Sign-in" });
    act(() => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(await screen.findByRole("button", { name: "Saving" })).toBeDisabled();
    request.answer();
    expect(await screen.findByText(errors.internal_error.body.error.message)).toBeInTheDocument();
    expect(request.calls).toBe(1);
  });

  test("an answer for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    const door = gate();
    override("patch", "/api/settings", async () => {
      await door.opened;
      return ok("patch", "/api/settings", 200, fixtures.settings);
    });
    const app = await page();
    await hours();
    await userEvent.click(screen.getByRole("button", { name: "Save sign-in settings" }));
    await aCallFindsTheSessionEnded("/api/users");
    door.open();
    await settle();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/settings")}`);
  });

  test("the two forms do not send each other's field", async () => {
    const patch = counted("patch", "/api/settings", () =>
      ok("patch", "/api/settings", 200, fixtures.settings),
    );
    await page();
    await userEvent.click(await screen.findByRole("button", { name: "Save retention" }));
    await userEvent.click(screen.getByRole("button", { name: "Save sign-in settings" }));
    await waitFor(() => {
      expect(patch.bodies).toHaveLength(2);
    });
    expect(patch.bodies).toEqual([{ log_retention_days: 30 }, { session_hours: 12 }]);
  });
});

describe("backup", () => {
  test("the download is a link to the gateway, and the master key is explained", async () => {
    await page();
    await days();
    const section = screen.getByRole("region", { name: "Backup" });
    const link = within(section).getByRole("link", { name: "Download backup" });
    expect(link).toHaveAttribute("href", "/api/backup");
    expect(link).toHaveAttribute("download");
    expect(
      within(section).getByText(/The backup does not hold the master key, and is useless without it/),
    ).toBeInTheDocument();
    expect(within(section).getByText(/Restore is a command-line procedure/)).toBeInTheDocument();
    expect(link.className.split(/\s+/)).toContain("min-h-11");
  });

  test("the session is checked first, then the download starts", async () => {
    const started = downloads();
    const check = counted("get", "/api/settings", () => ok("get", "/api/settings", 200, fixtures.settings));
    await page();
    await days();
    const before = check.calls;
    await userEvent.click(screen.getByRole("link", { name: "Download backup" }));
    await waitFor(() => {
      expect(started).toEqual(["/api/backup"]);
    });
    expect(check.calls).toBe(before + 1);
  });

  test("a refusal is shown in place and nothing is downloaded", async () => {
    const started = downloads();
    await page();
    await days();
    override("get", "/api/settings", () => refuse(errors.internal_error));
    await userEvent.click(screen.getByRole("link", { name: "Download backup" }));
    const section = screen.getByRole("region", { name: "Backup" });
    expect(await within(section).findByRole("alert")).toHaveTextContent("Something went wrong.");
    expect(started).toEqual([]);
    // Trying again, when it works, clears the message.
    override("get", "/api/settings", () => ok("get", "/api/settings", 200, fixtures.settings));
    await userEvent.click(within(section).getByRole("link", { name: "Download backup" }));
    await waitFor(() => {
      expect(started).toEqual(["/api/backup"]);
    });
    expect(within(section).queryByRole("alert")).toBeNull();
  });

  test("an ended session downloads nothing and ends the session as every call does", async () => {
    const started = downloads();
    const app = await page();
    await days();
    override("get", "/api/settings", unauthenticated);
    override("get", "/api/auth/me", unauthenticated);
    await userEvent.click(screen.getByRole("link", { name: "Download backup" }));
    await waitFor(() => {
      expect(href(app)).toBe("/sign-in?next=%2Fsettings");
    });
    expect(started).toEqual([]);
  });
});

// ---------------------------------------------------------- configuration

function fileOf(text: string, name = "ultrafast-config.json"): File {
  return new File([text], name, { type: "application/json" });
}

async function choosing(text: string): Promise<void> {
  const input = await screen.findByLabelText("Configuration file");
  await userEvent.upload(input, fileOf(text));
}

/** Records the imports, and answers each with `answer`. */
function imports(
  answer: (dryRun: boolean) => Response = () =>
    ok("post", "/api/config/import", 200, fixtures.importReports.changes),
) {
  const seen: { dryRun: boolean; body: unknown }[] = [];
  override("post", "/api/config/import", async ({ request }) => {
    const dryRun = new URL(request.url).searchParams.get("dry_run") !== "false";
    seen.push({ dryRun, body: await request.json() });
    return answer(dryRun);
  });
  return seen;
}

const FILE = JSON.stringify(fixtures.configFile);

describe("configuration: export", () => {
  test("the download is a link to the gateway", async () => {
    await page();
    await days();
    const section = screen.getByRole("region", { name: "Configuration" });
    const link = within(section).getByRole("link", { name: "Download configuration" });
    expect(link).toHaveAttribute("href", "/api/config/export");
    expect(link).toHaveAttribute("download");
    expect(
      within(section).getByText(/No credential, key, token, password or log is in the file/),
    ).toBeInTheDocument();
  });

  test("the session is checked first, a refusal is shown in place, then the download starts", async () => {
    const started = downloads();
    await page();
    await days();
    const section = screen.getByRole("region", { name: "Configuration" });
    override("get", "/api/settings", () => refuse(errors.internal_error));
    await userEvent.click(within(section).getByRole("link", { name: "Download configuration" }));
    expect(await within(section).findByRole("alert")).toHaveTextContent("Something went wrong.");
    expect(started).toEqual([]);
    override("get", "/api/settings", () => ok("get", "/api/settings", 200, fixtures.settings));
    await userEvent.click(within(section).getByRole("link", { name: "Download configuration" }));
    await waitFor(() => {
      expect(started).toEqual(["/api/config/export"]);
    });
    expect(within(section).queryByRole("alert")).toBeNull();
  });
});

describe("configuration: import", () => {
  test("a file is checked with the gateway first and the report is shown", async () => {
    const seen = imports();
    await page();
    await choosing(FILE);
    const section = screen.getByRole("region", { name: "Configuration" });
    const table = await within(section).findByRole("table", { name: "What the import would do" });
    expect(seen).toEqual([{ dryRun: true, body: fixtures.configFile }]);
    const rows = [...table.querySelectorAll("tbody tr")].map((row) =>
      [...row.querySelectorAll("td")].map((cell) => cell.textContent),
    );
    expect(rows).toEqual([
      ["Create", "provider", "extra", ""],
      ["Create", "team", "Design", ""],
      ["Update", "model", "openai/gpt-4o", "enabled, grants"],
    ]);
    expect(within(section).getByText("5 unchanged.")).toBeInTheDocument();
    // The warning says where and what.
    expect(
      within(section).getByText(/providers\[0\]: provider 'extra' is created with no credential/),
    ).toBeInTheDocument();
    expect(within(section).getByText(/Nothing has been written yet/)).toBeInTheDocument();
    expect(within(section).getByRole("button", { name: "Apply import" })).toBeEnabled();
  });

  test("Apply asks first, then applies the same file, says so and clears the report", async () => {
    const seen = imports();
    const settingsRead = counted("get", "/api/settings", () =>
      ok("get", "/api/settings", 200, fixtures.settings),
    );
    await page();
    await choosing(FILE);
    await userEvent.click(await screen.findByRole("button", { name: "Apply import" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Apply this import?" });
    expect(within(dialog).getByText(/creates 2 things and updates 1/)).toBeInTheDocument();
    expect(within(dialog).getByText(/Nothing is deleted/)).toBeInTheDocument();
    // Asking again before the answer sent nothing.
    expect(seen).toHaveLength(1);
    await userEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    await waitFor(() => {
      expect(toasts()).toEqual(["Configuration imported."]);
    });
    expect(seen.map((s) => s.dryRun)).toEqual([true, false]);
    expect(seen[1]?.body).toEqual(fixtures.configFile);
    expect(screen.queryByRole("table", { name: "What the import would do" })).toBeNull();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    // What changed is read again.
    await waitFor(() => {
      expect(settingsRead.calls).toBe(2);
    });
  });

  test("cancelling the question applies nothing", async () => {
    const seen = imports();
    await page();
    await choosing(FILE);
    await userEvent.click(await screen.findByRole("button", { name: "Apply import" }));
    const dialog = await screen.findByRole("alertdialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).toBeNull();
    });
    expect(seen).toHaveLength(1);
    expect(screen.getByRole("table", { name: "What the import would do" })).toBeInTheDocument();
  });

  test("a file with errors shows them, says nothing was written and cannot be applied", async () => {
    imports(() => Response.json(fixtures.importReports.invalid, { status: 422 }));
    await page();
    await choosing(FILE);
    const section = screen.getByRole("region", { name: "Configuration" });
    const table = await within(section).findByRole("table", { name: "Problems in the file" });
    const rows = [...table.querySelectorAll("tbody tr")].map((row) =>
      [...row.querySelectorAll("td")].map((cell) => cell.textContent),
    );
    expect(rows).toEqual([
      ["models[0].grants.teams[1]", "team 'No such team' does not exist"],
      ["settings.session_hours", "must be from 1 to 720"],
    ]);
    expect(within(section).getByText(/The file has errors. Nothing was written/)).toBeInTheDocument();
    expect(within(section).queryByRole("button", { name: "Apply import" })).toBeNull();
    expect(within(section).queryByRole("table", { name: "What the import would do" })).toBeNull();
  });

  test("a file that says what the gateway has says there is nothing to do", async () => {
    imports(() => ok("post", "/api/config/import", 200, fixtures.importReports.nothing));
    await page();
    await choosing(FILE);
    const section = screen.getByRole("region", { name: "Configuration" });
    expect(await within(section).findByText(/Nothing to change: all 7 things are already so/)).toBeInTheDocument();
    expect(within(section).queryByRole("button", { name: "Apply import" })).toBeNull();
  });

  test("a file that is not JSON is refused by the console, and nothing is sent", async () => {
    const seen = imports();
    await page();
    await choosing("{ not json");
    expect(await screen.findByText("This file is not JSON.")).toBeInTheDocument();
    expect(seen).toEqual([]);
    expect(screen.queryByRole("button", { name: "Apply import" })).toBeNull();
  });

  test("a file larger than the gateway reads is refused before it is sent", async () => {
    const seen = imports();
    await page();
    const big = new File(["x"], "big.json", { type: "application/json" });
    Object.defineProperty(big, "size", { value: 8 * 1024 * 1024 + 1 });
    await userEvent.upload(await screen.findByLabelText("Configuration file"), big);
    expect(await screen.findByText("This file is larger than 8 MiB.")).toBeInTheDocument();
    expect(seen).toEqual([]);
  });

  test("a refusal of the gateway is said in place", async () => {
    imports(() => refuse(errors.internal_error));
    await page();
    await choosing(FILE);
    expect(await screen.findByText(errors.internal_error.body.error.message)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Apply import" })).toBeNull();
  });

  test("a gateway that cannot be reached is said", async () => {
    imports(() => networkFailure());
    await page();
    await choosing(FILE);
    expect(await screen.findByText("Could not reach the gateway.")).toBeInTheDocument();
  });

  test("an apply that finds errors shows them in the dialog's place and says nothing was written", async () => {
    imports((dryRun) =>
      dryRun
        ? ok("post", "/api/config/import", 200, fixtures.importReports.changes)
        : Response.json(fixtures.importReports.invalid, { status: 422 }),
    );
    await page();
    await choosing(FILE);
    await userEvent.click(await screen.findByRole("button", { name: "Apply import" }));
    await userEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Apply" }));
    expect(await screen.findByRole("table", { name: "Problems in the file" })).toBeInTheDocument();
    expect(toasts()).toEqual([]);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("button", { name: "Apply import" })).toBeNull();
  });

  test("another file replaces the report", async () => {
    imports((dryRun) =>
      ok("post", "/api/config/import", 200, dryRun ? fixtures.importReports.changes : fixtures.importReports.nothing),
    );
    await page();
    await choosing(FILE);
    await screen.findByRole("table", { name: "What the import would do" });
    imports(() => ok("post", "/api/config/import", 200, fixtures.importReports.nothing));
    await choosing(FILE.replace("Platform", "Other"));
    expect(await screen.findByText(/Nothing to change/)).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: "What the import would do" })).toBeNull();
  });

  test("the session ending while the file is checked says nothing and signs out", async () => {
    startGateway({ signedIn: true });
    imports(() => unauthenticated());
    const app = await page();
    await choosing(FILE);
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/settings")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(toasts()).toEqual([]);
  });

  test("the session ending while the import is applied says nothing and signs out", async () => {
    startGateway({ signedIn: true });
    imports((dryRun) =>
      dryRun ? ok("post", "/api/config/import", 200, fixtures.importReports.changes) : unauthenticated(),
    );
    const app = await page();
    await choosing(FILE);
    await userEvent.click(await screen.findByRole("button", { name: "Apply import" }));
    await userEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Apply" }));
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/settings")}`);
    });
    expect(toasts()).toEqual([]);
  });

  test("at width 390 the controls are touchable and the page does not scroll sideways", async () => {
    imports();
    await page({ width: 390 });
    await choosing(FILE);
    const apply = await screen.findByRole("button", { name: "Apply import" });
    expect(apply.className.split(/\s+/)).toContain("min-h-11");
    expect((await screen.findByLabelText("Configuration file")).className).toMatch(/min-h-11/);
    expectOneMain();
    expectOneH1("Settings");
  });
});
