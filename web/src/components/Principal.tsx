import { parsePrincipal, principalIcon, principalLabel } from "@/principals";

/** A person, group or service principal, with its icon. */
export function Principal({ id }: { id: string }) {
  const Icon = principalIcon[parsePrincipal(id).type];
  return (
    <span className="inline-flex min-w-0 items-center gap-2" title={id}>
      <Icon className="shrink-0 text-muted" aria-hidden />
      <span className="truncate text-sm text-foreground">{principalLabel(id)}</span>
    </span>
  );
}
