import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test } from "vitest";
import { errors, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { gate, startGateway } from "@/test/gateway";
import { networkFailure, ok, override, refuse } from "@/test/handlers";
import {
  aCallFindsTheSessionEnded,
  cached,
  counted,
  expectNoSecret,
  expectOneH1,
  expectOneMain,
  forgetToasts,
  held,
  href,
  settle,
  toasts,
} from "@/test/pages";
import { renderWithApp, type AppRenderResult } from "@/test/render";

afterEach(forgetToasts);

const SECRET = "client-secret-0000-of-the-test";

function readOidc(view: unknown = fixtures.oidc.fresh) {
  return counted("get", "/api/settings/oidc", () =>
    ok("get", "/api/settings/oidc", 200, view as typeof fixtures.oidc.fresh),
  );
}

async function page(options: { width?: number } = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route: "/settings", ...options });
}

async function section(): Promise<HTMLElement> {
  return screen.findByRole("region", { name: "Single sign-on (OIDC)" });
}

function paste(field: HTMLElement, text: string): Promise<void> {
  return userEvent.click(field).then(() => userEvent.paste(text));
}

async function save(): Promise<void> {
  await userEvent.click(
    within(await section()).getByRole("button", { name: "Save single sign-on" }),
  );
}

describe("Settings: single sign-on", () => {
  test("a fresh gateway: everything off, the secret not set, the address to register", async () => {
    readOidc();
    await page();
    const one = within(await section());
    expect(await one.findByLabelText("Label on the sign-in button")).toHaveValue("SSO");
    expect(one.getByRole("switch", { name: "Single sign-on" })).not.toBeChecked();
    expect(one.getByLabelText("Issuer")).toHaveValue("");
    expect(one.getByLabelText("Client ID")).toHaveValue("");
    const secret = one.getByLabelText("Client secret");
    expect(secret).toHaveValue("");
    expect(secret).toHaveAttribute("type", "password");
    expect(one.getByText(/^Not set\./)).toBeInTheDocument();
    expect(one.getByLabelText("Groups claim")).toHaveValue("groups");
    expect(one.getByLabelText("Admin group")).toHaveValue("");
    expect(one.getByRole("checkbox", { name: "Link users by email" })).toBeChecked();
    expect(
      one.getByRole("checkbox", { name: "Create users on first sign-in" }),
    ).not.toBeChecked();
    expect(one.getByText(fixtures.oidc.fresh.redirect_uri)).toBeInTheDocument();
    expectOneMain();
    expectOneH1("Settings");
  });

  test("a configured gateway shows what is saved, and says the secret is set without showing it", async () => {
    readOidc(fixtures.oidc.configured);
    const app = await page();
    const one = within(await section());
    expect(await one.findByLabelText("Issuer")).toHaveValue("https://idp.example.test");
    expect(one.getByRole("switch", { name: "Single sign-on" })).toBeChecked();
    expect(one.getByLabelText("Label on the sign-in button")).toHaveValue("Test IdP");
    expect(one.getByLabelText("Client ID")).toHaveValue("gateway-client");
    expect(one.getByLabelText("Extra scopes")).toHaveValue("offline_access");
    expect(one.getByLabelText("Admin group")).toHaveValue("gateway-admins");
    expect(one.getByLabelText("Allowed email domains")).toHaveValue("example.test");
    // Write only: the field is empty, and replacing is what it offers.
    const secret = one.getByLabelText("Replace client secret");
    expect(secret).toHaveValue("");
    expect(one.getByText(/^Set\./)).toBeInTheDocument();
    expect(cached(app.queryClient)).not.toContain("client_secret\":\"");
  });

  test("a secret that cannot be read is a warning above the form", async () => {
    readOidc({ ...fixtures.oidc.configured, enabled: false, client_secret_unreadable: true });
    await page();
    const one = within(await section());
    expect(await one.findByRole("alert")).toHaveTextContent(
      "The stored client secret cannot be read, perhaps because the master key changed. Single sign-on stays off until you enter the client secret again and save.",
    );
  });

  test("without a public URL the address is explained, nothing to copy, and it cannot be turned on", async () => {
    readOidc({ ...fixtures.oidc.fresh, redirect_uri: null, public_url_set: false });
    await page();
    const one = within(await section());
    expect(
      await one.findByText(/Start the gateway with UF_PUBLIC_URL/),
    ).toBeInTheDocument();
    expect(one.queryByRole("button", { name: "Copy address" })).toBeNull();
    expect(one.getByRole("switch", { name: "Single sign-on" })).toBeDisabled();
  });

  test("the copy button copies the address and says so", async () => {
    readOidc();
    const written: string[] = [];
    const before = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: (text: string) => Promise.resolve(void written.push(text)) },
    });
    try {
      await page();
      const one = within(await section());
      await userEvent.click(await one.findByRole("button", { name: "Copy address" }));
      await waitFor(() => {
        expect(written).toEqual([fixtures.oidc.fresh.redirect_uri]);
      });
      expect(toasts()).toEqual(["Address copied."]);
    } finally {
      if (before === undefined) Reflect.deleteProperty(navigator, "clipboard");
      else Object.defineProperty(navigator, "clipboard", before);
    }
  });

  test("a clipboard that refuses is said, not hidden", async () => {
    readOidc();
    const before = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: () => Promise.reject(new Error("no")) },
    });
    try {
      await page();
      const one = within(await section());
      await userEvent.click(await one.findByRole("button", { name: "Copy address" }));
      await waitFor(() => {
        expect(toasts()).toEqual(["Could not copy. Select the address and copy it."]);
      });
    } finally {
      if (before === undefined) Reflect.deleteProperty(navigator, "clipboard");
      else Object.defineProperty(navigator, "clipboard", before);
    }
  });

  test("the guidance lists the issuer of Google, Entra, Okta and Keycloak as text", async () => {
    readOidc();
    await page();
    const one = within(await section());
    const list = await one.findByRole("list", { name: "Provider examples" });
    const items = within(list).getAllByRole("listitem").map((item) => item.textContent);
    expect(items).toEqual([
      "Google: https://accounts.google.com",
      "Microsoft Entra ID: https://login.microsoftonline.com/<tenant id>/v2.0",
      "Okta: https://<your org>.okta.com",
      "Keycloak: https://<host>/realms/<realm>",
    ]);
    expect(within(list).queryAllByRole("link")).toEqual([]);
  });

  test("saving sends every setting, no secret when none was typed, then reads again", async () => {
    const read = readOidc(fixtures.oidc.configured);
    const put = counted("put", "/api/settings/oidc", () =>
      ok("put", "/api/settings/oidc", 200, fixtures.oidc.configured),
    );
    await page();
    const one = within(await section());
    await paste(await one.findByLabelText("Label on the sign-in button"), "2");
    await save();
    await waitFor(() => {
      expect(toasts()).toEqual(["Single sign-on settings saved."]);
    });
    expect(put.bodies).toEqual([
      {
        enabled: true,
        label: "Test IdP2",
        issuer: "https://idp.example.test",
        client_id: "gateway-client",
        scopes: "offline_access",
        groups_claim: "groups",
        admin_group: "gateway-admins",
        link_by_email: true,
        auto_create: true,
        allowed_domains: ["example.test"],
      },
    ]);
    await waitFor(() => {
      expect(read.calls).toBe(2);
    });
  });

  test("a typed secret is sent once, then the field is empty and the secret is nowhere", async () => {
    readOidc(fixtures.oidc.fresh);
    const put = counted("put", "/api/settings/oidc", () =>
      ok("put", "/api/settings/oidc", 200, fixtures.oidc.configured),
    );
    const app = await page();
    const one = within(await section());
    await paste(await one.findByLabelText("Issuer"), "https://idp.example.test");
    await paste(one.getByLabelText("Client ID"), "gateway-client");
    await paste(one.getByLabelText("Client secret"), SECRET);
    await userEvent.click(one.getByRole("checkbox", { name: "Create users on first sign-in" }));
    await paste(one.getByLabelText("Allowed email domains"), "Example.test, corp.test foo.org");
    await save();
    await waitFor(() => {
      expect(toasts()).toEqual(["Single sign-on settings saved."]);
    });
    expect(put.bodies).toMatchObject([
      {
        issuer: "https://idp.example.test",
        client_id: "gateway-client",
        client_secret: SECRET,
        auto_create: true,
        allowed_domains: ["Example.test", "corp.test", "foo.org"],
      },
    ]);
    await waitFor(() => {
      expect(one.getByLabelText(/client secret/i)).toHaveValue("");
    });
    expectNoSecret(app, SECRET);
  });

  test("a refusal shows on its field and keeps what was typed, secret included", async () => {
    readOidc();
    override("put", "/api/settings/oidc", () =>
      refuse(
        validationFailed({
          issuer: "issuer must be an https URL",
          client_secret: "a client secret is needed to turn on single sign-on",
        }),
      ),
    );
    await page();
    const one = within(await section());
    await paste(await one.findByLabelText("Issuer"), "http://idp.example.test");
    await paste(one.getByLabelText("Client secret"), SECRET);
    await save();
    expect(await one.findByText("issuer must be an https URL")).toBeInTheDocument();
    expect(one.getByText("a client secret is needed to turn on single sign-on")).toBeInTheDocument();
    expect(one.getByLabelText("Client secret")).toHaveValue(SECRET);
    expect(one.getByLabelText("Issuer")).toHaveAttribute("aria-invalid", "true");
    expect(toasts()).toEqual([]);
  });

  test("the enable refusal for a missing public URL is on the switch", async () => {
    readOidc(fixtures.oidc.fresh);
    override("put", "/api/settings/oidc", () =>
      refuse(validationFailed({ enabled: "needs the gateway to be started with UF_PUBLIC_URL" })),
    );
    await page();
    const one = within(await section());
    await userEvent.click(await one.findByRole("switch", { name: "Single sign-on" }));
    await save();
    expect(
      await one.findByText("needs the gateway to be started with UF_PUBLIC_URL"),
    ).toBeInTheDocument();
  });

  test("the save runs once however often it is sent, and says Saving", async () => {
    readOidc(fixtures.oidc.configured);
    const request = held("put", "/api/settings/oidc", errors.internal_error);
    await page();
    const one = within(await section());
    await one.findByLabelText("Issuer");
    const form = one.getByRole("form", { name: "Single sign-on settings" });
    act(() => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(await one.findByRole("button", { name: "Saving" })).toBeDisabled();
    request.answer();
    expect(await one.findByText(errors.internal_error.body.error.message)).toBeInTheDocument();
    expect(request.calls).toBe(1);
  });

  test("an answer for a session that is over says nothing", async () => {
    startGateway({ signedIn: true });
    readOidc(fixtures.oidc.configured);
    const door = gate();
    override("put", "/api/settings/oidc", async () => {
      await door.opened;
      return ok("put", "/api/settings/oidc", 200, fixtures.oidc.configured);
    });
    const app = await page();
    const one = within(await section());
    await one.findByLabelText("Issuer");
    await save();
    await aCallFindsTheSessionEnded("/api/users");
    door.open();
    await settle();
    expect(toasts()).toEqual([]);
    expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/settings")}`);
  });

  test("a test of the configuration sends the issuer as typed and shows what the provider answered", async () => {
    readOidc(fixtures.oidc.configured);
    const test = counted("post", "/api/settings/oidc/test", () =>
      ok("post", "/api/settings/oidc/test", 200, {
        ok: true,
        issuer: "https://idp.example.test",
        jwks_keys: 2,
      }),
    );
    await page();
    const one = within(await section());
    await userEvent.click(await one.findByRole("button", { name: "Test configuration" }));
    const result = await one.findByRole("status");
    expect(result).toHaveTextContent("The provider answers: https://idp.example.test, 2 signing keys.");
    expect(test.bodies).toEqual([{ issuer: "https://idp.example.test" }]);
    // It saved nothing and asked nothing else.
    expect(toasts()).toEqual([]);
  });

  test("a test that fails says what is wrong, in the words of the gateway", async () => {
    readOidc(fixtures.oidc.configured);
    override("post", "/api/settings/oidc/test", () =>
      ok("post", "/api/settings/oidc/test", 200, {
        ok: false,
        error: "the provider could not be reached",
      }),
    );
    await page();
    const one = within(await section());
    await userEvent.click(await one.findByRole("button", { name: "Test configuration" }));
    expect(await one.findByRole("status")).toHaveTextContent(
      "Not working: the provider could not be reached",
    );
  });

  test("a test with no issuer typed is refused on the issuer field", async () => {
    readOidc(fixtures.oidc.fresh);
    const test = counted("post", "/api/settings/oidc/test", () =>
      refuse(validationFailed({ issuer: "issuer is required" })),
    );
    await page();
    const one = within(await section());
    await userEvent.click(await one.findByRole("button", { name: "Test configuration" }));
    expect(await one.findByText("issuer is required")).toBeInTheDocument();
    expect(test.bodies).toEqual([{}]);
  });

  test("the result of a test goes when the issuer is edited", async () => {
    readOidc(fixtures.oidc.configured);
    await page();
    const one = within(await section());
    await userEvent.click(await one.findByRole("button", { name: "Test configuration" }));
    await one.findByRole("status");
    await paste(one.getByLabelText("Issuer"), "x");
    expect(one.queryByRole("status")).toBeNull();
  });

  test("a gateway that cannot be reached shows a retry in the section, and the rest of the page stays", async () => {
    override("get", "/api/settings/oidc", networkFailure);
    await page();
    const one = within(await section());
    expect(await one.findByRole("button", { name: "Retry" })).toBeInTheDocument();
    expect(screen.getByLabelText("Keep request logs for (days)")).toBeInTheDocument();
  });

  test("at width 390 the controls are high enough to touch and nothing sticks out", async () => {
    readOidc(fixtures.oidc.configured);
    await page({ width: 390 });
    const one = within(await section());
    await one.findByLabelText("Issuer");
    for (const name of ["Save single sign-on", "Test configuration", "Copy address"]) {
      expect(one.getByRole("button", { name }).className.split(/\s+/)).toContain("min-h-11");
    }
    for (const field of one.getAllByRole("textbox")) {
      expect(field.className.split(/\s+/)).toContain("min-h-11");
    }
  });

  test("the switch is the only control called Single sign-on", async () => {
    readOidc(fixtures.oidc.configured);
    await page();
    const one = within(await section());
    await one.findByLabelText("Issuer");
    // Exactly one: the form has a name of its own.
    expect(one.getByLabelText("Single sign-on")).toBe(one.getByRole("switch", { name: "Single sign-on" }));
    expect(one.getByRole("form", { name: "Single sign-on settings" })).toBeInTheDocument();
  });

  test("the address and its Copy button are one group named by the label", async () => {
    readOidc(fixtures.oidc.configured);
    await page();
    const one = within(await section());
    const group = await one.findByRole("group", {
      name: "Address to register at the provider (redirect URI)",
    });
    expect(within(group).getByText(fixtures.oidc.configured.redirect_uri)).toBeInTheDocument();
    expect(within(group).getByRole("button", { name: "Copy address" })).toBeInTheDocument();
  });

  test("every field has a name, and the password manager is told not to fill the secret", async () => {
    readOidc(fixtures.oidc.configured);
    await page();
    const one = within(await section());
    await one.findByLabelText("Issuer");
    const secret = one.getByLabelText("Replace client secret");
    expect(secret).toHaveAttribute("autocomplete", "new-password");
    for (const field of one.getAllByRole("textbox")) {
      expect(field).toHaveAccessibleName();
    }
  });
});
