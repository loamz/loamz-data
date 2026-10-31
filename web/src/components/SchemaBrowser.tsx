import { ChevronRight } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "@/design";
import * as api from "@/api";
import { kindIcon } from "@/kinds";

/** The tables you can reach, as a tree to click into the editor. Children are loaded when a row is opened. */
export function SchemaBrowser({ ws, onPick, onNames }: { ws: string; onPick: (fullName: string) => void; onNames: (names: string[]) => void }) {
  const [roots, setRoots] = useState<api.Securable[] | null>(null);
  const [failed, setFailed] = useState("");
  const known = useRef(new Set<string>());
  useEffect(() => {
    let live = true;
    api.list(ws, "").then(
      (r) => live && setRoots(r),
      (e: Error) => live && setFailed(e.message),
    );
    return () => {
      live = false;
    };
  }, [ws]);
  // Completion learns each schema's tables as they are opened.
  const learn = (names: string[]) => {
    names.forEach((n) => known.current.add(n));
    onNames([...known.current].sort());
  };
  return (
    <div className="scroll-area min-h-0 flex-1 px-2 pb-3" role="tree" aria-label="Tables">
      {failed ? (
        <p className="px-2 py-2 text-xs text-danger">{failed}</p>
      ) : roots?.length === 0 ? (
        <p className="px-3 py-4 text-xs text-muted">No catalogs here yet.</p>
      ) : (
        <ul className="space-y-0.5">
          {(roots ?? []).map((s) => (
            <Row key={s.id} ws={ws} item={s} path={[s.name]} depth={0} onPick={onPick} onTables={learn} />
          ))}
        </ul>
      )}
    </div>
  );
}

function Row({ ws, item, path, depth, onPick, onTables }: { ws: string; item: api.Securable; path: string[]; depth: number; onPick: (n: string) => void; onTables: (names: string[]) => void }) {
  const [open, setOpen] = useState(false);
  const [children, setChildren] = useState<api.Securable[] | null>(null);
  const Icon = kindIcon[item.kind];
  const isContainer = item.kind === "catalog" || item.kind === "schema";
  const isQueryable = item.kind === "table" || item.kind === "view";
  const toggle = async () => {
    const next = !open;
    setOpen(next);
    if (next && children === null) {
      try {
        const got = await api.list(ws, item.id);
        setChildren(got);
        // A schema's tables are what completion should know.
        onTables(got.filter((c) => c.kind === "table" || c.kind === "view").map((c) => [...path, c.name].join(".")));
      } catch {
        setChildren([]);
      }
    }
  };
  const shown = (children ?? []).filter((c) => c.kind === "schema" || c.kind === "table" || c.kind === "view");
  return (
    <li role="treeitem" aria-expanded={isContainer ? open : undefined}>
      <button
        type="button"
        onClick={() => (isContainer ? void toggle() : isQueryable && onPick(path.join(".")))}
        className={cn("flex h-control-sm w-full cursor-pointer items-center gap-1.5 rounded-md pr-2 text-left text-xs transition-colors hover:bg-card-hover", isQueryable ? "text-foreground" : "font-medium text-foreground")}
        style={{ paddingLeft: 6 + depth * 12 }}
        title={isQueryable ? `Insert ${path.join(".")}` : item.name}
        disabled={!isContainer && !isQueryable}
      >
        {isContainer ? <ChevronRight className={cn("shrink-0 text-muted transition-transform", open && "rotate-90")} aria-hidden /> : <span className="size-[var(--icon-size)] shrink-0" aria-hidden />}
        <Icon className="shrink-0 text-muted" aria-hidden />
        <span className="min-w-0 truncate">{item.name}</span>
      </button>
      {open && isContainer && (
        <ul className="space-y-0.5">
          {children === null ? (
            <li className="px-3 py-1 text-2xs text-muted" style={{ paddingLeft: 30 + depth * 12 }}>Loading…</li>
          ) : shown.length === 0 ? (
            <li className="px-3 py-1 text-2xs text-muted" style={{ paddingLeft: 30 + depth * 12 }}>Nothing you can query here</li>
          ) : (
            shown.map((c) => <Row key={c.id} ws={ws} item={c} path={[...path, c.name]} depth={depth + 1} onPick={onPick} onTables={onTables} />)
          )}
        </ul>
      )}
    </li>
  );
}
