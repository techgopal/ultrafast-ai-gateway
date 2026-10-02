import { useModels, useRoutes } from "@/api/queries";
import { Checks } from "@/components/CheckList";
import { control } from "@/components/classes";
import { Field } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { callableItems, type CallableItem } from "@/lib/keys";

export const ALL_MODELS = "All models I can use";
export const ONLY_THESE = "Only these";

/** What the form can offer to limit a key to: `null` while the lists are on their way. */
export interface Callable {
  items: readonly CallableItem[] | null;
  error: unknown;
  retry: () => void;
}

/**
 * The models and routes the viewer can use, read when the key is limited to
 * some (`enabled`): a key for all of them asks for nothing. The gateway gives
 * a member the models they may call and the routes they may use, and an admin
 * every one. A key of another owner is limited from the same lists: the
 * gateway checks the names.
 */
export function useCallable(enabled: boolean): Callable {
  const models = useModels(enabled);
  const routes = useRoutes(enabled);
  const items =
    models.data === undefined || routes.data === undefined
      ? null
      : callableItems(models.data.models, routes.data.routes);
  return {
    items,
    error: (models.data === undefined ? models.error : null) ?? routes.error,
    retry: () => {
      if (models.data === undefined) void models.refetch();
      if (routes.data === undefined) void routes.refetch();
    },
  };
}

interface AllowedFieldProps {
  mode: "all" | "some";
  onMode: (mode: "all" | "some") => void;
  chosen: readonly string[];
  onChosen: (names: string[]) => void;
  callable: Callable;
  /** What is wrong with the choice of names. */
  error: string | undefined;
}

/** "All models I can use", or a choice of the models and routes the key may call. */
export function AllowedField({ mode, onMode, chosen, onChosen, callable, error }: AllowedFieldProps) {
  return (
    <>
      <Field group label="Allowed models" name="allow">
        {({ id, name, ...described }) => (
          <RadioGroup
            {...described}
            id={id}
            name={name}
            value={mode}
            onValueChange={(next) => {
              onMode(next === "some" ? "some" : "all");
            }}
          >
            {(
              [
                ["all", ALL_MODELS],
                ["some", ONLY_THESE],
              ] as const
            ).map(([value, label]) => (
              <Label key={value} htmlFor={`${id}-${value}`} className={control}>
                <RadioGroupItem id={`${id}-${value}`} value={value} />
                {label}
              </Label>
            ))}
          </RadioGroup>
        )}
      </Field>
      {mode === "some" ? (
        <Field group label="Models and routes" name="allowed" error={error}>
          {(wiring) => (
            <Checks
              wiring={wiring}
              items={callable.items}
              error={callable.error}
              retry={callable.retry}
              loading="Loading the models and routes"
              none="There is no model or route you can use yet."
              checked={chosen}
              onChange={onChosen}
              label={(item) => (
                <>
                  <span className="font-mono break-all">{item.id}</span>
                  {item.route ? <Badge variant="outline">Route</Badge> : null}
                </>
              )}
            />
          )}
        </Field>
      ) : null}
    </>
  );
}
