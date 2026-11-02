import type { WireEvent } from "./stream";

export interface ToolRun {
  id: string;
  name: string;
  risk: string | null;
  state: "running" | "done" | "failed";
  output: string;
}

/** One answer being built: the text so far, the tools it called, and how it ended. */
export interface Turn {
  text: string;
  tools: ToolRun[];
  status: "running" | "done" | "failed" | "cancelled";
  error: string | null;
  /** The agent is waiting for a person to approve something; the app shows its own card. */
  approval: Record<string, unknown> | null;
}

export const newTurn = (): Turn => ({ text: "", tools: [], status: "running", error: null, approval: null });

export function apply(turn: Turn, e: WireEvent): Turn {
  switch (e.type) {
    case "text.delta":
      return { ...turn, text: turn.text + e.text };
    case "tool.call":
      return { ...turn, tools: [...turn.tools, { id: e.call_id, name: e.tool_name, risk: e.risk ?? null, state: "running", output: "" }] };
    case "tool.result":
      return {
        ...turn,
        tools: turn.tools.map((t) => (t.id === e.call_id ? { ...t, state: e.ok ? "done" : "failed", output: e.ok ? e.output : (e.error ?? e.output) } : t)),
      };
    case "approval.requested":
      return { ...turn, approval: e };
    case "turn.completed":
      // The final text is authoritative when the server sends it (deltas may have been partial after a reconnect).
      return { ...turn, text: e.text ?? turn.text };
    case "run.completed":
      return { ...turn, status: "done", approval: null };
    case "run.failed":
      return { ...turn, status: "failed", error: e.error, approval: null };
    case "run.cancelled":
      return { ...turn, status: "cancelled", approval: null };
    default:
      return turn;
  }
}

/** Reads a whole stream into turns, calling `onTurn` after each event so a view can follow along. */
export async function follow(source: AsyncIterable<WireEvent>, onTurn: (t: Turn) => void): Promise<Turn> {
  let turn = newTurn();
  onTurn(turn);
  for await (const e of source) {
    turn = apply(turn, e);
    onTurn(turn);
  }
  // A stream that ends without saying how is a cut connection, not a success.
  if (turn.status === "running") turn = { ...turn, status: "failed", error: "The connection ended before the answer was finished." };
  onTurn(turn);
  return turn;
}
