import { useMemo } from "react";
import { useKeys, useTeams, useUsers } from "@/api/queries";
import { control, cutLongChoice, selectList } from "@/components/classes";
import { ErrorState } from "@/components/ErrorState";
import { Field } from "@/components/Field";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { chosenTarget, SCOPES, scopeText, type OfferedTargets } from "@/lib/limits";

const selectTrigger = `${control} w-full ${cutLongChoice}`;

interface Choice {
  id: number;
  text: string;
}

interface Targets {
  /** The ids that can be chosen, for the request. Empty while a list is on its way. */
  offered: OfferedTargets;
  /** What can be chosen for the scope: `null` while it is on its way or failed. */
  choices: (scope: string) => readonly Choice[] | null;
  error: (scope: string) => unknown;
  retry: (scope: string) => void;
}

/**
 * The teams, users and keys a limit or a budget can be about: an admin reads
 * all three. Revoked keys are not offered: they cannot be called.
 */
export function useTargets(): Targets {
  const teams = useTeams();
  const users = useUsers();
  const keys = useKeys();
  const lists = useMemo(
    () => ({
      team: teams.data?.teams.map((one): Choice => ({ id: one.id, text: one.name })) ?? null,
      user: users.data?.users.map((one): Choice => ({ id: one.id, text: one.email })) ?? null,
      key:
        keys.data?.keys
          .filter((one) => one.status !== "revoked")
          .map((one): Choice => ({ id: one.id, text: one.name })) ?? null,
    }),
    [teams.data, users.data, keys.data],
  );
  const ids = (list: readonly Choice[] | null) => (list ?? []).map((one) => one.id);
  const queries = { team: teams, user: users, key: keys };
  const of = (scope: string) => (scope === "team" || scope === "user" || scope === "key" ? scope : null);
  return {
    offered: { team: ids(lists.team), user: ids(lists.user), key: ids(lists.key) },
    choices: (scope) => {
      const one = of(scope);
      return one === null ? null : lists[one];
    },
    error: (scope) => {
      const one = of(scope);
      return one === null ? null : queries[one].error;
    },
    retry: (scope) => {
      const one = of(scope);
      if (one !== null) void queries[one].refetch();
    },
  };
}

/** The scope of a limit or a budget: the gateway, a team, a user or a key. */
export function ScopeField({
  value,
  error,
  onChange,
}: {
  value: string;
  error: string | undefined;
  onChange: (value: string) => void;
}) {
  return (
    <Field label="Scope" name="scope" error={error}>
      {({ id, name, ...described }) => (
        <Select name={name} value={value} onValueChange={onChange}>
          <SelectTrigger id={id} {...described} className={selectTrigger}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent className={selectList}>
            {SCOPES.map(([scope, label]) => (
              <SelectItem key={scope} value={scope}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </Field>
  );
}

/**
 * The team, user or key the scope asks for. While the list is on its way a
 * skeleton; when it could not be read the error with Retry. A choice that is
 * not offered any more shows as none.
 */
export function TargetField({
  scope,
  value,
  targets,
  error,
  onChange,
}: {
  scope: string;
  value: string;
  targets: Targets;
  error: string | undefined;
  onChange: (value: string) => void;
}) {
  if (scope === "gateway") return null;
  const label = scopeText(scope);
  const choices = targets.choices(scope);
  return (
    <Field label={label} name="scope_id" error={error}>
      {({ id, name, ...described }) => {
        if (choices === null) {
          const failure = targets.error(scope);
          if (failure !== null) {
            return (
              <ErrorState
                error={failure}
                onRetry={() => {
                  targets.retry(scope);
                }}
              />
            );
          }
          return (
            <div
              role="status"
              aria-busy="true"
              aria-label={`Loading the ${label.toLowerCase()}s`}
              className="flex flex-col gap-2"
            >
              <Skeleton className="h-8 w-full" />
            </div>
          );
        }
        return (
          <Select
            name={name}
            value={chosenTarget(
              value,
              choices.map((one) => one.id),
            )}
            onValueChange={onChange}
          >
            <SelectTrigger id={id} {...described} className={selectTrigger}>
              <SelectValue placeholder={`Choose a ${label.toLowerCase()}`} />
            </SelectTrigger>
            <SelectContent className={selectList}>
              {choices.map((one) => (
                <SelectItem key={one.id} value={String(one.id)}>
                  {one.text}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        );
      }}
    </Field>
  );
}
