import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { DataTable, type Column } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { StatusBadge } from "@/components/StatusBadge";
import { Button } from "@/components/ui/button";
import { renderWithApp } from "@/test/render";

interface Key {
  id: number;
  name: string;
  status: string;
  requests: number;
  created: string;
}

const rows: Key[] = [
  { id: 1, name: "mobile", status: "active", requests: 20, created: "2026-03-01" },
  { id: 2, name: "Batch", status: "revoked", requests: 3, created: "2026-01-01" },
  { id: 3, name: "ci", status: "active", requests: 100, created: "2026-02-01" },
];

const columns: Column<Key>[] = [
  { id: "name", header: "Name", cell: (key) => key.name, sortValue: (key) => key.name },
  { id: "status", header: "Status", cell: (key) => <StatusBadge status={key.status} /> },
  {
    id: "requests",
    header: "Requests",
    cell: (key) => key.requests,
    sortValue: (key) => key.requests,
  },
  { id: "created", header: "Created", cell: (key) => key.created, hideOnMobile: true },
];

const revoke = vi.fn();

function Keys({ list = rows, loading = false }: { list?: Key[]; loading?: boolean }) {
  return (
    <DataTable
      caption="Virtual keys"
      columns={columns}
      rows={list}
      loading={loading}
      getRowId={(key) => String(key.id)}
      actions={(key) => (
        <Button
          type="button"
          variant="outline"
          onClick={() => {
            revoke(key.id);
          }}
        >
          Revoke {key.name}
        </Button>
      )}
      empty={<EmptyState title="No keys yet" description="Create a key to call the gateway." />}
    />
  );
}

/** The text of the first cell of every row of the body. */
function names(): (string | null)[] {
  const [, body] = screen.getAllByRole("rowgroup");
  if (body === undefined) throw new Error("the table has no body");
  return within(body)
    .getAllByRole("row")
    .map((row) => within(row).getAllByRole("cell")[0]?.textContent ?? null);
}

