import { screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { renderWithApp } from "@/test/render";

// The policy lets the same people read the audit log and change the
// settings today. The page asks each question on its own, so here only the
// audit action is refused.
vi.mock("@/auth/guards", async (original) => {
  const guards = await original<typeof import("@/auth/guards")>();
  return {
    ...guards,
    can: (me: Parameters<typeof guards.can>[0], action: Parameters<typeof guards.can>[1]) =>
      action.type === "viewAudit" ? false : guards.can(me, action),
  };
});

describe("the audit view of the settings", () => {
  test("is not offered, and not shown at its address, to a viewer who may not read the audit log", async () => {
    await renderWithApp(null, { route: "/settings#audit" });
    expect(await screen.findByLabelText("Keep request logs for (days)")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Audit log" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Audit log" })).toBeNull();
    expect(screen.queryByRole("table", { name: "Audit log" })).toBeNull();
  });
});
