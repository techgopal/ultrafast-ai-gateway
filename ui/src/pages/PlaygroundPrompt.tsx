// The prompt template of a playground call: which template, which version, and
// the value of each variable. A choice that is no longer offered (a template
// that was deleted, a version that is not there) is never sent.
import { useMemo, useState } from "react";
import { usePrompt, usePromptVersion, usePrompts } from "@/api/queries";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { FilterSelect, type Choice } from "@/components/FilterSelect";
import { Textarea } from "@/components/ui/textarea";
import type { PromptBody } from "@/lib/playground";

export const NO_TEMPLATE = "none";
export const LATEST = "latest";
export const UNREADABLE_TEMPLATE = "The variables of this template could not be read.";

export interface PromptPicker {
  /** The picker is shown only when there are templates to choose from. */
  offered: Choice[];
  name: string;
  setName: (name: string) => void;
  versions: Choice[];
  version: string;
  setVersion: (version: string) => void;
  variables: readonly string[];
  values: Readonly<Record<string, string>>;
  setValue: (name: string, value: string) => void;
  /** A template is chosen. */
  chosen: boolean;
  /** What to send; `null` without a template, and while the version is being read. */
  body: PromptBody | null;
  /** Nothing is chosen, or the template is read: a call can be made. */
  ready: boolean;
  unreadable: boolean;
  /** The model the chosen version names. */
  templateModel: string | null;
}

/** The state of the picker, from where the page was opened (`Open in Playground`). */
export function usePromptPicker(initial: { name: string; version: number | null } | undefined): PromptPicker {
  const list = usePrompts();
  const [name, setName] = useState(initial?.name ?? NO_TEMPLATE);
  const [version, setVersion] = useState(initial?.version == null ? LATEST : String(initial.version));
  const [typed, setTyped] = useState<Readonly<Record<string, string>>>({});

  const templates = useMemo(
    () => (list.data?.prompts ?? []).filter((one) => !one.unreadable).sort((a, b) => a.name.localeCompare(b.name)),
    [list.data],
  );
  const template = templates.find((one) => one.name === name) ?? null;
  const view = usePrompt(template?.id ?? 0, template !== null);
  const numbers = view.data?.versions.map((one) => one.version) ?? [];
  const latest = template?.latest_version ?? 0;
  const explicit = version !== LATEST && numbers.includes(Number(version)) ? Number(version) : null;
  const shown = explicit ?? latest;
  const read = usePromptVersion(template?.id ?? 0, shown, template !== null && shown > 0);

  const variables = read.data?.variables ?? [];
  const values = Object.fromEntries(variables.map((one) => [one, typed[one] ?? ""]));
  const chosen = template !== null;
  const body: PromptBody | null =
    template === null || read.data === undefined
      ? null
      : { id: template.name, ...(explicit === null ? {} : { version: explicit }), variables: values };

  return {
    offered: [
      { value: NO_TEMPLATE, label: "No template" },
      ...templates.map((one) => ({ value: one.name, label: one.name })),
    ],
    name: template?.name ?? NO_TEMPLATE,
    setName,
    versions: [
      { value: LATEST, label: `Latest (version ${String(latest)})` },
      ...[...numbers].sort((a, b) => b - a).map((n) => ({ value: String(n), label: `Version ${String(n)}` })),
    ],
    version: explicit === null ? LATEST : String(explicit),
    setVersion,
    variables,
    values,
    setValue: (variable, value) => {
      setTyped((before) => ({ ...before, [variable]: value }));
    },
    chosen,
    body,
    ready: !chosen || body !== null,
    unreadable: chosen && read.data === undefined && read.error !== null,
    templateModel: read.data?.model ?? null,
  };
}

/** The controls of the picker; nothing at all when there are no templates. */
export function PromptPickerFields({ picker }: { picker: PromptPicker }) {
  if (picker.offered.length <= 1) return null;
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <p className="text-sm font-medium">Prompt template</p>
        <FilterSelect
          label="Prompt template"
          value={picker.name}
          choices={picker.offered}
          onChange={picker.setName}
        />
        <p className="text-sm text-muted-foreground">
          The messages of the template come first, then the system prompt and your message.
        </p>
      </div>
      {picker.chosen ? (
        <>
          <div className="flex flex-col gap-2">
            <p className="text-sm font-medium">Template version</p>
            <FilterSelect
              label="Template version"
              value={picker.version}
              choices={picker.versions}
              onChange={picker.setVersion}
            />
          </div>
          {picker.unreadable ? (
            <p role="alert" className="text-sm text-destructive">
              {UNREADABLE_TEMPLATE}
            </p>
          ) : null}
          {picker.variables.map((variable) => (
            <Field key={variable} label={`Variable: ${variable}`} name={`variable-${variable}`}>
              <Textarea
                autoComplete="off"
                className={`${control} min-h-16`}
                spellCheck={false}
                value={picker.values[variable] ?? ""}
                onChange={(event) => {
                  picker.setValue(variable, event.target.value);
                }}
              />
            </Field>
          ))}
        </>
      ) : null}
    </div>
  );
}
