import { ChevronRight, Search } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Fragment, type MouseEvent, type ReactNode } from "react";
import { cn } from "../cn";
import { Input } from "../ui/Input";

/**
 * The frame of a data product: a top bar over a sidebar and the body. Data screens (catalogs, tables, queries, runs) are read
 * and scanned, not browsed, so everything here is dense: no page titles, no explanatory sentences, tables sized to their content.
 */
export function Workbench({ top, side, children }: { top: ReactNode; side: ReactNode; children: ReactNode }) {
  return (
    <div className="flex h-dvh min-h-0 w-full flex-col bg-background text-foreground">
      {top}
      <div className="flex min-h-0 flex-1">
        {side}
        <main className="min-h-0 min-w-0 flex-1">{children}</main>
      </div>
    </div>
  );
}

/** Left: who and where. Center: search. Right: the person's own controls. */
export function TopBar({ left, center, right }: { left: ReactNode; center?: ReactNode; right?: ReactNode }) {
  return (
    <header className="grid h-12 shrink-0 grid-cols-[1fr_minmax(0,40rem)_1fr] items-center gap-3 border-b border-border px-3">
      <div className="flex min-w-0 items-center gap-2">{left}</div>
      <div className="min-w-0">{center}</div>
      <div className="flex items-center justify-end gap-1">{right}</div>
    </header>
  );
}

