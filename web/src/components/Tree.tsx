import { ChevronRight } from "lucide-react";
import { Button, NavItem, cn } from "@/design";
import type { Securable } from "@/api";
import { canContain, kindIcon } from "@/kinds";

/** The catalog as an indented list. Children are loaded when a row opens, and kept by the parent screen. */
export function Tree({ byParent, expanded, selected, onToggle, parent = "", depth = 0, onOpen }: { byParent: Record<string, Securable[]>; expanded: Set<string>; selected: string | null; onToggle: (s: Securable) => void; parent?: string; depth?: number; onOpen: (s: Securable) => void }) {
  return (
    <ul role={depth === 0 ? "tree" : "group"} className="space-y-0.5">
      {(byParent[parent] ?? []).map((s) => {
        const open = expanded.has(s.id);
        const Icon = kindIcon[s.kind];
        return (
          <li key={s.id} role="treeitem" aria-expanded={canContain(s.kind) ? open : undefined} aria-selected={selected === s.id}>
            <div className="flex items-center" style={{ paddingLeft: depth * 14 }}>
              {canContain(s.kind) ? (
                <Button variant="ghost" size="icon-sm" aria-label={open ? `Collapse ${s.name}` : `Expand ${s.name}`} onClick={() => onToggle(s)} className="mr-0.5">
                  <ChevronRight className={cn("transition-transform", open && "rotate-90")} />
                </Button>
              ) : (
                <span className="mr-0.5 size-control-sm shrink-0" aria-hidden />
              )}
              <NavItem icon={Icon} active={selected === s.id} onClick={() => onOpen(s)} className="h-control-md">
                {s.name}
              </NavItem>
            </div>
            {open && canContain(s.kind) && (
              <Tree byParent={byParent} expanded={expanded} selected={selected} onToggle={onToggle} onOpen={onOpen} parent={s.id} depth={depth + 1} />
            )}
          </li>
        );
      })}
    </ul>
  );
}
