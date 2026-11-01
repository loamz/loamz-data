import { useCallback, useRef, useState } from "react";
import * as api from "@/api";

export interface Statement {
  running: boolean;
  run?: api.Run;
  error?: string;
}

/** Runs a read-only statement on the workspace's compute and follows it until it ends. One at a time. */
export function useStatement(ws: string) {
  const [state, setState] = useState<Statement>({ running: false });
  const busy = useRef(false);
  const start = useCallback(
    async (sql: string, limit = 100, target?: string) => {
      if (busy.current) return;
      busy.current = true;
      setState({ running: true });
      try {
        let r = await api.runStatement(ws, sql, limit, 0, target);
        setState({ running: true, run: r });
        while (api.isRunning(r)) {
          await new Promise((ok) => setTimeout(ok, 700));
          r = await api.run(ws, r.id);
          setState({ running: true, run: r });
        }
        setState({ running: false, run: r, error: r.state === "succeeded" ? undefined : r.error || `It ${r.state.replace("_", " ")}.` });
      } catch (e) {
        setState({ running: false, error: (e as Error).message });
      } finally {
        busy.current = false;
      }
    },
    [ws],
  );
  return { ...state, start };
}
