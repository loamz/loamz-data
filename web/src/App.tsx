import { Database, LogOut, Moon, ScrollText, ShieldCheck, Sun, type LucideIcon } from "lucide-react";
import { useEffect, useState, useSyncExternalStore } from "react";
import { Button, ConfirmHost, Menu, MenuContent, MenuItem, MenuLabel, MenuSeparator, MenuTrigger, Toaster, Tooltip, TooltipProvider, cn } from "@/design";
import * as api from "./api";
import { finishLogin, getToken, onAuthChange, signOut, takeTokenFromHash } from "./auth";
import { Activity } from "./screens/Activity";
import { Admin } from "./screens/Admin";
import { Catalog } from "./screens/Catalog";
import { hrefOf, parseRoute, type Route } from "./route";
import { useHash, useLoad } from "./hooks";
import { Mark } from "./Mark";
import { SignIn } from "./SignIn";
import { applyTheme, getTheme, setTheme } from "./theme";

const tokenSnapshot = () => getToken() ?? "";

function useSignedIn(): boolean {
  return useSyncExternalStore(onAuthChange, tokenSnapshot) !== "";
}

const places: { route: Route; label: string; icon: LucideIcon }[] = [
  { route: { screen: "catalog", id: null }, label: "Catalog", icon: Database },
  { route: { screen: "activity" }, label: "Activity", icon: ScrollText },
  { route: { screen: "admin" }, label: "Admin", icon: ShieldCheck },
];

export function App() {
  const signedIn = useSignedIn();
  const [problem, setProblem] = useState("");
  const [ready, setReady] = useState(false);
  useEffect(() => {
    takeTokenFromHash();
    finishLogin()
      .catch((e: Error) => setProblem(e.message))
      .finally(() => setReady(true));
  }, []);
  if (!ready) return null;
  return (
    <TooltipProvider delayDuration={300}>
      {signedIn ? <Shell /> : <SignIn notice={problem} />}
      <Toaster />
      <ConfirmHost />
    </TooltipProvider>
  );
}

function Shell() {
  const route = parseRoute(useHash());
  const who = useLoad(api.me, "me");
  const [theme, setThemeState] = useState(getTheme);
  const dark = theme === "dark" || (theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  useEffect(() => applyTheme(theme), [theme]);
  // A rejected token (the server said 401) is handled in api.ts by signing out; any other failure to learn who we are is shown below.
  const me = who.data;
  return (
    <div className="flex h-full flex-col-reverse bg-background text-foreground md:flex-row">
      <nav aria-label="Main" className="flex shrink-0 items-center justify-around gap-1 border-t border-border bg-sidebar px-2 py-1.5 md:w-14 md:flex-col md:justify-start md:border-r md:border-t-0 md:px-0 md:py-3">
        <div className="hidden md:mb-3 md:block"><Mark /></div>
        {places.map(({ route: r, label, icon: Icon }) => {
          const active = r.screen === route.screen;
          return (
            <Tooltip key={label} label={label} side="right">
              <Button asChild variant="ghost" size="icon" className={cn(active && "bg-accent/20 text-accent hover:bg-accent/20")}>
                <a href={hrefOf(r)} aria-label={label} aria-current={active ? "page" : undefined}>
                  <Icon />
                </a>
              </Button>
            </Tooltip>
          );
        })}
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
            <Button variant="ghost" size="icon" aria-label="Account" className="rounded-full bg-badge text-xs font-semibold uppercase text-badge-foreground">
              {(me?.subject ?? "?").slice(0, 2)}
            </Button>
          </MenuTrigger>
          <MenuContent side="right" align="end">
            <MenuLabel>{me ? me.subject : "Signed in"}</MenuLabel>
            {me && <p className="px-2 pb-1.5 text-2xs text-muted">{me.tenant.startsWith("u-") ? "Personal space" : `Organization ${me.tenant.slice(2)}`}{me.tenant_owner ? " · owner" : ""}</p>}
            <MenuSeparator />
            <MenuItem onSelect={signOut}>
              <LogOut /> Sign out
            </MenuItem>
          </MenuContent>
        </Menu>
      </nav>
      <main className="min-h-0 min-w-0 flex-1">
        {route.screen === "catalog" && <Catalog id={route.id} />}
        {route.screen === "activity" && <Activity />}
        {route.screen === "admin" && <Admin me={me} />}
      </main>
    </div>
  );
}
