import { useRef, useState } from "react";
import { Button, Select } from "@/design";
import { ask } from "@/agents";
import { follow, newTurn, Thread, type Exchange } from "@/conversation";

const ALL = "";

/** "Ask anything" on Home: a question in words; the agent looks at the catalogs (all the person may read, or the one chosen), runs SQL on the
 * workspace's compute and answers. It acts as the person, so it sees only what they may see. */
export function Ask({ base, ws, catalogs }: { base: string; ws: string; catalogs: string[] }) {
  const [text, setText] = useState("");
  const [catalog, setCatalog] = useState(ALL);
  const [exchanges, setExchanges] = useState<Exchange[]>([]);
  const [busy, setBusy] = useState(false);
  const thread = useRef(crypto.randomUUID());
  const stop = useRef<AbortController | null>(null);

  async function send() {
    const question = text.trim();
    if (!question || busy) return;
    setText("");
    setBusy(true);
    const mine = new AbortController();
    stop.current = mine;
    const put = (turn: Exchange["turn"]) => setExchanges((all) => [...all.slice(0, -1), { ask: question, turn }]);
    setExchanges((all) => [...all, { ask: question, turn: newTurn() }]);
    try {
      await follow(ask(base, ws, question, catalog === ALL ? [] : [catalog], thread.current, mine.signal), put);
    } catch (e) {
      put({ ...newTurn(), status: mine.signal.aborted ? "cancelled" : "failed", error: e instanceof Error ? e.message : "Something went wrong." });
    } finally {
      setBusy(false);
      stop.current = null;
    }
  }

  return (
    <section className="flex flex-col gap-3" aria-label="Ask anything">
      {exchanges.length > 0 && <Thread exchanges={exchanges} />}
      <form
        className="flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-2"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <input
          className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted"
          placeholder="Ask anything about your data"
          aria-label="Ask anything"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <div className="w-40 shrink-0">
          <Select
            size="sm"
            aria-label="Catalogs the agent may look at"
            value={catalog}
            onValueChange={setCatalog}
            options={[{ value: ALL, label: "All catalogs" }, ...catalogs.map((c) => ({ value: c, label: c }))]}
          />
        </div>
        {busy ? (
          <Button type="button" size="sm" variant="secondary" onClick={() => stop.current?.abort()}>
            Stop
          </Button>
        ) : (
          <Button type="submit" size="sm" disabled={!text.trim()}>
            Ask
          </Button>
        )}
      </form>
    </section>
  );
}
