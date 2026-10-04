import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { MAX_TAGS, type TagRow } from "@/lib/tags";

interface TagsEditorProps {
  rows: readonly TagRow[];
  onChange: (rows: TagRow[]) => void;
  /** What is wrong with the tags. */
  error: string | undefined;
}

/** Rows of a name and a value, to add and to remove. Checked when the form is sent. */
export function TagsEditor({ rows, onChange, error }: TagsEditorProps) {
  const change = (at: number, patch: Partial<TagRow>) => {
    onChange(rows.map((row, index) => (index === at ? { ...row, ...patch } : row)));
  };
  return (
    <Field group label="Tags" name="tags" error={error}>
      {(wiring) => (
        <div
          role="group"
          {...(wiring["aria-labelledby"] === undefined
            ? {}
            : { "aria-labelledby": wiring["aria-labelledby"] })}
          {...(wiring["aria-describedby"] === undefined
            ? {}
            : { "aria-describedby": wiring["aria-describedby"] })}
          {...(wiring["aria-invalid"] === undefined ? {} : { "aria-invalid": true })}
          className="flex flex-col gap-2"
        >
          {rows.map((row, index) => (
            // The rows have no identity but their place: removing one moves the others up.
            <div key={index} className="flex flex-wrap items-center gap-2">
              <Input
                aria-label={`Tag ${index + 1} name`}
                placeholder="Name"
                autoComplete="off"
                className={`${control} min-w-0 flex-1 basis-32`}
                value={row.name}
                onChange={(event) => {
                  change(index, { name: event.target.value });
                }}
              />
              <Input
                aria-label={`Tag ${index + 1} value`}
                placeholder="Value"
                autoComplete="off"
                className={`${control} min-w-0 flex-1 basis-32`}
                value={row.value}
                onChange={(event) => {
                  change(index, { value: event.target.value });
                }}
              />
              <Button
                type="button"
                variant="outline"
                className={control}
                aria-label={`Remove tag ${index + 1}`}
                onClick={() => {
                  onChange(rows.filter((_, at) => at !== index));
                }}
              >
                Remove
              </Button>
            </div>
          ))}
          <div>
            <Button
              type="button"
              variant="outline"
              className={control}
              disabled={rows.length >= MAX_TAGS}
              onClick={() => {
                onChange([...rows, { name: "", value: "" }]);
              }}
            >
              Add tag
            </Button>
          </div>
        </div>
      )}
    </Field>
  );
}
