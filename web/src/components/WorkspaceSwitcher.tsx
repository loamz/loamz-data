import { Check, ChevronsUpDown } from "lucide-react";
import { Menu, MenuContent, MenuItem, MenuLabel, MenuSeparator, MenuTrigger } from "@/design";
import type { WorkspaceInfo } from "@/api";
import { openWorkspace } from "@/site";

/** The name of the workspace you are in; open it to move to another, as in Databricks. */
export function WorkspaceSwitcher({ current, all }: { current: string; all: WorkspaceInfo[] }) {
  const here = all.find((w) => w.id === current);
  const orgs = [...new Set(all.map((w) => w.org_alias))];
  return (
    <Menu>
      <MenuTrigger asChild>
        <button type="button" className="flex h-control-md min-w-0 max-w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-left text-sm font-semibold text-foreground transition-colors hover:bg-card-hover" aria-label="Switch workspace">
          <span className="truncate">{here?.name ?? "Workspace"}</span>
          <ChevronsUpDown className="shrink-0 text-muted" aria-hidden />
        </button>
      </MenuTrigger>
      <MenuContent align="end">
        {orgs.map((org, i) => (
          <div key={org}>
            {i > 0 && <MenuSeparator />}
            <MenuLabel>{org}</MenuLabel>
            {all
              .filter((w) => w.org_alias === org)
              .map((w) => (
                <MenuItem key={w.id} onSelect={() => w.id !== current && openWorkspace(w.id)}>
                  {w.id === current ? <Check /> : <span className="size-[var(--icon-size)]" />} {w.name}
                </MenuItem>
              ))}
          </div>
        ))}
      </MenuContent>
    </Menu>
  );
}
