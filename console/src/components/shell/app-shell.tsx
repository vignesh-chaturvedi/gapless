import { Outlet } from "react-router";

import { IncidentDrawer } from "@/components/incident-drawer";
import { CommandMenu } from "@/components/shell/command-menu";
import { StatusBar } from "@/components/shell/status-bar";
import { TopBar } from "@/components/shell/top-bar";

export function AppShell() {
  return (
    <div className="flex min-h-dvh flex-col">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50 focus:rounded-md focus:bg-primary focus:px-3 focus:py-2 focus:text-primary-foreground"
      >
        Skip to content
      </a>
      <TopBar />
      <main id="main" className="flex-1">
        <Outlet />
      </main>
      <StatusBar />
      <CommandMenu />
      <IncidentDrawer />
    </div>
  );
}