describe("data table", () => {
  test("it is a table with a caption and column headers", async () => {
    await renderWithApp(<Keys />);
    const table = screen.getByRole("table", { name: "Virtual keys" });
    expect(table.tagName).toBe("TABLE");
    expect(table.querySelector("caption")).toHaveTextContent("Virtual keys");
    const headers = within(table).getAllByRole("columnheader");
    expect(headers.map((th) => th.textContent)).toEqual([
      "Name",
      "Status",
      "Requests",
      "Created",
      "Actions",
    ]);
    for (const th of headers) {
      expect(th.tagName).toBe("TH");
      expect(th).toHaveAttribute("scope", "col");
    }
    // The rows come in the order they were given.
    expect(names()).toEqual(["mobile", "Batch", "ci"]);
    expect(within(table).getByRole("button", { name: "Revoke ci" })).toBeInTheDocument();
    // A column that does not sort has no button and no aria-sort.
    const status = within(table).getByRole("columnheader", { name: "Status" });
    expect(within(status).queryByRole("button")).toBeNull();
    expect(status).not.toHaveAttribute("aria-sort");
  });

  test("table sorts", async () => {
    await renderWithApp(<Keys />);
    const header = screen.getByRole("columnheader", { name: "Name" });
    const button = within(header).getByRole("button", { name: "Name" });
    expect(header).toHaveAttribute("aria-sort", "none");

    await userEvent.click(button);
    expect(header).toHaveAttribute("aria-sort", "ascending");
    expect(names()).toEqual(["Batch", "ci", "mobile"]);

    await userEvent.click(button);
    expect(header).toHaveAttribute("aria-sort", "descending");
    expect(names()).toEqual(["mobile", "ci", "Batch"]);

    await userEvent.click(button);
    expect(header).toHaveAttribute("aria-sort", "ascending");

    // Numbers sort as numbers, ascending first, and only one column is sorted.
    const requests = screen.getByRole("columnheader", { name: "Requests" });
    await userEvent.click(within(requests).getByRole("button", { name: "Requests" }));
    expect(requests).toHaveAttribute("aria-sort", "ascending");
    expect(header).toHaveAttribute("aria-sort", "none");
    expect(names()).toEqual(["Batch", "mobile", "ci"]);
  });

  test("a list of exactly one row", async () => {
    const [only] = rows;
    if (only === undefined) throw new Error("the test has no rows");
    await renderWithApp(<Keys list={[only]} />);
    const table = screen.getByRole("table", { name: "Virtual keys" });
    expect(table).not.toHaveAttribute("aria-busy", "true");
    expect(screen.queryByText("No keys yet")).toBeNull();
    expect(names()).toEqual(["mobile"]);
    expect(screen.getAllByRole("button", { name: /^Revoke / })).toHaveLength(1);
    // One row sorts too, and stays.
    const header = screen.getByRole("columnheader", { name: "Name" });
    await userEvent.click(within(header).getByRole("button", { name: "Name" }));
    expect(header).toHaveAttribute("aria-sort", "ascending");
    expect(names()).toEqual(["mobile"]);
    await userEvent.click(within(header).getByRole("button", { name: "Name" }));
    expect(header).toHaveAttribute("aria-sort", "descending");
    expect(names()).toEqual(["mobile"]);
  });

  test("a list of exactly one row, as a card", async () => {
    const [only] = rows;
    if (only === undefined) throw new Error("the test has no rows");
    await renderWithApp(<Keys list={[only]} />, { width: 390 });
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByText("No keys yet")).toBeNull();
    expect(screen.getAllByText("mobile")).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: /^Revoke / })).toHaveLength(1);
  });

  test("a list of 500 rows is shown and sorts", async () => {
    // 37 and 500 share no factor: every number of requests from 0 to 499 is there once.
    const many: Key[] = Array.from({ length: 500 }, (_, index) => ({
      id: index + 1,
      name: `key-${String(index + 1)}`,
      status: index % 7 === 0 ? "revoked" : "active",
      requests: (index * 37) % 500,
      created: "2026-03-01",
    }));
    const started = performance.now();
    await renderWithApp(<Keys list={many} />);
    const took = performance.now() - started;
    // Measured in the test environment (jsdom): see the report. The bound is
    // far above it, and says only that the render does not hang.
    expect(took).toBeLessThan(20_000);
    // Queries by role take seconds on 500 rows in jsdom: the table and its
    // rows are read through the elements, and the name of the one table is asked.
    const table = document.querySelector("table");
    if (table === null) throw new Error("no table");
    expect(table).toHaveAccessibleName("Virtual keys");
    const head = table.querySelector("thead");
    if (head === null) throw new Error("the table has no head");
    const shown = () =>
      [...table.querySelectorAll("tbody tr")].map(
        (row) => row.querySelector("td")?.textContent ?? null,
      );
    const all = shown();
    expect(all).toHaveLength(500);
    expect(all[0]).toBe("key-1");
    expect(all[499]).toBe("key-500");
    expect(table.querySelectorAll("tbody button")).toHaveLength(500);

    const requests = within(head).getByRole("columnheader", { name: "Requests" });
    const button = within(requests).getByRole("button", { name: "Requests" });
    await userEvent.click(button);
    expect(requests).toHaveAttribute("aria-sort", "ascending");
    const byRequests = (order: "asc" | "desc") =>
      [...many]
        .sort((a, b) => (order === "asc" ? a.requests - b.requests : b.requests - a.requests))
        .map((key) => key.name);
    expect(shown()).toEqual(byRequests("asc"));
    await userEvent.click(button);
    expect(requests).toHaveAttribute("aria-sort", "descending");
    expect(shown()).toEqual(byRequests("desc"));

    // Names with numbers sort by the value of the number: key-2 before key-10.
    const name = within(head).getByRole("columnheader", { name: "Name" });
    await userEvent.click(within(name).getByRole("button", { name: "Name" }));
    expect(shown()).toEqual(many.map((key) => key.name));
    if (process.env.UF_SHOW_TIMES === "1") {
      console.info(`500 rows: first render ${took.toFixed(0)} ms`);
    }
  }, 30_000);

  test("the sort button works from the keyboard", async () => {
    await renderWithApp(<Keys />);
    const header = screen.getByRole("columnheader", { name: "Name" });
    within(header).getByRole("button", { name: "Name" }).focus();
    await userEvent.keyboard("{Enter}");
    expect(header).toHaveAttribute("aria-sort", "ascending");
  });

  test("table empty and loading", async () => {
    const empty = await renderWithApp(<Keys list={[]} />);
    expect(screen.getByText("No keys yet")).toBeInTheDocument();
    expect(screen.getByText("Create a key to call the gateway.")).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    empty.unmount();

    await renderWithApp(<Keys list={[]} loading />);
    const table = screen.getByRole("table", { name: "Virtual keys" });
    expect(table).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByText("No keys yet")).toBeNull();
    const [, body] = within(table).getAllByRole("rowgroup");
    if (body === undefined) throw new Error("the table has no body");
    const skeletons = within(body).getAllByRole("row");
    expect(skeletons).toHaveLength(5);
    for (const row of skeletons) {
      expect(row.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(5);
    }
  });

  test("table becomes cards on narrow screens", async () => {
    revoke.mockClear();
    await renderWithApp(<Keys />, { width: 390 });
    await waitFor(() => {
      expect(screen.queryByRole("table")).toBeNull();
    });
    expect(document.querySelector("table, tr, td, th")).toBeNull();
    const list = screen.getByRole("list", { name: "Virtual keys" });
    const cards = within(list).getAllByRole("listitem");
    expect(cards).toHaveLength(3);
    const [first] = cards;
    if (first === undefined) throw new Error("no card");
    // Every cell is a line: the header of the column, then the value, in the order of the columns.
    const lines = [...first.querySelectorAll("dt")].map((dt) => [
      dt.textContent,
      dt.nextElementSibling?.tagName,
      dt.nextElementSibling?.textContent,
    ]);
    expect(lines).toEqual([
      ["Name", "DD", "mobile"],
      ["Status", "DD", "active"],
      ["Requests", "DD", "20"],
    ]);
    expect(within(list).queryByText("Created")).toBeNull();
    expect(within(list).queryByText("2026-03-01")).toBeNull();
    // The actions are at the end of the card.
    const action = within(first).getByRole("button", { name: "Revoke mobile" });
    const lastLine = first.querySelector("dl");
    if (lastLine === null) throw new Error("no lines");
    expect(
      lastLine.compareDocumentPosition(action) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    await userEvent.click(action);
    expect(revoke).toHaveBeenCalledWith(1);
  });

  test("narrow screens: empty and loading", async () => {
    const empty = await renderWithApp(<Keys list={[]} />, { width: 390 });
    expect(screen.getByText("No keys yet")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Virtual keys" })).toBeNull();
    empty.unmount();

    await renderWithApp(<Keys list={[]} loading />, { width: 390 });
    const list = await screen.findByRole("list", { name: "Virtual keys" });
    expect(list).toHaveAttribute("aria-busy", "true");
    expect(within(list).getAllByRole("listitem")).toHaveLength(5);
    expect(document.querySelector("table")).toBeNull();
  });

  test("the table turns into cards when the screen gets narrow", async () => {
    const { setDevice } = await import("@/test/device");
    const { act } = await import("@testing-library/react");
    await renderWithApp(<Keys />);
    expect(screen.getByRole("table")).toBeInTheDocument();
    act(() => {
      setDevice({ width: 767 });
    });
    await waitFor(() => {
      expect(screen.queryByRole("table")).toBeNull();
    });
    act(() => {
      setDevice({ width: 768 });
    });
    await waitFor(() => {
      expect(screen.getByRole("table")).toBeInTheDocument();
    });
  });

  test.each(["light", "dark"] as const)("the table shows its text in the %s theme", async (theme) => {
    await renderWithApp(<Keys />, { theme });
    expect(screen.getByRole("table", { name: "Virtual keys" })).toHaveTextContent("mobile");
    expect(screen.getByRole("columnheader", { name: "Requests" })).toBeInTheDocument();
  });

  test("a status the console does not know is shown as it is", async () => {
    const list = [{ ...rows[0], id: 9, name: "odd", status: "quarantined" } as Key];
    await renderWithApp(<Keys list={list} />);
    const badge = screen.getByText("quarantined");
    expect(badge).toHaveAttribute("data-variant", "outline");
    expect(screen.queryByText("active")).toBeNull();
  });
});
