import { LogOut, Moon, Sun } from "lucide-react";
import { useEffect, useState, type AnchorHTMLAttributes, type ReactNode } from "react";
import { Button, Menu, MenuContent, MenuItem, MenuLabel, MenuSeparator, MenuTrigger, Tooltip } from "@/design";
import type { Me } from "@/api";
import { signOut } from "@/auth";
import { go, hrefOf, type Route } from "@/route";
import { applyTheme, getTheme, setTheme } from "@/theme";

/** A link that moves without reloading the page, and still opens in a new tab when asked to. */
export function Link({ to, children, ...rest }: { to: Route; children: ReactNode } & AnchorHTMLAttributes<HTMLAnchorElement>) {
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

export function ThemeButton() {
  const [theme, setThemeState] = useState(getTheme);
  const dark = theme === "dark" || (theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  useEffect(() => applyTheme(theme), [theme]);
  return (
    <Tooltip label={dark ? "Light theme" : "Dark theme"} side="bottom">
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
  );
}

/** The person's menu: who they are, whatever `children` adds (as menu items), and signing out. */
export function UserMenu({ me, note, children }: { me: Me | undefined; note?: string; children?: ReactNode }) {
  return (
    <Menu>
      <MenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label="Account menu" className="rounded-full bg-badge text-xs font-semibold uppercase text-badge-foreground">
          {(me?.name || me?.subject || "?").slice(0, 2)}
        </Button>
      </MenuTrigger>
      <MenuContent align="end">
        <MenuLabel>{me ? me.name || me.subject : "Signed in"}</MenuLabel>
        {me?.email && <p className="px-2 text-2xs text-muted">{me.email}</p>}
        {note && <p className="px-2 pb-1.5 text-2xs text-muted">{note}</p>}
        <MenuSeparator />
        {children}
        <MenuItem onSelect={() => void signOut()}>
          <LogOut /> Sign out
        </MenuItem>
      </MenuContent>
    </Menu>
  );
}
