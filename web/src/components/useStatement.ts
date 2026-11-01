import { useCallback, useRef, useState } from "react";
import * as api from "@/api";

export interface Statement {
  running: boolean;
  run?: api.Run;
  error?: string;
  /** When it was started, and how long it took once it ended. */
  startedAt?: number;
  tookMs?: number;
}

/** Runs a read-only statement on the workspace's compute and follows it until it ends. One at a time. */
export function useStatement(ws: string) {
  const [state, setState] = useState<Statement>({ running: false });
  const busy = useRef(false);
  const start = useCallback(
    async (sql: string, limit = 100, target?: string) => {
      if (busy.current) return;
      busy.current = true;
      const began = Date.now();
      setState({ running: true, startedAt: began });
      try {
        let r = await api.runStatement(ws, sql, limit, 0, target);
        setState({ running: true, run: r, startedAt: began });
        while (api.isRunning(r)) {
          await new Promise((ok) => setTimeout(ok, 700));
          r = await api.run(ws, r.id);
          setState({ running: true, run: r, startedAt: began });
        }
        setState({ running: false, run: r, startedAt: began, tookMs: Date.now() - began, error: r.state === "succeeded" ? undefined : r.error || `It ${r.state.replace("_", " ")}.` });
      } catch (e) {
        setState({ running: false, startedAt: began, tookMs: Date.now() - began, error: (e as Error).message });
      } finally {
        busy.current = false;
      }
    },
    [ws],
  );
  return { ...state, start };
}
