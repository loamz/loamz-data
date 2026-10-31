import { Database } from "lucide-react";

/** The product's square: a violet tile with a database glyph. */
export function Mark({ className = "" }: { className?: string }) {
  return (
    <span className={`inline-flex size-8 shrink-0 items-center justify-center rounded-lg bg-accent text-accent-foreground ${className}`} aria-hidden>
      <Database />
    </span>
  );
}
