export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 10 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export function when(ms: number, now = Date.now()): string {
  const s = Math.round((now - ms) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short", year: new Date(ms).getFullYear() === new Date(now).getFullYear() ? undefined : "numeric" });
}

const tried: Record<string, string> = { use: "open", read: "read", write: "write to", create: "create inside", manage: "manage" };

/** "create.volume" → "Created a volume"; a refused request is named by the privilege it needed. Unknown actions are shown as they are. */
export function describe(action: string, outcome = "ok"): string {
  if (outcome === "denied" && tried[action]) return `Tried to ${tried[action]}`;
  const words: Record<string, string> = {
    "file.read": "Read a file",
    "file.write": "Wrote a file",
    "file.delete": "Deleted a file",
    revoke: "Removed access",
    "drop.requested": "Started a drop",
    "drop.done": "Finished a drop",
    "drop.failed": "A drop failed",
    "erase.requested": "Started an erase",
    "erase.done": "Finished an erase",
    "erase.failed": "An erase failed",
  };
  if (words[action]) return words[action];
  const [verb, rest] = action.split(".");
  if (verb === "create" && rest) return `Created a ${rest}`;
  if (verb === "grant" && rest) return `Gave ${rest} access`;
  return action;
}

/** The pieces of an audit target: "volume <id>/a/b.csv", "catalog <id> <- bob", "tenant", "subject <id>". */
export function parseTarget(target: string): { kind: string; id: string; path: string; rest: string } {
  const m = /^(\w+) ([0-9a-f-]{36})((?:\/\S*)?)(.*)$/.exec(target);
  return m ? { kind: m[1]!, id: m[2]!, path: m[3]!, rest: m[4]!.trim() } : { kind: target, id: "", path: "", rest: "" };
}

/** What to show for a target: the name when it is known, else the start of its id. */
export function shortTarget(target: string, names: Record<string, string> = {}): string {
  const t = parseTarget(target);
  if (!t.id) return target;
  const name = names[t.id] ?? `${t.id.slice(0, 8)}…`;
  return `${t.kind} ${name}${t.path}${t.rest ? ` ${t.rest.replace(/[0-9a-f]{8}-[0-9a-f-]{27}/g, (x) => x.slice(0, 8))}` : ""}`;
}
