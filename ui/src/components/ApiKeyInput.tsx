import { EyeIcon, EyeOffIcon } from "lucide-react";
import { useState } from "react";
import { control } from "@/components/classes";
import type { FieldWiring } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

interface ApiKeyInputProps {
  wiring: FieldWiring;
  value: string;
  onChange: (value: string) => void;
  onBlur: () => void;
}

/**
 * The field of an API key: what is typed is hidden unless the user asks to
 * see it, and the browser is asked not to remember it. The key is held by the
 * form, and by nothing else: it goes when the request succeeded, and with the
 * form when the dialog closes. After a request that was refused it is still
 * in its field, so that what is sent next is what the form shows.
 */
export function ApiKeyInput({ wiring, value, onChange, onBlur }: ApiKeyInputProps) {
  const [shown, setShown] = useState(false);
  return (
    <div className="flex gap-2">
      <Input
        {...wiring}
        type={shown ? "text" : "password"}
        autoComplete="off"
        spellCheck={false}
        className={control}
        value={value}
        onBlur={onBlur}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
      <Button
        type="button"
        variant={shown ? "secondary" : "outline"}
        className={control}
        // One name, and whether it is pressed. A name that changed with it would say it twice.
        aria-label="Show API key"
        aria-pressed={shown}
        onClick={() => {
          setShown((now) => !now);
        }}
      >
        {/*
          The text is the same either way, and the fill of a pressed button
          differs too little from the dialog to be seen by everyone: the icon
          shows whether it is pressed. An open eye: the key is shown.
        */}
        {shown ? (
          <EyeIcon data-icon="inline-start" aria-hidden="true" />
        ) : (
          <EyeOffIcon data-icon="inline-start" aria-hidden="true" />
        )}
        Show
      </Button>
    </div>
  );
}
