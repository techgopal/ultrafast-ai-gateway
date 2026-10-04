// Tags on keys and calls: the editor of a new key, the tags of a key in the
// list and in a dialog of their own, and the tags in the logs.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { errors, fieldMessages, validationFailed } from "@/test/errors";
import * as fixtures from "@/test/fixtures";
import { startGateway } from "@/test/gateway";
import { ok, override, refuse } from "@/test/handlers";
import {
  counted,
  descriptionOf,
  expectOneH1,
  forgetToasts,
  href,
  installSelect,
  rowWithCell,
  SESSION_ENDED,
  settle,
  toasts,
} from "@/test/pages";
import { renderWithApp, unauthenticated, type AppRenderResult } from "@/test/render";

const { active, noOwner, revoked } = fixtures.keys;
const tagged: fixtures.Key = { ...active, tags: { team: "platform", env: "prod" } };

beforeAll(installSelect);
afterEach(forgetToasts);

type Options = { user?: fixtures.Me; route?: string };

function page(route: string, options: Options = {}): Promise<AppRenderResult> {
  return renderWithApp(null, { route, ...options });
}

function keysAre(list: readonly fixtures.Key[]) {
  const state = { keys: [...list], lists: 0, patched: [] as unknown[], created: [] as unknown[] };
  override("get", "/api/keys", () => {
    state.lists += 1;
    return ok("get", "/api/keys", 200, { keys: state.keys });
  });
  override("patch", "/api/keys/{id}", async ({ request, params }) => {
    const body = (await request.json()) as { tags: Record<string, string> };
    state.patched.push({ id: params.id, body });
    const found = state.keys.find((key) => String(key.id) === params.id);
    if (found === undefined) return refuse(errors.not_found);
    const key = { ...found, tags: body.tags };
    state.keys = state.keys.map((one) => (one.id === key.id ? key : one));
    return ok("patch", "/api/keys/{id}", 200, key);
  });
  override("post", "/api/keys", async ({ request }) => {
    state.created.push(await request.json());
    return ok("post", "/api/keys", 201, { key: active, secret: fixtures.newKeySecret });
  });
  return state;
}

async function table(): Promise<HTMLElement> {
  const found = await screen.findByRole("table", { name: "Virtual keys" });
  await waitFor(() => {
    expect(found).toHaveAttribute("aria-busy", "false");
  });
  return found;
}

async function openCreate(): Promise<HTMLElement> {
  await userEvent.click(await screen.findByRole("button", { name: "Create key" }));
  return screen.findByRole("dialog", { name: "Create key" });
}

function chipsIn(scope: HTMLElement): string[] {
  return [...scope.querySelectorAll('[role="group"][aria-label="Tags"] [data-slot="badge"]')].map(
    (chip) => chip.textContent,
  );
}

/** Fills the nth tag (from 1) of an editor. */
async function fill(dialog: HTMLElement, n: number, name: string, value: string): Promise<void> {
  await userEvent.click(within(dialog).getByRole("textbox", { name: `Tag ${n} name` }));
  await userEvent.paste(name);
  await userEvent.click(within(dialog).getByRole("textbox", { name: `Tag ${n} value` }));
  await userEvent.paste(value);
}

async function addTag(dialog: HTMLElement): Promise<void> {
  await userEvent.click(within(dialog).getByRole("button", { name: "Add tag" }));
}

