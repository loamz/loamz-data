// The optional link from Home to an agents server. The browser calls it with the person's own token; Data's server knows nothing of it.
// Without VITE_AGENTS_URL there is no ask box and Data works as before.

import { eventsOf, type WireEvent } from "./conversation";
import { token } from "./auth";

export const agentsUrl: string | null = ((import.meta.env.VITE_AGENTS_URL as string | undefined) ?? "").replace(/\/$/, "") || null;

/** The body of `POST /api/v1/chat`: one message, the workspace, and the catalogs the person narrowed the question to (none: all they may read). */
export function chatRequest(workspace: string, text: string, catalogs: readonly string[], thread: string) {
  return {
    thread_id: thread,
    messages: [{ role: "user", content: text }],
    data: { workspace, catalogs: [...catalogs] },
  };
}

export async function* ask(base: string, workspace: string, text: string, catalogs: readonly string[], thread: string, signal?: AbortSignal): AsyncGenerator<WireEvent> {
  const t = await token();
  if (!t) throw new Error("Sign in to continue.");
  const init: RequestInit = {
    method: "POST",
    headers: { Authorization: `Bearer ${t}`, "Content-Type": "application/json", Accept: "text/event-stream" },
    body: JSON.stringify(chatRequest(workspace, text, catalogs, thread)),
  };
  if (signal) init.signal = signal;
  yield* eventsOf(await fetch(`${base}/api/v1/chat`, init));
}
