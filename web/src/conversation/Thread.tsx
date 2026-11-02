import type { ReactNode } from "react";
import type { Turn } from "./turn";

export interface Exchange {
  ask: string;
  turn: Turn;
}

/** A plain view of a conversation: what was asked, the tools the agent used (the app decides how to show one), and the answer.
 * It carries no styling beyond the design tokens, so it sits in any Loamz app. */
export function Thread({ exchanges, tool }: { exchanges: readonly Exchange[]; tool?: (t: Turn["tools"][number]) => ReactNode }) {
  return (
    <div className="flex flex-col gap-4" role="log" aria-live="polite">
      {exchanges.map((x, i) => (
        <div key={i} className="flex flex-col gap-2">
          <div className="self-end max-w-[85%] rounded-lg bg-card px-3 py-2 text-sm">{x.ask}</div>
          {x.turn.tools.map((t) => (
            <div key={t.id} className="text-xs text-muted">
              {tool ? tool(t) : `${t.name}: ${t.state}`}
            </div>
          ))}
          {x.turn.text && <div className="whitespace-pre-wrap text-sm">{x.turn.text}</div>}
          {x.turn.status === "running" && !x.turn.text && <div className="text-xs text-muted">Thinking…</div>}
          {x.turn.status === "failed" && <div className="text-sm text-danger">{x.turn.error}</div>}
          {x.turn.status === "cancelled" && <div className="text-xs text-muted">Stopped.</div>}
        </div>
      ))}
    </div>
  );
}
