import { screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { PageHeader } from "@/components/PageHeader";
import { StatusBadge } from "@/components/StatusBadge";
import { renderWithApp } from "@/test/render";

test("page header shows title, subtitle and actions", async () => {
  await renderWithApp(
    <PageHeader
      title="Users"
      subtitle="People who can sign in"
      actions={<button type="button">Invite</button>}
    />,
  );
  expect(screen.getByRole("heading", { level: 1, name: "Users" })).toBeInTheDocument();
  expect(screen.getByText("People who can sign in")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Invite" })).toBeInTheDocument();
});

test.each([
  ["active", "default"],
  ["ok", "default"],
  ["invited", "secondary"],
  ["suspended", "secondary"],
  ["warning", "secondary"],
  ["error", "destructive"],
  ["disabled", "outline"],
  ["expired", "outline"],
  ["revoked", "outline"],
  ["something-new", "outline"],
])("status badge states %s in text, as %s", async (status, variant) => {
  await renderWithApp(<StatusBadge status={status} />);
  const badge = screen.getByText(status);
  expect(badge).toHaveAttribute("data-variant", variant);
});
