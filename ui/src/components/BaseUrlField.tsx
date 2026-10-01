import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { Input } from "@/components/ui/input";

export const V1_HINT = "The base URL of an OpenAI-compatible provider usually ends in /v1.";

interface BaseUrlFieldProps {
  /** The name of the field in the form, which is the name the API has for it. */
  name: string;
  /** The kind of the provider. An OpenAI-compatible one has the hint about `/v1`. */
  kind: string;
  value: string;
  onChange: (value: string) => void;
  onBlur: () => void;
  /** What is wrong with the address. */
  error: string | undefined;
}

/**
 * The base URL of a provider, in the form that adds one and in the form that
 * edits one. The gateway says what is wrong with an address; the console
 * asks nothing of it.
 */
export function BaseUrlField({ name, kind, value, onChange, onBlur, error }: BaseUrlFieldProps) {
  return (
    <Field
      label="Base URL"
      name={name}
      required
      hint={kind === "openai" ? V1_HINT : undefined}
      error={error}
    >
      <Input
        inputMode="url"
        autoComplete="off"
        autoCapitalize="none"
        spellCheck={false}
        className={control}
        value={value}
        onBlur={onBlur}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
    </Field>
  );
}
