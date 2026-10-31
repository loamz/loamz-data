import { Cpu, Database, LayoutDashboard, LockKeyhole, Settings as Gear, TerminalSquare, type LucideIcon } from "lucide-react";
import { Suspense, lazy } from "react";
import { Button, PageEmpty, Page, Tooltip, cn } from "@/design";
import * as api from "./api";
import { Link, ThemeButton, UserMenu } from "./components/RailParts";
import { useLoad, usePath } from "./hooks";
import { Mark } from "./Mark";
import { parseRoute, type Route } from "./route";
import { Catalog } from "./screens/Catalog";
import { Settings } from "./screens/Settings";
import { Workspace } from "./screens/Workspace";

// The code editor is the heaviest part of the app and only two screens use it, so they are loaded when first opened.
const SqlEditor = lazy(() => import("./screens/SqlEditor").then((m) => ({ default: m.SqlEditor })));
const Compute = lazy(() => import("./screens/Compute").then((m) => ({ default: m.Compute })));

/**
 * One workspace, as a website of its own. Everything in it belongs to this workspace, and nothing links back out: not to Home, not to the
 * account console. Signing out is the only way to leave (the switcher on a page jumps to another workspace's own site).
 */
export function WorkspaceShell({ ws }: { ws: string }) {
  const route = parseRoute(usePath(), "workspace", ws);
  const who = useLoad(api.me, "me");
  const me = who.data;
  const mine = me?.workspaces.find((w) => w.id === ws);

  const places: { to: Route; label: string; icon: LucideIcon; active: boolean; show: boolean }[] = [
    { to: { screen: "workspace", ws }, label: "Workspace overview", icon: LayoutDashboard, active: route.screen === "workspace", show: true },
    { to: { screen: "catalog", ws, id: null }, label: "Catalog", icon: Database, active: route.screen === "catalog", show: true },
    { to: { screen: "sql", ws }, label: "SQL editor", icon: TerminalSquare, active: route.screen === "sql", show: true },
    { to: { screen: "compute", ws }, label: "Compute", icon: Cpu, active: route.screen === "compute", show: true },
    { to: { screen: "settings", ws }, label: "Workspace settings", icon: Gear, active: route.screen === "settings", show: Boolean(mine?.admin) },
  ];

  return (
    <div className="flex h-full flex-col-reverse bg-background text-foreground md:flex-row">
      <nav aria-label="Main" className="flex shrink-0 items-center justify-around gap-1 border-t border-border bg-sidebar px-2 py-1.5 md:w-14 md:flex-col md:justify-start md:border-r md:border-t-0 md:px-0 md:py-3">
        <div className="hidden md:mb-3 md:block" aria-hidden>
          <Mark />
        </div>
        {places
          .filter((p) => p.show)
          .map(({ to, label, icon: Icon, active }) => (
            <Tooltip key={label} label={label} side="right">
              <Button asChild variant="ghost" size="icon" className={cn(active && "bg-accent/20 text-accent hover:bg-accent/20")}>
                <Link to={to} aria-label={label} aria-current={active ? "page" : undefined}>
                  <Icon />
                </Link>
              </Button>
            </Tooltip>
          ))}
        <div className="hidden flex-1 md:block" />
        <ThemeButton />
        <UserMenu me={me} note={mine ? `${mine.name} · ${mine.org_alias === "Personal" ? "your own space" : mine.org_alias}` : undefined} />
      </nav>
      <main className="min-h-0 min-w-0 flex-1">
        {me && !mine ? (
          <Page title="No access">
            <PageEmpty icon={LockKeyhole} title="You are not part of this workspace">
              Ask an admin of the workspace to add you, then sign in again.
            </PageEmpty>
          </Page>
        ) : (
          <>
            {route.screen === "workspace" && <Workspace ws={ws} me={me} />}
            {route.screen === "catalog" && <Catalog ws={ws} id={route.id} me={me} />}
            <Suspense fallback={null}>
              {route.screen === "sql" && <SqlEditor ws={ws} />}
            {route.screen === "compute" && <Compute ws={ws} />}
            </Suspense>
            {route.screen === "settings" && <Settings ws={ws} me={me} />}
          </>
        )}
      </main>
    </div>
  );
}
