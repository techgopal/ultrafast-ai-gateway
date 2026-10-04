import { screen, within } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { queryKeys } from "@/api/queries";
import { active, changeButton, fields, main, maya, page, part, table } from "@/test/account";
import { startGateway } from "@/test/gateway";
import {
  expectOneH1,
  expectOneMain,
  expectSessionEndsOnPage,
  forgetToasts,
  installSelect,
  shown,
  toasts,
} from "@/test/pages";

beforeAll(installSelect);
afterEach(forgetToasts);

describe("the account page", () => {
  test("it has three parts, under one heading and in one main", async () => {
    await page();
    await table();
    expect(screen.getByRole("heading", { level: 1, name: "Account" })).toBeInTheDocument();
    expect(
      within(main())
        .getAllByRole("heading", { level: 2 })
        .map((heading) => heading.textContent),
    ).toEqual(["Profile", "Password", "Access tokens"]);
    expect(within(main()).getAllByRole("region")).toEqual([
      part("Profile"),
      part("Password"),
      part("Access tokens"),
    ]);
    expectOneMain();
    expectOneH1();
    expect(toasts()).toEqual([]);
  });

  test("the session ends while the page is open", async () => {
    startGateway({ signedIn: true });
    const app = await page();
    await table();
    await expectSessionEndsOnPage(app, {
      path: "/api/tokens",
      queryKey: queryKeys.tokens.list(),
      at: "/account",
    });
    expect(shown()).not.toContain(active.display);
    expect(shown()).not.toContain(maya.email);
  });

  test("at width 390 the controls are high enough to touch", async () => {
    await page({ width: 390 });
    await screen.findByRole("list", { name: "Access tokens" });
    for (const control of [
      within(part("Profile")).getByRole("button", { name: "Edit name" }),
      ...fields(),
      changeButton(),
      within(part("Access tokens")).getByRole("button", { name: "Create token" }),
    ]) {
      expect(control.className.split(/\s+/)).toContain("min-h-11");
    }
    // The fields are as wide as the page at most.
    for (const one of fields()) expect(one.className.split(/\s+/)).toContain("w-full");
    expectOneMain();
    expectOneH1();
  });
});
