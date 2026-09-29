import {
  createSortedRowModel,
  rowSortingFeature,
  tableFeatures,
  useTable,
  type ColumnDef,
  type Row,
  type RowData,
} from "@tanstack/react-table";
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import { useMemo, type ReactElement, type ReactNode } from "react";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCaption,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useIsMobile } from "@/hooks/use-mobile";

export interface Column<T extends RowData> {
  id: string;
  /** The header of the column, and the label of its line on a card. */
  header: string;
  cell: (row: T) => ReactNode;
  /** What the column sorts by. A column without it does not sort. */
  sortValue?: (row: T) => string | number | null;
  /** Left out of the cards that are shown on a narrow screen. */
  hideOnMobile?: boolean;
}

interface DataTableProps<T extends RowData> {
  /** What the table lists. It is its accessible name. */
  caption: string;
  columns: readonly Column<T>[];
  rows: readonly T[];
  getRowId: (row: T) => string;
  /** Shown in place of the table when there are no rows. */
  empty: ReactElement;
  /** The rows are on their way: skeleton rows are shown. */
  loading?: boolean;
  /** What can be done with a row. It is the last cell of the row, and the end of its card. */
  actions?: (row: T) => ReactNode;
}

const SKELETON_ROWS = [0, 1, 2, 3, 4];

const features = tableFeatures({
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
});
type Features = typeof features;

type SortValue = string | number | null;

/** Ascending. Text sorts without regard to case, numbers in it by their value; nothing sorts last. */
function compare(a: SortValue, b: SortValue): number {
  if (a === null || b === null) return a === b ? 0 : a === null ? 1 : -1;
  if (typeof a === "number" && typeof b === "number") return a - b;
  return String(a).localeCompare(String(b), undefined, { sensitivity: "base", numeric: true });
}

const ARIA_SORT = { asc: "ascending", desc: "descending", none: "none" } as const;

/**
 * A list of things as a table that sorts in the browser. Below 768 px every
 * row is a card: each cell a line with the header of its column.
 */
export function DataTable<T extends RowData>({
  caption,
  columns,
  rows,
  getRowId,
  empty,
  loading = false,
  actions,
}: DataTableProps<T>) {
  const narrow = useIsMobile();
  const data = useMemo(() => [...rows], [rows]);
  const defs = useMemo(
    () =>
      columns.map((column): ColumnDef<Features, T> => {
        const { sortValue } = column;
        return {
          id: column.id,
          header: column.header,
          accessorFn: (row: T) => sortValue?.(row) ?? null,
          enableSorting: sortValue !== undefined,
          sortDescFirst: false,
          sortUndefined: false,
          sortFn: (a: Row<Features, T>, b: Row<Features, T>) =>
            sortValue === undefined ? 0 : compare(sortValue(a.original), sortValue(b.original)),
        };
      }),
    [columns],
  );
  const table = useTable({
    features,
    data,
    columns: defs,
    getRowId,
    enableSortingRemoval: false,
    enableMultiSort: false,
  });

  if (!loading && rows.length === 0) return empty;

  const sorted = table.getRowModel().rows;

  if (narrow) {
    const lines = columns.filter((column) => column.hideOnMobile !== true);
    return (
      <ul aria-label={caption} aria-busy={loading} className="flex flex-col gap-3">
        {loading
          ? SKELETON_ROWS.map((row) => (
              <li key={row} className="flex flex-col gap-3 rounded-lg border bg-card p-4">
                {lines.map((column) => (
                  <Skeleton key={column.id} className="h-4 w-full" />
                ))}
              </li>
            ))
          : sorted.map((row) => (
              <li
                key={row.id}
                className="flex flex-col gap-3 rounded-lg border bg-card p-4 text-card-foreground"
              >
                <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
                  {lines.map((column) => (
                    <div key={column.id} className="contents">
                      <dt className="text-muted-foreground">{column.header}</dt>
                      <dd className="min-w-0 break-words">{column.cell(row.original)}</dd>
                    </div>
                  ))}
                </dl>
                {actions === undefined ? null : (
                  <div className="flex flex-wrap gap-2 *:min-h-11">{actions(row.original)}</div>
                )}
              </li>
            ))}
      </ul>
    );
  }

  return (
    <Table aria-busy={loading}>
      <TableCaption className="sr-only">{caption}</TableCaption>
      <TableHeader>
        <TableRow>
          {columns.map((column) => {
            const state = table.getColumn(column.id);
            if (column.sortValue === undefined || state === undefined) {
              return (
                <TableHead key={column.id} scope="col">
                  {column.header}
                </TableHead>
              );
            }
            const direction = state.getIsSorted();
            const Icon = direction === "asc" ? ArrowUp : direction === "desc" ? ArrowDown : ArrowUpDown;
            return (
              <TableHead
                key={column.id}
                scope="col"
                aria-sort={ARIA_SORT[direction === false ? "none" : direction]}
              >
                <button
                  type="button"
                  className="-mx-2 inline-flex h-8 items-center gap-1 rounded-md px-2 font-medium outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50"
                  onClick={() => {
                    state.toggleSorting();
                  }}
                >
                  {column.header}
                  <Icon aria-hidden="true" className="size-3.5 text-muted-foreground" />
                </button>
              </TableHead>
            );
          })}
          {actions === undefined ? null : (
            <TableHead scope="col">
              <span className="sr-only">Actions</span>
            </TableHead>
          )}
        </TableRow>
      </TableHeader>
      <TableBody>
        {loading
          ? SKELETON_ROWS.map((row) => (
              <TableRow key={row}>
                {columns.map((column) => (
                  <TableCell key={column.id}>
                    <Skeleton className="h-4 w-full" />
                  </TableCell>
                ))}
                {actions === undefined ? null : (
                  <TableCell>
                    <Skeleton className="h-4 w-full" />
                  </TableCell>
                )}
              </TableRow>
            ))
          : sorted.map((row) => (
              <TableRow key={row.id}>
                {columns.map((column) => (
                  <TableCell key={column.id}>{column.cell(row.original)}</TableCell>
                ))}
                {actions === undefined ? null : (
                  <TableCell>
                    <div className="flex justify-end gap-2">{actions(row.original)}</div>
                  </TableCell>
                )}
              </TableRow>
            ))}
      </TableBody>
    </Table>
  );
}
