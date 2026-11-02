# An agent on the workspace Home

- **Status:** design, with slice 1 built (on the Agents side) and tested. Slices 2 to 5 are not started.
- **Applies to:** `loamz-data/web` (Home), `loamz-agents` (chat API and data tools), `loamz-chat/web` (the shared conversation UI), `loamz-deploy` (identity and routing).

## What is wanted

On a workspace's Home, an "Ask anything" box like Databricks has: ask in words, the agent looks at the catalog, runs SQL on the workspace's compute and shows rows. A catalog filter beside the box narrows which catalogs the agent may look at. The sample data is `nyc.main.yellow_trips` (one week of NYC taxi trips, `scripts/nyc_week.py`).

## The rule that shapes it: Data does not depend on Agents

The dependency points one way, from the web app and from Agents to Data's public API, never back.

- **Data server:** no agent code and no agent concept. It offers catalog, SQL statements, sandboxed jobs and scoped tokens. The only link is the existing allowlist of client names (`LOAMZ_DATA_AGENT_CLIENTS`), which is configuration: an agent's token is the person's rights narrowed to what the agent may also do.
- **Agents server:** owns conversations, the model and the tool loop. It reaches Data as a client, with a credential obtained for the person at each call (token exchange), so it holds none.
- **Data web:** shows the ask box only when an Agents address is configured. Without one, Home is what it is now (Recents and Catalogs) and Data works alone.
- **Checks to add:** a test that the Data server crate depends on no agents crate, and a web test that Home renders with no Agents address.

## What already exists (read in the code, 2 November 2026)

- `POST /api/v1/chat` on the Agents server streams a turn as server-sent events (`start_chat` in `features/conversations/chat/api.py`); threads, cancel and approvals are separate calls.
- The runtime has three data tools, `data_tables`, `data_describe` and `data_sql` (`loamz/runtime/data/tools.py`), and a code interpreter whose sandbox reaches Data only through a socket the host owns, so it never holds a credential (`source.py`).
- The workspace comes from the run's user attributes (`loamz.data/workspace`). Before slice 1 nothing in the HTTP layer set them, so the tools could only use one workspace configured for the whole server.
- The conversation UI today is the console inside Chat: `app/Console.tsx` (2,595 lines) built on `chat/Thread`, `chat/Bubble` and `Composer`, which Chat already shares between person, agent and group chats.

## Slices

1. **Built.** A chat names the workspace and catalogs: `ChatRequest.data = {workspace, catalogs[]}`, validated at the edge and passed into the run as user attributes. The three tools honour the catalogs. Tests in `loamz-agents`: 330 pass (1 skipped), and the data tool tests pass against the local rewritten client.
   - The catalog choice narrows what the agent is shown and may name. It is not a boundary: Data decides what the person may read, and a choice can only take away from that.
   - It does not yet narrow the code interpreter's `sd.sql(...)`. That path needs the gateway's policy to read the statement.
2. **Identity and routing.** The Data web's access token must be accepted by Agents (an audience mapper on its Keycloak client, in the realm template), and the browser must reach Agents from the workspace origin (a same-origin path through the dev proxy and the edge, or CORS for `*.data.loamz.com`).
3. **The conversation UI, shared.** Do not copy the console. Move what a conversation needs (thread, message bubble, composer, the tool-call and approval cards, the event-stream reader) out of Chat into the part that is already copied between products, then use it from Data. The console's own screens (agents, groups, settings, marketplace) stay in Chat.
4. **Home.** The ask box, the catalog filter (a choice among the catalogs the person can see), and an answer that shows the SQL it ran and its rows.
5. **Checks.** The independence tests above, and one end-to-end run with a real model against the NYC week.

## Known gaps to close along the way

- **Client version.** The Agents lock pins an older `loamz-data` client from GitHub. The data tools need the rewritten one (`AsyncDataClient(workspace=...)`), which is committed in loamz-python but not pushed. Until then those tests fail in a plain `uv run` and pass only against the local source.
- **Listing tables is a crawl.** `client.tables()` asks once per catalog and schema. A flat "tables I may read" call on Data would make every agent turn cheaper.
- **Two dev identity providers.** Data's dev Keycloak (port 8082) and Chat's differ. Running the whole path locally means the Agents server trusts Data's realm, with the `loamz-agents` exchange client from `deploy/dev/agents_client.py`.
- **Compute must be running.** A statement is refused on a stopped warehouse, and nothing starts it for an agent either. The ask box should say so and offer the person who may start it the same Start action.
- **Model.** Dev uses the real model in `~/.local/share/loamz-dev/model.env`; the product has no stand-in model.
