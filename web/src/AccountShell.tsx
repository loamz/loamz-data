import { Building2, House, type LucideIcon } from "lucide-react";
import { Button, MenuItem, Tooltip, cn } from "@/design";
import * as api from "./api";
import { Link, ThemeButton, UserMenu } from "./components/RailParts";
import { useLoad, usePath } from "./hooks";
import { Mark } from "./Mark";
import { go, parseRoute, type Route } from "./route";
import { Account } from "./screens/Account";
import { Home } from "./screens/Home";

/** The account site: Home (your workspaces and organizations) and the account console. Choosing a workspace leaves for that workspace's own site. */
export function AccountShell() {
  const route = parseRoute(usePath(), "account");
  const who = useLoad(api.me, "me");
  const me = who.data;
  const anAdmin = me?.orgs.find((o) => o.account_admin && o.kind === "org");
  const places: { to: Route; label: string; icon: LucideIcon; active: boolean }[] = [
    { to: { screen: "home" }, label: "Home", icon: House, active: route.screen === "home" },
    ...(anAdmin ? [{ to: { screen: "account", org: anAdmin.id, tab: "workspaces" } as Route, label: "Account console", icon: Building2, active: route.screen === "account" }] : []),
  ];
  return (
    <div className="flex h-full flex-col-reverse bg-background text-foreground md:flex-row">
      <nav aria-label="Main" className="flex shrink-0 items-center justify-around gap-1 border-t border-border bg-sidebar px-2 py-1.5 md:w-14 md:flex-col md:justify-start md:border-r md:border-t-0 md:px-0 md:py-3">
        <div className="hidden md:mb-3 md:block">
          <Link to={{ screen: "home" }} aria-label="Loamz Data home">
            <Mark />
          </Link>
        </div>
        {places.map(({ to, label, icon: Icon, active }) => (
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
        <UserMenu me={me}>
          {anAdmin && (
            <MenuItem onSelect={() => go({ screen: "account", org: anAdmin.id, tab: "workspaces" })}>
              <Building2 /> Account console
            </MenuItem>
          )}
        </UserMenu>
      </nav>
      <main className="min-h-0 min-w-0 flex-1">
        {route.screen === "home" && <Home me={me} reload={who.reload} />}
        {route.screen === "account" && <Account org={route.org} tab={route.tab} me={me} />}
      </main>
    </div>
  );
}
