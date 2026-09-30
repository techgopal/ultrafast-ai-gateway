import { describe, expect, test } from "vitest";
import * as fixtures from "@/test/fixtures";
import { entriesMatching } from "./audit";

const all = fixtures.auditEntries;

function ids(text: string): number[] {
  return entriesMatching(all, text).map((entry) => entry.id);
}

describe("the filter of the audit log", () => {
  test("no text leaves nothing out, and keeps the order", () => {
    expect(ids("")).toEqual([5, 4, 3, 2, 1]);
    expect(ids("   ")).toEqual([5, 4, 3, 2, 1]);
  });

  test("the text is looked for in the actor, the action and the summary", () => {
    // The actor.
    expect(ids("lena@")).toEqual([4]);
    // The action.
    expect(ids("team.create")).toEqual([3]);
    expect(ids("user.")).toEqual([5, 2]);
    // The summary.
    expect(ids("Growth")).toEqual([3]);
    expect(ids("disabled")).toEqual([2]);
  });

  test("without regard to case, and to blanks around the text", () => {
    expect(ids("GROWTH")).toEqual([3]);
    expect(ids("  Key.Revoke ")).toEqual([4]);
  });

  test("what is nowhere finds nothing", () => {
    expect(ids("no such thing")).toEqual([]);
  });

  test("the time and the ids are not looked at", () => {
    expect(ids("2026")).toEqual([]);
    expect(ids("09:00")).toEqual([]);
  });

  test("the entries that are given are not changed", () => {
    const given = [...all];
    entriesMatching(given, "lena");
    expect(given).toEqual(all);
  });
});
