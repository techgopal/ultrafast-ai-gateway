import { useId } from "react";
import { control, selectList } from "@/components/classes";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
  ACTIONS,
  DIRECTIONS,
  newRule,
  PII_TYPES,
  type Action,
  type Directions,
  type MatcherKind,
  type PiiType,
  type RuleRow,
} from "@/lib/guardrails";

export const KINDS: readonly [MatcherKind, string][] = [
  ["pii", "PII and secrets"],
  ["keywords", "Keywords"],
  ["regex", "Regular expression"],
];

export const WORDS_HINT =
  "One word or phrase for each line. Case does not matter. Scripts written without spaces are matched anywhere in a word.";
export const REGEX_HINT =
  "Rust regular expression syntax. Anchors (^ and $) are refused, and a match longer than 256 characters may be missed in a stream.";
export const ACTION_HINT =
  "Redact replaces the match with a placeholder, Block refuses the call, Flag only records it.";

function RowError({ id, message }: { id: string; message: string | undefined }) {
  if (message === undefined) return null;
  return (
    <p id={id} role="alert" className="text-sm text-destructive">
      {message}
    </p>
  );
}

interface RowProps {
  row: RuleRow;
  index: number;
  error: string | undefined;
  onChange: (row: RuleRow) => void;
  onRemove: () => void;
}

function Rule({ row, index, error, onChange, onRemove }: RowProps) {
  const base = useId();
  const errorId = `${base}-error`;
  const described = error === undefined ? {} : { "aria-invalid": true as const, "aria-describedby": errorId };
  const set = (patch: Partial<RuleRow>) => {
    onChange({ ...row, ...patch });
  };
  return (
    <div
      role="group"
      aria-label={`Rule ${String(index + 1)}`}
      className="flex flex-col gap-3 rounded-lg border bg-card p-3"
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="flex flex-col gap-2">
          <Label htmlFor={`${base}-id`}>Rule id</Label>
          <Input
            id={`${base}-id`}
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            className={`${control} font-mono`}
            value={row.id}
            {...described}
            onChange={(event) => {
              set({ id: event.target.value });
            }}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor={`${base}-kind`}>Matches</Label>
          <Select
            name={`${base}-kind`}
            value={row.kind}
            onValueChange={(kind) => {
              set({ kind: kind as MatcherKind });
            }}
          >
            <SelectTrigger id={`${base}-kind`} className={`${control} w-full`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent className={selectList}>
              {KINDS.map(([value, label]) => (
                <SelectItem key={value} value={value}>
                  {label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor={`${base}-action`}>Action</Label>
          <Select
            name={`${base}-action`}
            value={row.action}
            onValueChange={(action) => {
              set({ action: action as Action });
            }}
          >
            <SelectTrigger id={`${base}-action`} className={`${control} w-full`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent className={selectList}>
              {ACTIONS.map(([value, label]) => (
                <SelectItem key={value} value={value}>
                  {label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor={`${base}-directions`}>Applies to</Label>
          <Select
            name={`${base}-directions`}
            value={row.directions}
            onValueChange={(directions) => {
              set({ directions: directions as Directions });
            }}
          >
            <SelectTrigger id={`${base}-directions`} className={`${control} w-full`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent className={selectList}>
              {DIRECTIONS.map(([value, label]) => (
                <SelectItem key={value} value={value}>
                  {label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {row.kind === "keywords" ? (
        <div className="flex flex-col gap-2">
          <Label htmlFor={`${base}-words`}>Words</Label>
          <Textarea
            id={`${base}-words`}
            autoComplete="off"
            spellCheck={false}
            className="min-h-20 font-mono"
            value={row.words}
            {...described}
            onChange={(event) => {
              set({ words: event.target.value });
            }}
          />
          <p className="text-sm text-muted-foreground">{WORDS_HINT}</p>
          <Label htmlFor={`${base}-whole`} className={`${control} gap-2`}>
            <Switch
              id={`${base}-whole`}
              checked={row.wholeWord}
              onCheckedChange={(wholeWord) => {
                set({ wholeWord });
              }}
            />
            Whole words only
          </Label>
        </div>
      ) : null}

      {row.kind === "regex" ? (
        <div className="flex flex-col gap-2">
          <Label htmlFor={`${base}-regex`}>Regular expression</Label>
          <Input
            id={`${base}-regex`}
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            className={`${control} font-mono`}
            value={row.regex}
            {...described}
            onChange={(event) => {
              set({ regex: event.target.value });
            }}
          />
          <p className="text-sm text-muted-foreground">{REGEX_HINT}</p>
        </div>
      ) : null}

      {row.kind === "pii" ? (
        <div role="group" aria-label="Types" className="flex flex-col" {...described}>
          {PII_TYPES.map((one) => {
            const on = row.pii.includes(one.type);
            return (
              <Label key={one.type} htmlFor={`${base}-${one.type}`} className="min-h-11 items-start gap-2 py-2">
                <Checkbox
                  id={`${base}-${one.type}`}
                  checked={on}
                  onCheckedChange={(checked) => {
                    const without = row.pii.filter((type) => type !== one.type);
                    const next: PiiType[] =
                      checked === true
                        ? PII_TYPES.map((t) => t.type).filter(
                            (type) => type === one.type || without.includes(type),
                          )
                        : without;
                    set({ pii: next });
                  }}
                />
                <span className="flex min-w-0 flex-col">
                  <span>{one.label}</span>
                  <span className="text-sm font-normal text-muted-foreground">{one.hint}</span>
                </span>
              </Label>
            );
          })}
        </div>
      ) : null}

      <RowError id={errorId} message={error} />
      <div>
        <Button type="button" variant="outline" className={control} onClick={onRemove}>
          {`Remove rule ${String(index + 1)}`}
        </Button>
      </div>
    </div>
  );
}

interface RulesProps {
  rows: readonly RuleRow[];
  /** The fault of each row, by index: from the form, then from the gateway. */
  errors: Readonly<Record<number, string>>;
  onChange: (rows: RuleRow[]) => void;
}

/** The rules of a guardrail: one block for each, and a button to add one. */
export function Rules({ rows, errors, onChange }: RulesProps) {
  return (
    <div className="flex flex-col gap-3">
      {rows.map((row, index) => (
        <Rule
          key={row.key}
          row={row}
          index={index}
          error={errors[index]}
          onChange={(next) => {
            onChange(rows.map((one, at) => (at === index ? next : one)));
          }}
          onRemove={() => {
            onChange(rows.filter((_, at) => at !== index));
          }}
        />
      ))}
      <div>
        <Button
          type="button"
          variant="outline"
          className={control}
          onClick={() => {
            onChange([...rows, newRule()]);
          }}
        >
          Add rule
        </Button>
      </div>
    </div>
  );
}
