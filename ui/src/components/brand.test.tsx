import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { AuthPage } from "@/components/AuthForm";
import { Brand } from "@/components/Brand";

describe("brand", () => {
  test("shows the mark and the name; the mark is decoration", () => {
    const { container } = render(<Brand />);
    expect(screen.getByText("Ultrafast")).toBeInTheDocument();
    const mark = container.querySelector("svg");
    expect(mark).toHaveAttribute("aria-hidden", "true");
    expect(mark?.querySelector("polygon")).toHaveClass("fill-brand-signal");
  });

  test("the sign-in, setup and invite pages carry the brand above their card", () => {
    render(
      <AuthPage title="Sign in">
        <p>form</p>
      </AuthPage>,
    );
    expect(screen.getByText("Ultrafast")).toBeInTheDocument();
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
  });
});
