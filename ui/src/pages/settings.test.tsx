import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test } from "vitest";
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
import { renderWithApp, type AppRenderResult } from "@/test/render";

afterEach(forgetToasts);

function page(options: { user?: fixtures.Me; width?: number } = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/settings", ...options });
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
      ok("patch", "/api/settings", 200, { log_retention_days: 90 }),
    );
    const read = counted("get", "/api/settings", () =>
      ok("get", "/api/settings", 200, fixtures.settings),
    );
    await page();
    const field = await days();
    await userEvent.clear(field);
    await paste(field, "90");
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => {
      expect(toasts()).toEqual(["Settings saved."]);
    });
    expect(patch.bodies).toEqual([{ log_retention_days: 90 }]);
    await waitFor(() => {
      expect(read.calls).toBe(2);
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
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
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
      await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
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
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(await screen.findByText("must be from 1 to 3650")).toBeInTheDocument();
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(field).toHaveValue("30");
    expect(toasts()).toEqual([]);
  });

  test("one request runs at a time, and the button says so", async () => {
    const request = held("patch", "/api/settings", errors.internal_error);
    await page();
    await days();
    const form = screen.getByRole("form", { name: "Settings" });
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
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
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
    expect(screen.getByRole("button", { name: "Save settings" }).className.split(/\s+/)).toContain("min-h-11");
  });
});