/** The search field of the top bar. It only looks like one: it opens the real search (`onOpen`) so results can live in a popover. */
export function SearchField({ placeholder, hint, value, onChange, onFocus }: { placeholder: string; hint?: string; value?: string; onChange?: (v: string) => void; onFocus?: () => void }) {
  return (
    <div className="relative">
      <Search className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted" aria-hidden />
      <Input size="md" className="pl-9 pr-16" placeholder={placeholder} aria-label={placeholder} value={value} onChange={(e) => onChange?.(e.target.value)} onFocus={onFocus} />
      {hint && <kbd className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 rounded border border-border px-1.5 text-2xs text-muted">{hint}</kbd>}
    </div>
  );
}

export function Sidebar({ children, footer, label = "Main" }: { children: ReactNode; footer?: ReactNode; label?: string }) {
  return (
    <nav aria-label={label} className="hidden w-44 shrink-0 flex-col border-r border-border bg-sidebar md:flex">
      <div className="scroll-area flex min-h-0 flex-1 flex-col gap-0.5 px-2 py-1.5">{children}</div>
      {footer && <div className="flex flex-col gap-0.5 border-t border-border px-2 py-1.5">{footer}</div>}
    </nav>
  );
}

export function SidebarGroup({ label, children }: { label?: string; children: ReactNode }) {
  return (
    <div className="mt-2 flex flex-col gap-0.5 first:mt-0">
      {label && <p className="px-2.5 pb-1 text-2xs font-medium uppercase tracking-wide text-muted">{label}</p>}
      {children}
    </div>
  );
}

/** One row of the sidebar: 32px, an icon and a label, violet while its page is showing. */
export function SidebarItem({ icon: Icon, active, onClick, href, children }: { icon: LucideIcon; active?: boolean; onClick?: (e: MouseEvent<HTMLElement>) => void; href?: string; children: ReactNode }) {
  const cls = cn(
    "flex h-control-md w-full cursor-pointer items-center gap-2.5 rounded-md px-2.5 text-left text-sm font-medium text-foreground transition-colors " +
      "focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent",
    active ? "bg-accent/20" : "hover:bg-card-hover",
  );
  const inner = (
    <>
      <Icon className={cn("shrink-0", active ? "text-accent" : "text-muted")} aria-hidden />
      <span className="min-w-0 flex-1 truncate">{children}</span>
    </>
  );
  return href ? (
    <a href={href} aria-current={active ? "page" : undefined} className={cls} onClick={onClick}>
      {inner}
    </a>
  ) : (
    <button type="button" aria-current={active ? "page" : undefined} className={cls} onClick={onClick}>
      {inner}
    </button>
  );
}

export interface Crumb {
  label: string;
  onClick?: () => void;
}

/**
 * The top of an object's page: where it sits, then its icon, name and the actions that apply to it. No subtitle: what the object is
 * belongs in its tabs and its `DetailRail`.
 */
export function ObjectHeader({ crumbs, icon: Icon, title, tools, actions }: { crumbs?: Crumb[]; icon?: LucideIcon; title: string; tools?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      {crumbs && crumbs.length > 0 && (
        <ol className="flex flex-wrap items-center gap-1 text-sm">
          {crumbs.map((c, i) => (
            <Fragment key={`${i}-${c.label}`}>
              {i > 0 && <ChevronRight className="text-muted" aria-hidden />}
              <li>
                {c.onClick ? (
                  <button type="button" onClick={c.onClick} className="cursor-pointer text-accent hover:underline">
                    {c.label}
                  </button>
                ) : (
                  <span className="text-muted">{c.label}</span>
                )}
              </li>
            </Fragment>
          ))}
        </ol>
      )}
      <div className="flex items-center gap-2">
        {Icon && (
          <span className="flex size-control-lg shrink-0 items-center justify-center rounded-md border border-border text-muted">
            <Icon aria-hidden />
          </span>
        )}
        <h1 className="min-w-0 truncate text-xl font-semibold tracking-tight">{title}</h1>
        {tools && <div className="flex items-center gap-0.5">{tools}</div>}
        {actions && <div className="ml-auto flex shrink-0 items-center gap-2">{actions}</div>}
      </div>
    </div>
  );
}

/** A full-height scrolling body with the workbench's padding (no title: the `ObjectHeader` is the first child when there is one). */
export function Body({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className="scroll-area @container h-full min-h-0 w-full">
      <div className={cn("flex min-h-full flex-col gap-3 px-4 py-3", className)}>{children}</div>
    </div>
  );
}

/** A main column with a `DetailRail` beside it (under it on a narrow screen). */
export function WithRail({ children, rail }: { children: ReactNode; rail: ReactNode }) {
  return (
    <div className="flex flex-col gap-3 @4xl:flex-row @4xl:items-start">
      <div className="min-w-0 flex-1">{children}</div>
      <aside className="flex w-full shrink-0 flex-col gap-4 @4xl:w-72">{rail}</aside>
    </div>
  );
}

/** A titled block in the rail: label/value rows (a `DetailRow`), or anything else. */
export function DetailBlock({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-sm font-semibold">{title}</h2>
        {action}
      </div>
      <div className="flex flex-col gap-1.5 text-sm">{children}</div>
    </section>
  );
}

export function DetailRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-baseline gap-3">
      <span className="w-20 shrink-0 text-muted">{label}</span>
      <span className="min-w-0 flex-1 break-words">{children}</span>
    </div>
  );
}

/** The left pane of a browser (a catalog tree, a schema list): a header with small actions, a filter, then the rows. */
export function TreePane({ title, actions, note, filter, onFilter, children }: { title: string; actions?: ReactNode; note?: ReactNode; filter?: string; onFilter?: (v: string) => void; children: ReactNode }) {
  return (
    <div className="flex h-full min-h-0 w-60 shrink-0 flex-col border-r border-border">
      <div className="flex h-control-lg items-center justify-between gap-2 px-3 pt-1">
        <h2 className="text-sm font-semibold">{title}</h2>
        <div className="flex items-center gap-0.5">{actions}</div>
      </div>
      {note && <div className="truncate px-3 pt-1 text-xs text-muted">{note}</div>}
      {onFilter && (
        <div className="p-2">
          <Input size="md" placeholder="Filter" aria-label="Filter" value={filter ?? ""} onChange={(e) => onFilter(e.target.value)} />
        </div>
      )}
      <div className="scroll-area min-h-0 flex-1 px-1 pb-2">{children}</div>
    </div>
  );
}
