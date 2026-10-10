import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import { PASSWORD, startGateway } from "@/test/gateway";
import { errors } from "@/test/errors";
import { networkFailure, ok, override, refuse } from "@/test/handlers";
import { counted, expectOneH1, expectOneMain } from "@/test/pages";
import { renderWithApp } from "@/test/render";

function withSso(): void {
  override("get", "/api/auth/methods", () =>
    ok("get", "/api/auth/methods", 200, fixtures.signInMethods.withOidc),
  );
}

async function signInPage(route = "/sign-in", width = 1280) {
  startGateway();
  return renderWithApp(null, { route, width });
}

const SSO_MESSAGES = [
  ["state", "Sign-in took too long or was interrupted. Try again."],
  ["expired", "Sign-in took too long or was interrupted. Try again."],
  ["idp", "Your identity provider refused the sign-in."],
  ["token", "Single sign-on is not set up correctly. Ask an admin."],
  ["config", "Single sign-on is not set up correctly. Ask an admin."],
  ["not_allowed", "Your account is not allowed to sign in here. Ask an admin to invite you."],
  ["disabled", "Your account is disabled."],
  ["rate_limited", "Too many sign-in attempts. Try again in a few minutes."],
] as const;

describe("the sign-in page with single sign-on", () => {
  test("without a provider there is only the password form", async () => {
    await signInPage();
    expect(await screen.findByRole("form", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Sign in with/ })).toBeNull();
    expectOneMain();
    expectOneH1("Sign in");
  });

  test("with a provider a link named after it stands above the password form", async () => {
    withSso();
    await signInPage();
    const link = await screen.findByRole("link", { name: "Sign in with Test IdP" });
    expect(link).toHaveAttribute("href", "/api/auth/oidc/start?return_to=%2F");
    const form = screen.getByRole("form", { name: "Sign in" });
    expect(link.compareDocumentPosition(form) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expectOneMain();
    expectOneH1("Sign in");
  });

  test("the link is a full-page navigation: no click handler stops it", async () => {
    withSso();
    await signInPage();
    const link = await screen.findByRole("link", { name: "Sign in with Test IdP" });
    expect(link.tagName).toBe("A");
    expect(link).not.toHaveAttribute("target");
    // Seen at the document, after the handlers of the page; then stopped here,
    // so that the test does not navigate.
    let prevented: boolean | null = null;
    const watch = (event: Event) => {
      prevented = event.defaultPrevented;
      event.preventDefault();
    };
    document.addEventListener("click", watch);
    try {
      await userEvent.click(link);
    } finally {
      document.removeEventListener("click", watch);
    }
    expect(prevented).toBe(false);
  });

  test("it returns to the page the visitor came for", async () => {
    withSso();
    await signInPage(`/sign-in?next=${encodeURIComponent("/keys?tag=a b")}`);
    const link = await screen.findByRole("link", { name: "Sign in with Test IdP" });
    expect(link).toHaveAttribute(
      "href",
      `/api/auth/oidc/start?return_to=${encodeURIComponent("/keys?tag=a b")}`,
    );
  });

  test("an address that leaves the console is not carried", async () => {
    withSso();
    await signInPage(`/sign-in?next=${encodeURIComponent("//evil.example/x")}`);
    const link = await screen.findByRole("link", { name: "Sign in with Test IdP" });
    expect(link).toHaveAttribute("href", "/api/auth/oidc/start?return_to=%2F");
  });

  test("a label with markup or an address in it is text only", async () => {
    override("get", "/api/auth/methods", () =>
      ok("get", "/api/auth/methods", 200, { password: true, oidc: { label: "<b>Acme</b> SSO" } }),
    );
    await signInPage();
    const link = await screen.findByRole("link", { name: "Sign in with <b>Acme</b> SSO" });
    expect(link.querySelector("b")).toBeNull();
  });

  test("when the methods cannot be read the password form is still there, and nothing is said", async () => {
    override("get", "/api/auth/methods", networkFailure);
    await signInPage();
    expect(await screen.findByRole("form", { name: "Sign in" })).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.queryByRole("link", { name: /Sign in with/ })).toBeNull();
    });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test.each(SSO_MESSAGES)("sso_error=%s says: %s", async (code, text) => {
    withSso();
    await signInPage(`/sign-in?sso_error=${code}`);
    expect(await screen.findByRole("alert")).toHaveTextContent(text);
    // The password form and the link are still there to try again.
    expect(screen.getByRole("form", { name: "Sign in" })).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "Sign in with Test IdP" })).toBeInTheDocument();
    expectOneH1("Sign in");
  });

  test("a code the console does not know is said in general words, never as it came", async () => {
    withSso();
    await signInPage("/sign-in?sso_error=%3Cb%3Eboom");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Single sign-on did not work. Try again.");
    expect(alert.textContent).not.toContain("boom");
  });

  test("a message is shown even when the provider is no longer offered", async () => {
    await signInPage("/sign-in?sso_error=config");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Single sign-on is not set up correctly. Ask an admin.",
    );
  });

  test("the code leaves the address once the message is shown, and next stays", async () => {
    withSso();
    startGateway();
    const app = await renderWithApp(null, {
      route: `/sign-in?sso_error=idp&next=${encodeURIComponent("/keys")}`,
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("Your identity provider refused");
    await waitFor(() => {
      expect(app.router.state.location.search).toEqual({ next: "/keys" });
    });
    // Still said after the address changed.
    expect(screen.getByRole("alert")).toHaveTextContent("Your identity provider refused");
    expect(
      await screen.findByRole("link", { name: "Sign in with Test IdP" }),
    ).toHaveAttribute("href", `/api/auth/oidc/start?return_to=${encodeURIComponent("/keys")}`);
  });

  test("without a next the address is the plain sign-in page", async () => {
    withSso();
    startGateway();
    const app = await renderWithApp(null, { route: "/sign-in?sso_error=state" });
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    await waitFor(() => {
      expect(app.router.state.location.search).toEqual({});
    });
    expect(app.router.state.location.href).toBe("/sign-in");
  });

  test("a password attempt replaces the message", async () => {
    withSso();
    startGateway();
    await renderWithApp(null, { route: "/sign-in?sso_error=idp" });
    expect(await screen.findByRole("alert")).toHaveTextContent("Your identity provider refused");
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Email"), "maya@example.test");
    await user.type(screen.getByLabelText("Password"), `${PASSWORD}-wrong`);
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("Email or password is incorrect.");
    });
  });

  test("asking for the methods is one call, and it needs no session", async () => {
    const methods = counted("get", "/api/auth/methods", () =>
      ok("get", "/api/auth/methods", 200, fixtures.signInMethods.withOidc),
    );
    await signInPage();
    await screen.findByRole("link", { name: "Sign in with Test IdP" });
    expect(methods.calls).toBe(1);
  });

  test("a refusal of the methods (401 from a proxy) does not end anything", async () => {
    override("get", "/api/auth/methods", () => refuse(errors.unauthenticated));
    await signInPage();
    expect(await screen.findByRole("form", { name: "Sign in" })).toBeInTheDocument();
  });

  test("at width 390 the link is as wide as the form and high enough to touch", async () => {
    withSso();
    await signInPage("/sign-in", 390);
    const link = await screen.findByRole("link", { name: "Sign in with Test IdP" });
    const classes = link.className.split(/\s+/);
    expect(classes).toContain("min-h-11");
    expect(classes).toContain("w-full");
  });
});
