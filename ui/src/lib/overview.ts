// What the overview counts. Everything here is counted from what the list
// calls of the API returned to the viewer: no number is made up.

/** The statuses of a key that the gateway knows, in the order they are shown. */
export const KEY_STATUSES: readonly string[] = ["active", "suspended", "expired", "revoked"];
/** The statuses of a user that the gateway knows, in the order they are shown. */
export const USER_STATUSES: readonly string[] = ["active", "invited", "disabled"];

export interface StatusCount {
  status: string;
  count: number;
}

/**
 * How many of the things have each status. The statuses of `known` come
 * first, in their order; a status the console does not know follows under its
 * own text, by the alphabet. A status that nothing has is left out.
 */
export function countByStatus(
  things: readonly { status: string }[],
  known: readonly string[],
): StatusCount[] {
  const counts = new Map<string, number>();
  for (const { status } of things) counts.set(status, (counts.get(status) ?? 0) + 1);
  const others = [...counts.keys()].filter((status) => !known.includes(status)).sort();
  return [...known, ...others].flatMap((status) => {
    const count = counts.get(status);
    return count === undefined ? [] : [{ status, count }];
  });
}

/**
 * What the counts of users and of teams are called. The gateway lists to an
 * admin every user and every team, and to the lead of a team the people of
 * the teams they lead and the teams they are in: the same two lists, which
 * are not the same thing. `all` says that the viewer is listed all of them;
 * otherwise the title says whose they are, so that nobody reads a number of
 * their own teams as the number of the gateway.
 */
export function countTitles(all: boolean): { users: string; teams: string } {
  return all
    ? { users: "Users", teams: "Teams" }
    : { users: "Users in your teams", teams: "Your teams" };
}

/** How many of the providers have a credential. */
export function withCredential(providers: readonly { has_credential: boolean }[]): number {
  return providers.filter((provider) => provider.has_credential).length;
}

/** Which of the first steps are done, of those the console can see. */
export interface FirstSteps {
  /** There is a provider. */
  provider: boolean;
  /** There is a virtual key among those the viewer sees. */
  key: boolean;
}

/**
 * What there is to get started with, from the number of providers and of
 * keys; `null` when both steps are done. Whether a call was ever made is not
 * among them: the API does not tell.
 */
export function firstSteps(providers: number, keys: number): FirstSteps | null {
  const steps = { provider: providers > 0, key: keys > 0 };
  return steps.provider && steps.key ? null : steps;
}

/**
 * A first call to the gateway at `origin`, as a command for a shell. The key
 * and the model are placeholders: the console never writes a key into it.
 */
export function exampleCall(origin: string): string {
  return [
    `curl ${origin}/v1/chat/completions \\`,
    '  -H "Authorization: Bearer <key>" \\',
    '  -H "Content-Type: application/json" \\',
    `  -d '{"model": "<provider>/<model>", "messages": [{"role": "user", "content": "Hello"}]}'`,
  ].join("\n");
}
