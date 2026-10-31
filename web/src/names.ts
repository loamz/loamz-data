import { useEffect, useState } from "react";
import { get } from "./api";
import { parseTarget } from "./format";

const known = new Map<string, string>();

/** Names for the things an audit entry points at, looked up once each. A thing that has been dropped, or that you may not see, keeps its short id. */
export function useNames(targets: string[]): Record<string, string> {
  const [, bump] = useState(0);
  const key = targets.join("\n");
  useEffect(() => {
    const ids = [...new Set(targets.map((t) => parseTarget(t).id).filter((id) => id && !known.has(id)))].slice(0, 40);
    if (ids.length === 0) return;
    let live = true;
    void Promise.allSettled(ids.map((id) => get(id).then((s) => known.set(id, s.name)))).then(() => live && bump((n) => n + 1));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return Object.fromEntries(known);
}
