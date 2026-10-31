import { Building2, Database, House, LogOut, Moon, Settings as Gear, Sun, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Button, ConfirmHost, Menu, MenuContent, MenuItem, MenuLabel, MenuSeparator, MenuTrigger, Toaster, Tooltip, TooltipProvider, cn } from "@/design";
import * as api from "./api";
import { finishLogin, onAuthChange, signOut, signedIn } from "./auth";
import { useLoad, usePath } from "./hooks";
import { Mark } from "./Mark";
import { parseRoute, hrefOf, go, type Route } from "./route";
import { Account } from "./screens/Account";
import { Catalog } from "./screens/Catalog";
import { Home } from "./screens/Home";
import { Settings } from "./screens/Settings";
import { SignIn } from "./SignIn";
import { applyTheme, getTheme, setTheme } from "./theme";

const snapshot = () => (signedIn() ? "1" : "");
const LAST = "loamz-data-last-workspace";

function remembered(): string | null {
  try {
    return localStorage.getItem(LAST);
  } catch {
    return null;
  }
}

export function App() {
  const here = useSyncExternalStore(onAuthChange, snapshot) !== "";
  const [problem, setProblem] = useState("");
  const [ready, setReady] = useState(false);
  useEffect(() => {
    finishLogin()
      .catch((e: Error) => setProblem(e.message))
      .finally(() => setReady(true));
  }, []);
  if (!ready) return null;
  return (
    <TooltipProvider delayDuration={300}>
      {here ? <Shell /> : <SignIn notice={problem} />}
      <Toaster />
      <ConfirmHost />
    </TooltipProvider>
  );
}

/** A link that moves without reloading, and still opens in a new tab when asked to. */
function Link({ to, children, ...rest }: { to: Route; children: React.ReactNode } & React.AnchorHTMLAttributes<HTMLAnchorElement>) {
  return (
    <a
      href={hrefOf(to)}
      onClick={(e) => {
        if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
        e.preventDefault();
        go(to);
      }}
      {...rest}
    >
      {children}
    </a>
  );
}

function Shell() {
  const route = parseRoute(usePath());
  const who = useLoad(api.me, "me");
  const me = who.data;
  const [theme, setThemeState] = useState(getTheme);
  const dark = theme === "dark" || (theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  useEffect(() => applyTheme(theme), [theme]);

  // The workspace the rail points into: the one in the address, else the last one used, else the first there is.
  const inAddress = route.screen === "catalog" || route.screen === "settings" ? route.ws : null;
  const ws = inAddress ?? (me?.workspaces.find((w) => w.id === remembered())?.id ?? me?.workspaces[0]?.id ?? null);
  useEffect(() => {
    if (inAddress) {
      try {
        localStorage.setItem(LAST, inAddress);
      } catch {
        /* the rail falls back to the first workspace */
      }
    }
  }, [inAddress]);
  // A workspace made, renamed or opened from a link since we last asked: ask again, once for each.
  const asked = useRef(new Set<string>());
  useEffect(() => {
    if (inAddress && me && !me.workspaces.some((w) => w.id === inAddress) && !asked.current.has(inAddress)) {
      asked.current.add(inAddress);
      who.reload();
    }
  }, [inAddress, me]); // eslint-disable-line react-hooks/exhaustive-deps
  const anAdmin = me?.orgs.find((o) => o.account_admin && o.kind === "org");

  const places: { to: Route; label: string; icon: LucideIcon; active: boolean; show: boolean }[] = [
    { to: { screen: "home" }, label: "Home", icon: House, active: route.screen === "home", show: true },
    { to: { screen: "catalog", ws: ws ?? "", id: null }, label: "Catalog", icon: Database, active: route.screen === "catalog", show: ws !== null },
    { to: { screen: "settings", ws: ws ?? "" }, label: "Workspace settings", icon: Gear, active: route.screen === "settings", show: ws !== null },
    { to: { screen: "account", org: anAdmin?.id ?? "", tab: "workspaces" }, label: "Account console", icon: Building2, active: route.screen === "account", show: Boolean(anAdmin) },
  ];

  return (
    <div className="flex h-full flex-col-reverse bg-background text-foreground md:flex-row">
      <nav aria-label="Main" className="flex shrink-0 items-center justify-around gap-1 border-t border-border bg-sidebar px-2 py-1.5 md:w-14 md:flex-col md:justify-start md:border-r md:border-t-0 md:px-0 md:py-3">
        <div className="hidden md:mb-3 md:block">
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
        <Tooltip label={dark ? "Light theme" : "Dark theme"} side="right">
          <Button
            variant="ghost"
            size="icon"
            aria-label={dark ? "Switch to light theme" : "Switch to dark theme"}
            onClick={() => {
              const next = dark ? "light" : "dark";
              setTheme(next);
              setThemeState(next);
            }}
          >
            {dark ? <Sun /> : <Moon />}
          </Button>
        </Tooltip>
        <Menu>
          <MenuTrigger asChild>
            <Button variant="ghost" size="icon" aria-label="Account menu" className="rounded-full bg-badge text-xs font-semibold uppercase text-badge-foreground">
              {(me?.name || me?.subject || "?").slice(0, 2)}
            </Button>
          </MenuTrigger>
          <MenuContent side="right" align="end">
            <MenuLabel>{me ? me.name || me.subject : "Signed in"}</MenuLabel>
            {me?.email && <p className="px-2 pb-1.5 text-2xs text-muted">{me.email}</p>}
            <MenuSeparator />
            <MenuItem onSelect={() => void signOut()}>
              <LogOut /> Sign out
            </MenuItem>
          </MenuContent>
        </Menu>
      </nav>
      <main className="min-h-0 min-w-0 flex-1">
        {route.screen === "home" && <Home me={me} reload={who.reload} />}
        {route.screen === "catalog" && <Catalog key={route.ws} ws={route.ws} id={route.id} me={me} />}
        {route.screen === "settings" && <Settings ws={route.ws} me={me} />}
        {route.screen === "account" && <Account org={route.org} tab={route.tab} me={me} />}
      </main>
    </div>
  );
}
