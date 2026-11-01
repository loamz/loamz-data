import { Briefcase, Cpu, Database, FileCode2, History, LayoutDashboard, LockKeyhole, Settings as Gear, TerminalSquare, type LucideIcon } from "lucide-react";
import { Suspense, lazy, type MouseEvent } from "react";
import { Button, PageEmpty, Page, Sidebar, SidebarGroup, SidebarItem, TopBar, Workbench, cn } from "@/design";
import * as api from "./api";
import { Link, ThemeButton, UserMenu } from "./components/RailParts";
import { useLoad, usePath } from "./hooks";
import { Mark } from "./Mark";
import { WorkspaceSwitcher } from "./components/WorkspaceSwitcher";
import { go, hrefOf, parseRoute, type Route } from "./route";
import { Catalog } from "./screens/Catalog";
import { Jobs } from "./screens/Jobs";
import { Queries } from "./screens/Queries";
import { Run } from "./screens/Run";
import { Settings } from "./screens/Settings";
import { Workspace } from "./screens/Workspace";

// The code editor is the heaviest part of the app and only two screens use it, so they are loaded when first opened.
const SqlEditor = lazy(() => import("./screens/SqlEditor").then((m) => ({ default: m.SqlEditor })));
const QueryHistory = lazy(() => import("./screens/QueryHistory").then((m) => ({ default: m.QueryHistory })));
const Compute = lazy(() => import("./screens/Compute").then((m) => ({ default: m.Compute })));

/** A sidebar link moves without reloading, and still opens in a new tab when asked to. */
const navigate = (to: Route) => (e: MouseEvent<HTMLElement>) => {
  if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
  e.preventDefault();
  go(to);
};

/**
 * One workspace, as a website of its own. Everything in it belongs to this workspace, and nothing links back out: not to Home, not to the
 * account console. Signing out is the only way to leave (the switcher on a page jumps to another workspace's own site).
 */
export function WorkspaceShell({ ws }: { ws: string }) {
  const route = parseRoute(usePath(), "workspace", ws);
  const who = useLoad(api.me, "me");
  const me = who.data;
  const mine = me?.workspaces.find((w) => w.id === ws);

  const places: { to: Route; label: string; icon: LucideIcon; active: boolean; show: boolean; group: "work" | "sql" | "admin" }[] = [
    { to: { screen: "workspace", ws }, label: "Home", icon: LayoutDashboard, active: route.screen === "workspace", show: true, group: "work" },
    { to: { screen: "catalog", ws, id: null }, label: "Catalog", icon: Database, active: route.screen === "catalog", show: true, group: "work" },
    { to: { screen: "jobs", ws }, label: "Jobs", icon: Briefcase, active: route.screen === "jobs" || route.screen === "run", show: true, group: "work" },
    { to: { screen: "compute", ws }, label: "Compute", icon: Cpu, active: route.screen === "compute", show: true, group: "work" },
    { to: { screen: "sql", ws }, label: "SQL editor", icon: TerminalSquare, active: route.screen === "sql", show: true, group: "sql" },
    { to: { screen: "queries", ws }, label: "Queries", icon: FileCode2, active: route.screen === "queries", show: true, group: "sql" },
    { to: { screen: "history", ws }, label: "Query history", icon: History, active: route.screen === "history", show: true, group: "sql" },
    { to: { screen: "settings", ws }, label: "Settings", icon: Gear, active: route.screen === "settings", show: Boolean(mine?.admin), group: "admin" },
  ];
  const shown = places.filter((p) => p.show);
  const item = (p: (typeof places)[number]) => (
    <SidebarItem key={p.label} icon={p.icon} active={p.active} href={hrefOf(p.to)} onClick={navigate(p.to)}>
      {p.label}
    </SidebarItem>
  );

  return (
    <Workbench
      top={
        <TopBar
          left={
            <>
              <Mark />
              {me && <WorkspaceSwitcher current={ws} all={me.workspaces} />}
            </>
          }
          right={
            <>
              <ThemeButton />
              <UserMenu me={me} note={mine ? `${mine.name} · ${mine.org_alias === "Personal" ? "your own space" : mine.org_alias}` : undefined} />
            </>
          }
        />
      }
      side={
        <Sidebar footer={shown.filter((p) => p.group === "admin").map(item)}>
          <SidebarGroup>{shown.filter((p) => p.group === "work").map(item)}</SidebarGroup>
          <SidebarGroup label="SQL">{shown.filter((p) => p.group === "sql").map(item)}</SidebarGroup>
        </Sidebar>
      }
    >
      <nav aria-label="Pages" className="fixed inset-x-0 bottom-0 z-20 flex items-center justify-around border-t border-border bg-sidebar px-2 py-1.5 md:hidden">
        {shown.map(({ to, label, icon: Icon, active }) => (
          <Button key={label} asChild variant="ghost" size="icon" className={cn(active && "bg-accent/20 text-accent hover:bg-accent/20")}>
            <Link to={to} aria-label={label} aria-current={active ? "page" : undefined}>
              <Icon />
            </Link>
          </Button>
        ))}
      </nav>
      <div className="h-full min-h-0 pb-12 md:pb-0">
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
              {route.screen === "history" && <QueryHistory ws={ws} me={me} />}
            {route.screen === "compute" && <Compute ws={ws} me={me} />}
            </Suspense>
            {route.screen === "queries" && <Queries ws={ws} />}
            {route.screen === "jobs" && <Jobs ws={ws} />}
            {route.screen === "run" && <Run ws={ws} id={route.id} />}
            {route.screen === "settings" && <Settings ws={ws} me={me} />}
          </>
        )}
      </div>
    </Workbench>
  );
}
