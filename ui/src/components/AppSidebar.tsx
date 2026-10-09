import { Link, useRouterState } from "@tanstack/react-router";
import { LogOutIcon } from "lucide-react";
import { control } from "@/components/classes";
import { ROLE_NAMES } from "@/components/RoleBadge";
import type { ShellUser } from "@/components/shell-context";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarSeparator,
  useSidebar,
} from "@/components/ui/sidebar";
import { ThemeSwitch } from "@/theme/theme";

type Path =
  | "/"
  | "/logs"
  | "/alerts"
  | "/playground"
  | "/providers"
  | "/models"
  | "/routes"
  | "/prompts"
  | "/keys"
  | "/users"
  | "/teams"
  | "/limits"
  | "/guardrails"
  | "/settings"
  | "/account";

interface NavItem {
  label: string;
  /** No path: the page is coming, and the item is plain text. */
  to?: Path;
  /** Shown only to who may change the settings. */
  settings?: boolean;
  /** Shown only to who may manage alerts. */
  alerts?: boolean;
  /** Shown only to who may manage guardrails. */
  guardrails?: boolean;
}

interface NavSection {
  label: string;
  items: NavItem[];
}

const sections: NavSection[] = [
  {
    label: "Observe",
    items: [
      { label: "Overview", to: "/" },
      { label: "Logs", to: "/logs" },
      { label: "Alerts", to: "/alerts", alerts: true },
      { label: "Playground", to: "/playground" },
    ],
  },
  {
    label: "Configure",
    items: [
      { label: "Providers", to: "/providers" },
      { label: "Models", to: "/models" },
      { label: "Routing", to: "/routes" },
      { label: "Prompts", to: "/prompts" },
      { label: "Virtual keys", to: "/keys" },
    ],
  },
  {
    label: "Govern",
    items: [
      { label: "Users", to: "/users" },
      { label: "Teams", to: "/teams" },
      { label: "Budgets and limits", to: "/limits" },
      { label: "Guardrails", to: "/guardrails", guardrails: true },
      { label: "MCP tools" },
    ],
  },
];

const footerItems: NavItem[] = [
  { label: "Settings", to: "/settings", settings: true },
  { label: "Account", to: "/account" },
];

function isActive(pathname: string, to: Path): boolean {
  if (to === "/") return pathname === "/";
  return pathname === to || pathname.startsWith(`${to}/`);
}

function NavEntry({ item, pathname }: { item: NavItem; pathname: string }) {
  const { setOpenMobile } = useSidebar();
  if (item.to === undefined) {
    return (
      <SidebarMenuItem>
        <span
          aria-disabled="true"
          data-testid="nav-item"
          data-label={item.label}
          className={`flex w-full items-center justify-between gap-2 rounded-md p-2 text-sm text-muted-foreground ${control}`}
        >
          {item.label}
          <Badge variant="outline">Coming</Badge>
        </span>
      </SidebarMenuItem>
    );
  }
  const active = isActive(pathname, item.to);
  return (
    <SidebarMenuItem>
      <SidebarMenuButton asChild isActive={active} className={control}>
        <Link
          to={item.to}
          activeOptions={{ exact: item.to === "/" }}
          data-testid="nav-item"
          data-label={item.label}
          onClick={() => {
            setOpenMobile(false);
          }}
        >
          {item.label}
        </Link>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}

interface AppSidebarProps {
  user: ShellUser | null;
  onSignOut: () => void;
}

export function AppSidebar({ user, onSignOut }: AppSidebarProps) {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  return (
    <Sidebar>
      <nav aria-label="Main" className="flex min-h-0 flex-1 flex-col">
        <SidebarContent>
          {sections.map((section) => (
            <SidebarGroup key={section.label}>
              <SidebarGroupLabel asChild>
                <h2 data-testid="nav-section">{section.label}</h2>
              </SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu>
                  {section.items
                    .filter((item) => item.alerts !== true || user?.mayManageAlerts === true)
                    .filter(
                      (item) => item.guardrails !== true || user?.mayManageGuardrails === true,
                    )
                    .map((item) => (
                      <NavEntry key={item.label} item={item} pathname={pathname} />
                    ))}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          ))}
        </SidebarContent>
        <SidebarSeparator className="mx-0" />
        <SidebarFooter>
          <SidebarMenu>
            {footerItems
              .filter((item) => item.settings !== true || user?.maySetSettings === true)
              .map((item) => (
                <NavEntry key={item.label} item={item} pathname={pathname} />
              ))}
          </SidebarMenu>
          <ThemeSwitch />
          {user === null ? null : (
            <div className="flex items-center justify-between gap-2 p-2">
              <div className="min-w-0 text-sm">
                <div className="truncate font-medium">{user.name}</div>
                <div className="text-muted-foreground">{ROLE_NAMES[user.role] ?? user.role}</div>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className={control}
                onClick={onSignOut}
              >
                <LogOutIcon aria-hidden="true" />
                Sign out
              </Button>
            </div>
          )}
        </SidebarFooter>
      </nav>
    </Sidebar>
  );
}
