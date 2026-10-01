import { Outlet, useMatches } from "@tanstack/react-router";
import { MenuIcon } from "lucide-react";
import { useEffect, useRef } from "react";
import { AppSidebar } from "@/components/AppSidebar";
import { InShell, type ShellUser } from "@/components/shell-context";
import { Button } from "@/components/ui/button";
import {
  SidebarProvider,
  SidebarTrigger,
  useSidebar,
} from "@/components/ui/sidebar";

interface ShellProps {
  user: ShellUser | null;
  onSignOut: () => void;
}

function usePageTitle(): string {
  const titles = useMatches({
    select: (matches) => matches.map((match) => match.staticData.title),
  });
  return titles.findLast((title) => title !== undefined) ?? "";
}

/**
 * On narrow screens: the menu button and the page title.
 * On wide screens: the control that collapses and reopens the sidebar.
 */
function TopBar() {
  const { isMobile, openMobile, setOpenMobile } = useSidebar();
  const title = usePageTitle();
  const menuButton = useRef<HTMLButtonElement>(null);
  const wasOpen = useRef(false);

  // The drawer has no trigger of its own to return focus to, so it is done here.
  useEffect(() => {
    if (wasOpen.current && !openMobile) menuButton.current?.focus();
    wasOpen.current = openMobile;
  }, [openMobile]);

  if (!isMobile) {
    return (
      <header className="sticky top-0 z-10 flex h-12 items-center border-b bg-background px-2">
        <SidebarTrigger aria-label="Toggle sidebar" />
      </header>
    );
  }
  return (
    <header className="sticky top-0 z-10 flex h-14 items-center gap-2 border-b bg-background px-2">
      <Button
        ref={menuButton}
        type="button"
        variant="ghost"
        size="icon"
        className="size-11"
        aria-label="Open menu"
        onClick={() => {
          setOpenMobile(true);
        }}
      >
        <MenuIcon aria-hidden="true" />
      </Button>
      <span className="truncate font-medium">{title}</span>
    </header>
  );
}

export function Shell({ user, onSignOut }: ShellProps) {
  return (
    <SidebarProvider>
      <AppSidebar user={user} onSignOut={onSignOut} />
      <div className="flex min-w-0 flex-1 flex-col bg-background">
        <TopBar />
        <main className="flex min-w-0 flex-1 flex-col gap-6 p-4 md:p-6">
          <InShell value>
            <Outlet />
          </InShell>
        </main>
      </div>
    </SidebarProvider>
  );
}
