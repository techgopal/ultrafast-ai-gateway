import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { Sparkline } from "@/components/Sparkline";

describe("Sparkline", () => {
  test("is an image with a title and a name that says the lowest and the highest", () => {
    render(<Sparkline label="Requests per day" values={[3, 9, 0, 5]} format={String} />);
    const image = screen.getByRole("img");
    expect(image.tagName.toLowerCase()).toBe("svg");
    const name = "Requests per day, 4 days, lowest 0, highest 9";
    expect(image).toHaveAccessibleName(name);
    expect(image.querySelector("title")?.textContent).toBe(name);
  });

  test("takes its colour from the theme, with no literal", () => {
    const { container } = render(<Sparkline label="x" values={[1, 2, 3]} format={String} />);
    const html = container.innerHTML;
    expect(html).not.toMatch(/#[0-9a-f]{3,8}\b|rgb\(|hsl\(|oklch\(/i);
    expect(html).toContain("currentColor");
    expect(container.querySelector("[style]")).toBeNull();
  });

  test("a flat series is a flat line, and one point still draws", () => {
    const { container } = render(<Sparkline label="x" values={[0, 0, 0]} format={String} />);
    const points = container.querySelector("polyline")?.getAttribute("points") ?? "";
    expect(points.split(" ")).toHaveLength(3);
    expect(new Set(points.split(" ").map((p) => p.split(",")[1])).size).toBe(1);
    expect(points).not.toMatch(/NaN|Infinity/);
  });

  test("draws nothing for no values", () => {
    const { container } = render(<Sparkline label="x" values={[]} format={String} />);
    expect(container.querySelector("svg")).toBeNull();
  });
});
