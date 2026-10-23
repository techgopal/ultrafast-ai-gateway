import { useState } from "react";
import { control } from "@/components/classes";
import { Input } from "@/components/ui/input";

/** A text applied when it is entered or left, not at every key. */
export function TextFilter({
  value,
  label,
  placeholder,
  onApply,
}: {
  value: string;
  label: string;
  placeholder: string;
  onApply: (text: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const apply = () => {
    const text = draft.trim();
    if (text !== value) onApply(text);
  };
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        apply();
      }}
    >
      <Input
        type="text"
        aria-label={label}
        placeholder={placeholder}
        autoComplete="off"
        className={`${control} w-full sm:w-48`}
        value={draft}
        onChange={(event) => {
          setDraft(event.target.value);
        }}
        onBlur={apply}
      />
    </form>
  );
}
