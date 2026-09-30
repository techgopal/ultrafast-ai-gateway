import { Link } from "@tanstack/react-router";
import { ArrowRightIcon } from "lucide-react";
import { useId, type ReactNode } from "react";
import { ApiError } from "@/api/errors";
import { useKeys, useProviders, useTeams, useUsers } from "@/api/queries";
import { can, type Me } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { ErrorState } from "@/components/ErrorState";
import { NotAvailableNote } from "@/components/NotAvailableNote";
import { PageHeader } from "@/components/PageHeader";
import { StatusBadge } from "@/components/StatusBadge";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
  countByStatus,
  exampleCall,
  firstSteps,
  KEY_STATUSES,
  USER_STATUSES,
  withCredential,
  type FirstSteps,
  type StatusCount,
} from "@/lib/overview";

export const LATER_NOTE = "Usage, spend and request logs arrive with a later release.";
export const CALLS_NOT_TRACKED =
  "The console cannot tell yet whether a call was made, so this step is never marked done.";
export const ONLY_AN_ADMIN = "Only an admin can add a provider.";

/** A link of the page: high enough to touch on a narrow screen. */
const link =
  "inline-flex min-h-11 items-center rounded-sm underline-offset-4 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50 md:min-h-0";

/** A list call, as far as a tile needs it. */
interface Read<T> {
  data: T | undefined;
  error: unknown;
  refetch: () => unknown;
}

interface TileProps<T> {
  title: string;
  to: "/providers" | "/keys" | "/users" | "/teams";
  read: Read<T>;
  /** What the tile says of the list when it is read. */
  children: (list: T) => ReactNode;
}

/**
 * One list of the API, counted. The tile stands for itself: while its list
 * is on its way it shows a skeleton, and a list that cannot be read is said
 * in the tile, with Retry, while the rest of the page stays.
 */
function Tile<T>({ title, to, read, children }: TileProps<T>) {
  const id = useId();
  const { data, error } = read;
  let content: ReactNode;
  if (data !== undefined) {
    content = children(data);
  } else if (error === null) {
    content = (
      <>
        <Skeleton className="h-9 w-16" />
        <Skeleton className="h-4 w-full" />
      </>
    );
  } else if (error instanceof ApiError && error.status === 403) {
    content = <NotAvailableNote />;
  } else {
    content = (
      <ErrorState
        error={error}
        onRetry={() => {
          void read.refetch();
        }}
      />
    );
  }
  return (
    <Card role="group" aria-labelledby={id} aria-busy={data === undefined && error === null}>
      <CardHeader>
        <CardTitle>
          <h2 id={id}>
            <Link to={to} className={`${link} gap-1.5`}>
              {title}
              <ArrowRightIcon aria-hidden="true" className="size-4 text-muted-foreground" />
            </Link>
          </h2>
        </CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">{content}</CardContent>
    </Card>
  );
}

/** How many there are. */
function Count({ of }: { of: number }) {
  return <p className="text-3xl font-semibold tabular-nums">{of}</p>;
}

/** How many there are of each status. The pill says the status. */
function ByStatus({ counts }: { counts: readonly StatusCount[] }) {
  if (counts.length === 0) return null;
  return (
    <ul aria-label="By status" className="flex flex-wrap gap-x-4 gap-y-2 text-sm">
      {counts.map(({ status, count }) => (
        <li key={status} className="flex items-center gap-1.5">
          <span className="font-medium tabular-nums">{count}</span> <StatusBadge status={status} />
        </li>
      ))}
    </ul>
  );
}

// Users and teams are asked for only by who sees their tiles.

function UsersTile() {
  const users = useUsers();
  return (
    <Tile title="Users" to="/users" read={users}>
      {({ users: list }) => (
        <>
          <Count of={list.length} />
          <ByStatus counts={countByStatus(list, USER_STATUSES)} />
        </>
      )}
    </Tile>
  );
}