describe("the tags of a new key", () => {
  test("rows are added and removed, and the request holds exactly the tags that are left", async () => {
    const state = keysAre([]);
    await page("/keys", { user: fixtures.me.lena });
    const dialog = await openCreate();
    // No row until one is added.
    expect(within(dialog).queryByRole("textbox", { name: "Tag 1 name" })).toBeNull();
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("laptop");
    await addTag(dialog);
    // This test types for real.
    await userEvent.type(within(dialog).getByRole("textbox", { name: "Tag 1 name" }), "team");
    await userEvent.type(within(dialog).getByRole("textbox", { name: "Tag 1 value" }), "platform");
    await addTag(dialog);
    await fill(dialog, 2, "env", "dev");
    await addTag(dialog);
    await fill(dialog, 3, "gone", "soon");
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove tag 3" }));
    expect(within(dialog).queryByRole("textbox", { name: "Tag 3 name" })).toBeNull();
    // A row that was added and left empty is not a tag.
    await addTag(dialog);
    await userEvent.click(within(dialog).getByRole("button", { name: "Create key" }));
    await screen.findByRole("dialog", { name: "Your new key" });
    expect(state.created).toEqual([{ name: "laptop", tags: { env: "dev", team: "platform" } }]);
  });

  test("a key without tags is created without the field", async () => {
    const state = keysAre([]);
    await page("/keys", { user: fixtures.me.lena });
    const dialog = await openCreate();
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("laptop");
    await userEvent.click(within(dialog).getByRole("button", { name: "Create key" }));
    await screen.findByRole("dialog", { name: "Your new key" });
    expect(state.created).toEqual([{ name: "laptop" }]);
  });

  test.each([
    ["a name with a space", ["a b", "v"], "A tag name may use only letters, digits and _ . : -"],
    ["a name without a value", ["a", ""], "Every tag needs a name and a value."],
    ["a long value", ["a", "v".repeat(65)], "A name or value is at most 64 characters."],
  ])("%s is refused by the console, on the field, and nothing is sent", async (_, [name, value], message) => {
    const posts = counted("post", "/api/keys", () => refuse(errors.internal_error));
    await page("/keys", { user: fixtures.me.lena });
    const dialog = await openCreate();
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("laptop");
    await addTag(dialog);
    await fill(dialog, 1, name ?? "", value ?? "");
    await userEvent.click(within(dialog).getByRole("button", { name: "Create key" }));
    const group = await within(dialog).findByRole("group", { name: "Tags" });
    await waitFor(() => {
      expect(descriptionOf(group)).toBe(message);
    });
    expect(group).toHaveAttribute("aria-invalid", "true");
    expect(posts.calls).toBe(0);
    expect(dialog).toBeInTheDocument();
    // What was typed stays.
    expect(within(dialog).getByRole("textbox", { name: "Tag 1 name" })).toHaveValue(name);
  });

  test("a tag name used twice is refused", async () => {
    const posts = counted("post", "/api/keys", () => refuse(errors.internal_error));
    await page("/keys", { user: fixtures.me.lena });
    const dialog = await openCreate();
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("laptop");
    await addTag(dialog);
    await fill(dialog, 1, "a", "1");
    await addTag(dialog);
    await fill(dialog, 2, "a", "2");
    await userEvent.click(within(dialog).getByRole("button", { name: "Create key" }));
    expect(await within(dialog).findByText("A tag name can be used once.")).toBeInTheDocument();
    expect(posts.calls).toBe(0);
  });

  test("no more than 20 rows can be added", async () => {
    keysAre([]);
    await page("/keys", { user: fixtures.me.lena });
    const dialog = await openCreate();
    for (let n = 0; n < 20; n += 1) await addTag(dialog);
    expect(within(dialog).getAllByRole("button", { name: /^Remove tag / })).toHaveLength(20);
    expect(within(dialog).getByRole("button", { name: "Add tag" })).toBeDisabled();
  });

  test("an error of the gateway about the tags shows on the field", async () => {
    override("post", "/api/keys", () => refuse(validationFailed({ tags: fieldMessages.tags })));
    await page("/keys", { user: fixtures.me.lena });
    const dialog = await openCreate();
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("laptop");
    await addTag(dialog);
    await fill(dialog, 1, "a", "1");
    await userEvent.click(within(dialog).getByRole("button", { name: "Create key" }));
    const group = await within(dialog).findByRole("group", { name: "Tags" });
    await waitFor(() => {
      expect(descriptionOf(group)).toBe(fieldMessages.tags);
    });
    expect(fieldMessages.tags).toBe("it has more than 20 entries");
  });

  test("a dialog that is opened again has no tags", async () => {
    keysAre([]);
    await page("/keys", { user: fixtures.me.lena });
    let dialog = await openCreate();
    await addTag(dialog);
    await fill(dialog, 1, "a", "1");
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    dialog = await openCreate();
    expect(within(dialog).queryByRole("textbox", { name: "Tag 1 name" })).toBeNull();
  });

  test("the session ends when the key is created: the form says nothing", async () => {
    startGateway({ signedIn: true, me: fixtures.me.lena });
    const posts = counted("post", "/api/keys", unauthenticated);
    const app = await page("/keys");
    const dialog = await openCreate();
    await userEvent.click(within(dialog).getByLabelText("Name"));
    await userEvent.paste("laptop");
    await addTag(dialog);
    await fill(dialog, 1, "a", "1");
    await userEvent.click(within(dialog).getByRole("button", { name: "Create key" }));
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/keys")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    await settle();
    expect(posts.calls).toBe(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("the tags of a key in the list", () => {
  test("they are chips, by name, and a key without tags has none", async () => {
    keysAre([tagged, noOwner]);
    await page("/keys");
    await table();
    const row = rowWithCell(tagged.name);
    expect(chipsIn(row)).toEqual(["env:prod", "team:platform"]);
    expect(within(row).getByRole("group", { name: "Tags" })).toBeInTheDocument();
    expect(chipsIn(rowWithCell(noOwner.name))).toEqual([]);
    expect(within(rowWithCell(noOwner.name)).getByText("No tags")).toBeInTheDocument();
  });
});

describe("changing the tags of a key", () => {
  async function openEdit(name: string): Promise<HTMLElement> {
    await userEvent.click(within(rowWithCell(name)).getByRole("button", { name: "Edit tags" }));
    return screen.findByRole("dialog", { name: "Edit tags" });
  }

  test("the dialog starts with the tags the key has; Save sends all of them, and the list shows them", async () => {
    const state = keysAre([tagged, noOwner]);
    await page("/keys");
    await table();
    const dialog = await openEdit(tagged.name);
    expect(within(dialog).getByRole("textbox", { name: "Tag 1 name" })).toHaveValue("env");
    expect(within(dialog).getByRole("textbox", { name: "Tag 1 value" })).toHaveValue("prod");
    expect(within(dialog).getByRole("textbox", { name: "Tag 2 name" })).toHaveValue("team");
    // Change a value, remove a tag, add one.
    await userEvent.clear(within(dialog).getByRole("textbox", { name: "Tag 1 value" }));
    await userEvent.type(within(dialog).getByRole("textbox", { name: "Tag 1 value" }), "dev");
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove tag 2" }));
    await addTag(dialog);
    await fill(dialog, 2, "job", "nightly");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(state.patched).toEqual([
      { id: String(tagged.id), body: { tags: { env: "dev", job: "nightly" } } },
    ]);
    // The toast names nothing.
    expect(toasts()).toEqual(["Tags saved."]);
    // The list was asked for again and shows what the gateway has now.
    await waitFor(() => {
      expect(chipsIn(rowWithCell(tagged.name))).toEqual(["env:dev", "job:nightly"]);
    });
    expect(state.lists).toBe(2);
  });

  test("removing every tag sends an empty object", async () => {
    const state = keysAre([tagged]);
    await page("/keys");
    await table();
    const dialog = await openEdit(tagged.name);
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove tag 2" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove tag 1" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(state.patched).toEqual([{ id: String(tagged.id), body: { tags: {} } }]);
    await waitFor(() => {
      expect(within(rowWithCell(tagged.name)).getByText("No tags")).toBeInTheDocument();
    });
  });

  test("a tag the console refuses is not sent, and the dialog stays with what was typed", async () => {
    const state = keysAre([tagged]);
    await page("/keys");
    await table();
    const dialog = await openEdit(tagged.name);
    await userEvent.type(within(dialog).getByRole("textbox", { name: "Tag 1 name" }), " x");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    const group = await within(dialog).findByRole("group", { name: "Tags" });
    await waitFor(() => {
      expect(descriptionOf(group)).toBe("A tag name may use only letters, digits and _ . : -");
    });
    expect(state.patched).toEqual([]);
    expect(within(dialog).getByRole("textbox", { name: "Tag 1 name" })).toHaveValue("env x");
    expect(toasts()).toEqual([]);
  });

  test("a refusal of the gateway stays in the dialog", async () => {
    keysAre([tagged]);
    const patches = counted("patch", "/api/keys/{id}", () => refuse(errors.not_found));
    await page("/keys");
    await table();
    const dialog = await openEdit(tagged.name);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      errors.not_found.body.error.message,
    );
    expect(patches.calls).toBe(1);
    expect(toasts()).toEqual([]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });

  test("the session ends when the tags are saved: the dialog says nothing", async () => {
    startGateway({ signedIn: true });
    keysAre([tagged]);
    const patches = counted("patch", "/api/keys/{id}", unauthenticated);
    const app = await page("/keys");
    await table();
    const dialog = await openEdit(tagged.name);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(href(app)).toBe(`/sign-in?next=${encodeURIComponent("/keys")}`);
    });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_ENDED);
    expect(patches.calls).toBe(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(toasts()).toEqual([]);
  });

  test("it is offered where the gateway allows it, and not for a revoked key", async () => {
    keysAre([active, noOwner, revoked, fixtures.keys.suspended]);
    // The lead of Platform: their team's keys, not a key without a team of another.
    await page("/keys", { user: fixtures.me.arjun });
    await table();
    await userEvent.click(screen.getByRole("checkbox", { name: "Show revoked" }));
    expect(within(rowWithCell(active.name)).getByRole("button", { name: "Edit tags" })).toBeInTheDocument();
    expect(within(rowWithCell(noOwner.name)).getByRole("button", { name: "Edit tags" })).toBeInTheDocument();
    expect(within(rowWithCell(revoked.name)).queryByRole("button", { name: "Edit tags" })).toBeNull();
  });

  test("a member sees it on their own keys only", async () => {
    const own: fixtures.Key = { ...active, id: 20, name: "lena-own", owner_id: fixtures.users.lena.id };
    const other: fixtures.Key = { ...active, id: 21, name: "other", owner_id: fixtures.users.tomas.id, team_id: null };
    keysAre([own, other]);
    await page("/keys", { user: fixtures.me.lena });
    await table();
    expect(within(rowWithCell(own.name)).getByRole("button", { name: "Edit tags" })).toBeInTheDocument();
    expect(within(rowWithCell(other.name)).queryByRole("button", { name: "Edit tags" })).toBeNull();
  });
});

describe("the tags of a call in the logs", () => {
  const withTags: fixtures.Log = { ...fixtures.logs.answered, tags: { env: "prod", team: "platform" } };

  function logsAre(list: readonly fixtures.Log[]) {
    const state = { asked: [] as URLSearchParams[] };
    override("get", "/api/logs", ({ request }) => {
      state.asked.push(new URL(request.url).searchParams);
      return ok("get", "/api/logs", 200, { logs: [...list] });
    });
    return state;
  }

  async function logTable(): Promise<HTMLElement> {
    const found = await screen.findByRole("table", { name: "Request logs" });
    await waitFor(() => {
      expect(found).toHaveAttribute("aria-busy", "false");
    });
    return found;
  }

  test("the list shows them as chips", async () => {
    logsAre([withTags, fixtures.logs.failedOver]);
    await page("/logs");
    const found = await logTable();
    const rows = [...found.querySelectorAll("tbody tr")] as HTMLElement[];
    expect(chipsIn(rows[0] as HTMLElement)).toEqual(["env:prod", "team:platform"]);
    expect(chipsIn(rows[1] as HTMLElement)).toEqual([]);
  });

  test("the Tag filter is applied on Enter, as name:value, and cleared when emptied", async () => {
    const asked = logsAre([withTags]);
    await page("/logs");
    await logTable();
    expect(asked.asked[0]?.has("tag")).toBe(false);
    const field = screen.getByRole("textbox", { name: "Tag" });
    await userEvent.type(field, "env:prod");
    // Not at every key, and not when it is left.
    await settle();
    expect(asked.asked).toHaveLength(1);
    await userEvent.type(field, "{Enter}");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.getAll("tag")).toEqual(["env:prod"]);
    });
    await userEvent.clear(field);
    await userEvent.type(field, "{Enter}");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.has("tag")).toBe(false);
    });
  });

  test("a filter that is not name:value is refused on the field and not sent", async () => {
    const asked = logsAre([withTags]);
    await page("/logs");
    await logTable();
    const field = screen.getByRole("textbox", { name: "Tag" });
    await userEvent.type(field, "env{Enter}");
    await waitFor(() => {
      expect(descriptionOf(field)).toBe("Write the tag as name:value.");
    });
    expect(field).toHaveAttribute("aria-invalid", "true");
    await settle();
    expect(asked.asked).toHaveLength(1);
    // A good one takes the error away.
    await userEvent.type(field, ":prod{Enter}");
    await waitFor(() => {
      expect(asked.asked.at(-1)?.getAll("tag")).toEqual(["env:prod"]);
    });
    expect(field).not.toHaveAttribute("aria-invalid");
  });

  test("the detail lists the tags, or says there are none", async () => {
    override("get", "/api/logs/{id}", ({ params }) =>
      ok("get", "/api/logs/{id}", 200, {
        ...(fixtures.logDetail(Number(params.id)) as fixtures.LogDetail),
        tags: params.id === "5" ? { env: "prod", team: "platform" } : {},
      }),
    );
    await page("/logs/5");
    const details = await screen.findByLabelText("Details");
    expect(within(details).getByText("Tags").nextElementSibling).toHaveTextContent("env:prod");
    expect(chipsIn(details)).toEqual(["env:prod", "team:platform"]);
    expectOneH1("Call");
  });

  test("a call without tags says so in the detail", async () => {
    await page("/logs/4");
    const details = await screen.findByLabelText("Details");
    expect(within(details).getByText("Tags").nextElementSibling).toHaveTextContent("No tags");
  });
});
