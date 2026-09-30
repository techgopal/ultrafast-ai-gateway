import { Field } from "@/components/Field";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { EXPIRY_CHOICES, today, type Expiry } from "@/lib/expiry";

interface ExpiryFieldProps {
  /** The name of the field in the form, which is the name the API has for it. */
  name: string;
  value: Expiry;
  onChange: (value: Expiry) => void;
  onBlur: () => void;
  /** What is wrong with the expiry. */
  error: string | undefined;
  /** Says when on its day the thing expires. */
  hint: string;
}

/**
 * When a key or an access token expires: never, in some days, or on a date.
 * The choices are one group, named by the label of the field; the day has a
 * name of its own, and is described as the group is. `expiryOf` of
 * `lib/expiry` makes of the value what the gateway takes.
 */
export function ExpiryField({ name, value, onChange, onBlur, error, hint }: ExpiryFieldProps) {
  return (
    <Field group label="Expires" name={name} hint={hint} error={error}>
      {({ id, name: fieldName, "aria-labelledby": labelledBy, ...described }) => (
        <div className="flex flex-col gap-2">
          <RadioGroup
            {...described}
            id={id}
            name={fieldName}
            aria-labelledby={labelledBy}
            value={value.choice}
            onValueChange={(choice) => {
              onChange({ ...value, choice });
            }}
          >
            {EXPIRY_CHOICES.map(([choice, label]) => (
              <div key={choice} className="flex min-h-11 items-center gap-2 md:min-h-8">
                <RadioGroupItem id={`${id}-${choice}`} value={choice} />
                <Label htmlFor={`${id}-${choice}`}>{label}</Label>
              </div>
            ))}
          </RadioGroup>
          {value.choice === "date" ? (
            <Input
              {...described}
              type="date"
              aria-label="Expiry date"
              min={today()}
              className="min-h-11 md:min-h-8"
              value={value.day}
              onBlur={onBlur}
              onChange={(event) => {
                onChange({ ...value, day: event.target.value });
              }}
            />
          ) : null}
        </div>
      )}
    </Field>
  );
}
