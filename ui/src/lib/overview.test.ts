import { describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import {
  countByStatus,
  exampleCall,
  firstSteps,
  KEY_STATUSES,
  USER_STATUSES,
  withCredential,
} from "./overview";

describe("counting by status", () => {
  test("the keys of the fixtures", () => {
    expect(countByStatus(fixtures.keyList, KEY_STATUSES)).toEqual([
      { status: "active", count: 2 },
      { status: "suspended", count: 1 },
      { status: "expired", count: 1 },
      { status: "revoked", count: 1 },
    ]);
  });

  test("the users of the fixtures", () => {
    expect(countByStatus(fixtures.userList, USER_STATUSES)).toEqual([
      { status: "active", count: 5 },
      { status: "invited", count: 1 },
      { status: "disabled", count: 1 },
    ]);
  });

  test("the counts add up to the number of things", () => {
    for (const [list, known] of [
      [fixtures.keyList, KEY_STATUSES],
      [fixtures.userList, USER_STATUSES],
    ] as const) {
      const total = countByStatus(list, known).reduce((sum, one) => sum + one.count, 0);
      expect(total).toBe(list.length);
    }
  });

  test("the known statuses come in their order, whatever the order of the things", () => {
    const statuses = ["revoked", "active", "expired", "active", "suspended"];
    const counts = countByStatus(
      statuses.map((status) => ({ status })),
      KEY_STATUSES,
    );
    expect(counts.map((one) => one.status)).toEqual(["active", "suspended", "expired", "revoked"]);
  });

  test("a status that nothing has is left out", () => {
    expect(countByStatus([{ status: "revoked" }], KEY_STATUSES)).toEqual([
      { status: "revoked", count: 1 },
    ]);
    expect(countByStatus([], KEY_STATUSES)).toEqual([]);
  });

  test("a status the console does not know is counted under its own text, after the known ones", () => {
    const things = ["zeta", "active", "frozen", "frozen", "revoked"].map((status) => ({ status }));
    expect(countByStatus(things, KEY_STATUSES)).toEqual([
      { status: "active", count: 1 },
      { status: "revoked", count: 1 },
      { status: "frozen", count: 2 },
      { status: "zeta", count: 1 },
    ]);
  });

  test("a status with the name of something every object has is a status like any other", () => {
    const things = ["constructor", "toString", "constructor"].map((status) => ({ status }));
    expect(countByStatus(things, KEY_STATUSES)).toEqual([
      { status: "constructor", count: 2 },
      { status: "toString", count: 1 },
    ]);
  });
});

describe("providers with a credential", () => {
  test("the providers of the fixtures", () => {
    expect(withCredential(fixtures.providerList)).toBe(1);
  });

  test("none, and all", () => {
    expect(withCredential([])).toBe(0);
    expect(withCredential([fixtures.providers.withoutCredential])).toBe(0);
    expect(
      withCredential([fixtures.providers.withCredential, fixtures.providers.withCredential]),
    ).toBe(2);
  });
});

describe("the first steps", () => {
  test("without a provider there is something to get started with, whatever the keys", () => {
    expect(firstSteps(0, 0)).toEqual({ provider: false, key: false });
    expect(firstSteps(0, 3)).toEqual({ provider: false, key: true });
  });

  test("with a provider and no key, the first step is done", () => {
    expect(firstSteps(1, 0)).toEqual({ provider: true, key: false });
    expect(firstSteps(4, 0)).toEqual({ provider: true, key: false });
  });

  test("with a provider and a key there is nothing to show", () => {
    expect(firstSteps(1, 1)).toBeNull();
    expect(firstSteps(2, 5)).toBeNull();
  });
});

describe("the example of a first call", () => {
  const example = exampleCall("https://gateway.example.test");

  test("it calls the chat completions of the gateway at its origin", () => {
    expect(example.split("\n")[0]).toBe(
      "curl https://gateway.example.test/v1/chat/completions \\",
    );
  });

  test("the key and the model are placeholders", () => {
    expect(example).toContain('-H "Authorization: Bearer <key>"');
    expect(example).toContain('"model": "<provider>/<model>"');
    expect(example).not.toMatch(/uf-(sk|at)-/);
  });

  test("what it sends is JSON when the placeholders are filled", () => {
    const body = /-d '(.*)'$/.exec(example)?.[1] ?? "";
    expect(JSON.parse(body)).toEqual({
      model: "<provider>/<model>",
      messages: [{ role: "user", content: "Hello" }],
    });
  });

  test("every line but the last goes on in the next", () => {
    const lines = example.split("\n");
    expect(lines).toHaveLength(4);
    expect(lines.slice(0, -1).every((line) => line.endsWith(" \\"))).toBe(true);
    expect(lines.at(-1)?.endsWith("\\")).toBe(false);
  });
});
