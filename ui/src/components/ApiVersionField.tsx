import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { Input } from "@/components/ui/input";

interface ApiVersionFieldProps {
  /** The name of the field in the form, which is the name the API has for it. */
  name: string;
  value: string;
  onChange: (value: string) => void;
  onBlur: () => void;
  /** What is wrong with the version. */
  error: string | undefined;
}

/**
 * The API version of an Azure OpenAI provider, in the form that adds one and
 * in the form that edits one. The gateway says what is wrong with it.
 */
export function ApiVersionField({ name, value, onChange, onBlur, error }: ApiVersionFieldProps) {
  return (
    <Field label="API version" name={name} error={error}>
      <Input
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