function TeamsTile() {
  const teams = useTeams();
  return (
    <Tile title="Teams" to="/teams" read={teams}>
      {({ teams: list }) => <Count of={list.length} />}
    </Tile>
  );
}

function StepState({ done }: { done: boolean }) {
  return <Badge variant={done ? "default" : "outline"}>{done ? "Done" : "To do"}</Badge>;
}

const stepLink = `${link} font-medium underline`;
const stepText = "text-sm text-muted-foreground";

interface GetStartedProps {
  steps: FirstSteps;
  /** Whether the viewer may add a provider themselves. */
  mayAddProvider: boolean;
}

/** The way to a first call through the gateway, for a gateway that is new. */
function GetStarted({ steps, mayAddProvider }: GetStartedProps) {
  const id = useId();
  return (
    <section aria-labelledby={id}>
      <Card>
        <CardHeader>
          <CardTitle>
            <h2 id={id}>Get started</h2>
          </CardTitle>
          <CardDescription>Three steps to the first call through this gateway.</CardDescription>
        </CardHeader>
        <CardContent>
          <ol className="flex list-decimal flex-col gap-4 pl-5">
            <li>
              <div className="flex flex-wrap items-center gap-x-3">
                <Link to="/providers" className={stepLink}>
                  Add a provider
                </Link>
                <StepState done={steps.provider} />
              </div>
              <p className={stepText}>The gateway sends calls to the providers it knows.</p>
              {mayAddProvider ? null : <p className={stepText}>{ONLY_AN_ADMIN}</p>}
            </li>
            <li>
              <div className="flex flex-wrap items-center gap-x-3">
                <Link to="/keys" className={stepLink}>
                  Create a virtual key
                </Link>
                <StepState done={steps.key} />
              </div>
              <p className={stepText}>An app sends its virtual key with every call.</p>
            </li>
            <li className="min-w-0">
              <div className="flex min-h-11 items-center font-medium md:min-h-0">
                Make a first call
              </div>
              <p className={stepText}>
                Put the key and a model of the provider in place of the placeholders.
              </p>
              <div role="group" aria-label="Example call" className="my-2 min-w-0">
                {/* A long line wraps, at a blank where there is one: nothing is cut off or scrolls. */}
                <pre className="min-w-0 rounded-md bg-muted p-3 font-mono text-xs wrap-anywhere whitespace-pre-wrap">
                  <code>{exampleCall(window.location.origin)}</code>
                </pre>
              </div>
              <p className={stepText}>{CALLS_NOT_TRACKED}</p>
            </li>
          </ol>
        </CardContent>
      </Card>
    </section>
  );
}

function OverviewOf({ me }: { me: Me }) {
  const providers = useProviders();
  const keys = useKeys();
  // Known only when both lists are read: until then nothing is said.
  const steps =
    providers.data === undefined || keys.data === undefined
      ? null
      : firstSteps(providers.data.providers.length, keys.data.keys.length);

  return (
    <>
      <PageHeader title="Overview" />
      {steps === null ? null : (
        <GetStarted steps={steps} mayAddProvider={can(me, { type: "manageProviders" })} />
      )}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <Tile title="Providers" to="/providers" read={providers}>
          {({ providers: list }) => (
            <>
              <Count of={list.length} />
              {list.length === 0 ? null : (
                <p className="text-sm text-muted-foreground">
                  {withCredential(list)} with a credential
                </p>
              )}
            </>
          )}
        </Tile>
        <Tile title="Virtual keys" to="/keys" read={keys}>
          {({ keys: list }) => (
            <>
              <Count of={list.length} />
              <ByStatus counts={countByStatus(list, KEY_STATUSES)} />
            </>
          )}
        </Tile>
        {can(me, { type: "viewUserAndTeamCounts" }) ? (
          <>
            <UsersTile />
            <TeamsTile />
          </>
        ) : null}
      </div>
      <p className="text-sm text-muted-foreground">{LATER_NOTE}</p>
    </>
  );
}

/** What there is in the gateway, as far as the viewer may see it. */
export function Overview() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  return <OverviewOf me={session.me} />;
}
