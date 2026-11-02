import { ChevronRight, type LucideIcon } from "lucide-react";
import { useState } from "react";
import { cn } from "@/design";
import type { Kind, Securable } from "@/api";
import { canContain, kindIcon } from "@/kinds";

const INDENT = 12;

/** What a schema holds, listed in groups as Databricks does: its tables (and views) apart from its volumes. */
const GROUPS: { label: string; kinds: Kind[] }[] = [
  { label: "Tables", kinds: ["table", "view"] },
  { label: "Volumes", kinds: ["volume"] },
];

interface Shared {
  byParent: Record<string, Securable[]>;
  expanded: Set<string>;
  selected: string | null;
  onToggle: (s: Securable) => void;
  onOpen: (s: Securable) => void;
}

/**
 * One 28px row: an optional chevron, an icon and a name. The chevron opens and closes; the name goes to the thing (for a group, which
 * has nowhere to go, it opens and closes too). The whole row shows what is selected.
 */
export function TreeRow({
  depth,
  icon: Icon,
  label,
  title,
  active = false,
  open,
  toggleLabel,
  onToggle,
  onOpen,
}: {
  depth: number;
  icon?: LucideIcon;
  label: string;
  title?: string;
  active?: boolean;
  /** Undefined: nothing to open here, so no chevron. */
  open?: boolean;
  toggleLabel?: string;
  onToggle?: () => void;
  onOpen: () => void;
}) {
  return (
    <div className={cn("flex h-control-sm items-center rounded-md pr-2 text-sm text-foreground transition-colors", active ? "bg-accent/20" : "hover:bg-card-hover")} style={{ paddingLeft: 2 + depth * INDENT }}>
      {open === undefined ? (
        <span className="size-5 shrink-0" aria-hidden />
      ) : (
        <button type="button" aria-label={toggleLabel} onClick={onToggle} className="flex size-5 shrink-0 cursor-pointer items-center justify-center rounded text-muted hover:text-foreground">
          <ChevronRight className={cn("icon-free size-3.5 transition-transform", open && "rotate-90")} aria-hidden />
        </button>
      )}
      <button
        type="button"
        onClick={onOpen}
        title={title ?? label}
        aria-current={active ? "true" : undefined}
        className="flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent"
      >
        {Icon && <Icon className={cn("icon-free size-4 shrink-0", active ? "text-accent" : "text-muted")} aria-hidden />}
        <span className="truncate">{label}</span>
      </button>
    </div>
  );
}

/** The catalog as a compact list. Children are loaded when a row opens, and kept by the parent screen. */
export function Tree({ parent = "", depth = 0, ...shared }: Shared & { parent?: string; depth?: number }) {
  return (
    <ul role={depth === 0 ? "tree" : "group"} className="flex flex-col gap-px">
      {(shared.byParent[parent] ?? []).map((s) => (
        <Node key={s.id} item={s} depth={depth} {...shared} />
      ))}
    </ul>
  );
}

function Node({ item: s, depth, ...shared }: Shared & { item: Securable; depth: number }) {
  const { byParent, expanded, selected, onToggle, onOpen } = shared;
  const container = canContain(s.kind);
  const open = expanded.has(s.id);
  const kids = byParent[s.id] ?? [];
  return (
    <li role="treeitem" aria-expanded={container ? open : undefined} aria-selected={selected === s.id}>
      <TreeRow
        depth={depth}
        icon={kindIcon[s.kind]}
        label={s.name}
        active={selected === s.id}
        open={container ? open : undefined}
        toggleLabel={open ? `Collapse ${s.name}` : `Expand ${s.name}`}
        onToggle={() => onToggle(s)}
        onOpen={() => onOpen(s)}
      />
      {open && container &&
        (s.kind === "schema" ? (
          <ul role="group" className="flex flex-col gap-px">
            {GROUPS.map((g) => {
              const of = kids.filter((k) => g.kinds.includes(k.kind));
              return of.length === 0 ? null : <Group key={g.label} label={`${g.label} (${of.length})`} depth={depth + 1} items={of} {...shared} />;
            })}
          </ul>
        ) : (
          <Tree {...shared} parent={s.id} depth={depth + 1} />
        ))}
    </li>
  );
}

/** "Tables (3)": open until closed, and a row of its own that goes nowhere. */
function Group({ label, depth, items, ...shared }: Shared & { label: string; depth: number; items: Securable[] }) {
  const [open, setOpen] = useState(true);
  const { selected, onToggle, onOpen } = shared;
  return (
    <li role="treeitem" aria-expanded={open}>
      <TreeRow depth={depth} label={label} open={open} toggleLabel={`${open ? "Collapse" : "Expand"} ${label}`} onToggle={() => setOpen(!open)} onOpen={() => setOpen(!open)} />
      {open && (
        <ul role="group" className="flex flex-col gap-px">
          {items.map((s) => (
            <li key={s.id} role="treeitem" aria-selected={selected === s.id}>
              <TreeRow depth={depth + 1} icon={kindIcon[s.kind]} label={s.name} active={selected === s.id} onToggle={() => onToggle(s)} onOpen={() => onOpen(s)} />
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}
