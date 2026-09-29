import { render } from "@testing-library/react";
import { setNonce } from "get-nonce";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

// jsdom has no layout, so it lacks what the list calls to show its chosen item.
beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(() => {
  Reflect.deleteProperty(Element.prototype, "scrollIntoView");
});

// Radix Select renders a <style> element in its list. The policy applies it
// only when it carries the nonce of the page.
test("the style element of an open select carries the nonce", () => {
  setNonce("q83vEjRWeJCrze8SNFZ4kA");
  render(
    <Select open value="a">
      <SelectTrigger aria-label="Letter">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="a">A</SelectItem>
      </SelectContent>
    </Select>,
  );
  const styles = [...document.querySelectorAll("style")];
  expect(styles.length).toBeGreaterThan(0);
  for (const style of styles) {
    expect(style.nonce).toBe("q83vEjRWeJCrze8SNFZ4kA");
  }
});
