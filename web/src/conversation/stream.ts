// The Agents server streams a turn as server-sent events: `event: <type>` and `data: <json>` lines, a blank line between events, and a
// last `data: [DONE]`. This reads that from a fetch Response. A `ping` is dropped; a malformed event is an error, never skipped silently.

export type WireEvent =
  | { type: "text.delta"; text: string }
  | { type: "reasoning.delta"; text: string }
  | { type: "tool.call"; call_id: string; tool_name: string; risk?: string | null }
  | { type: "tool.result"; call_id: string; tool_name: string; ok: boolean; output: string; error?: string | null }
  | { type: "approval.requested"; [key: string]: unknown }
  | { type: "turn.completed"; text?: string }
  | { type: "run.completed" }
  | { type: "run.failed"; error: string; code?: string | null }
  | { type: "run.cancelled" }
  | { type: "protocol.hello"; version?: string };

/** Splits a stream of text chunks into the `data:` payloads of its events. */
export async function* sseData(chunks: AsyncIterable<string>): AsyncGenerator<string> {
  let buffer = "";
  for await (const chunk of chunks) {
    buffer += chunk.replaceAll("\r\n", "\n");
    let end: number;
    while ((end = buffer.indexOf("\n\n")) >= 0) {
      const block = buffer.slice(0, end);
      buffer = buffer.slice(end + 2);
      const data = block
        .split("\n")
        .filter((l) => l.startsWith("data:"))
        .map((l) => l.slice(5).trimStart())
        .join("\n");
      if (data) yield data;
    }
  }
}

async function* decoded(body: ReadableStream<Uint8Array>): AsyncGenerator<string> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      yield decoder.decode(value, { stream: true });
    }
  } finally {
    reader.releaseLock();
  }
}

export async function* events(chunks: AsyncIterable<string>): AsyncGenerator<WireEvent> {
  for await (const data of sseData(chunks)) {
    if (data === "[DONE]") return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(data);
    } catch {
      throw new Error("the server sent an event that is not JSON");
    }
    const type = (parsed as { type?: unknown } | null)?.type;
    if (typeof type !== "string") throw new Error("the server sent an event without a type");
    if (type === "ping") continue;
    yield parsed as WireEvent;
  }
}

/** The events of a response from `POST /api/v1/chat`. A refusal (not 2xx) is an error carrying the problem document's `detail`. */
export async function* eventsOf(response: Response): AsyncGenerator<WireEvent> {
  if (!response.ok) {
    let detail = `the server answered ${response.status}`;
    try {
      const problem = (await response.json()) as { detail?: unknown; title?: unknown };
      detail = String(problem.detail ?? problem.title ?? detail);
    } catch {
      /* no problem document */
    }
    throw new Error(detail);
  }
  if (!response.body) throw new Error("the server sent no stream");
  yield* events(decoded(response.body));
}
