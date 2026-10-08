import { Link, useRouterState } from "@tanstack/react-router";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control } from "@/components/classes";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { PageHeader } from "@/components/PageHeader";
import { AlertsChannels } from "@/pages/AlertsChannels";
import { AlertsHistory } from "@/pages/AlertsHistory";
import { AlertsRules } from "@/pages/AlertsRules";

type View = "rules" | "channels" | "history";

const link =
  "inline-flex items-center rounded-md px-3 text-sm font-medium text-muted-foreground hover:text-foreground aria-[current=page]:bg-muted aria-[current=page]:text-foreground " +
  control;

/** The three views of the page, as the settings page has its two: the address names the view. */
function ViewNav({ view }: { view: View }) {
  const current = (one: View) => (view === one ? { "aria-current": "page" as const } : {});
  return (
    <nav aria-label="Alerts sections" className="flex flex-wrap gap-1">
      <Link
        to="/alerts"
        activeOptions={{ includeHash: true }}
        className={link}
        {...current("rules")}
      >
        Rules
      </Link>
      <Link
        to="/alerts"
        hash="channels"
        activeOptions={{ includeHash: true }}
        className={link}
        {...current("channels")}
      >
        Channels
      </Link>
      <Link
        to="/alerts"
        hash="history"
        activeOptions={{ includeHash: true }}
        className={link}
        {...current("history")}
      >
        History
      </Link>
    </nav>
  );
}

function AlertsOf() {
  const hash = useRouterState({ select: (state) => state.location.hash });
  const view: View = hash === "channels" || hash === "history" ? hash : "rules";
  return (
    <>
      <PageHeader
        title="Alerts"
        subtitle="Be told when a budget, an error rate or a circuit needs attention."
      />
      <ViewNav view={view} />
      {view === "rules" ? <AlertsRules /> : view === "channels" ? <AlertsChannels /> : <AlertsHistory />}
    </>
  );
}

/** Rules, channels and the history of alerts: only an admin sees and changes them. */
export function Alerts() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  if (!can(session.me, { type: "manageAlerts" })) return <NotAvailableContent />;
  return <AlertsOf />;
}
