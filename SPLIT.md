# The split from loamz-server (2026-10-27)

Both repositories were made from `loamz-server` with `git filter-repo`, so each file keeps its history.

| Before | Now |
|---|---|
| `loamz.engine` (storage) | stays `loamz.engine`, in loamz-data (`loamz-engine`) |
| `loamz.engine.agents` | `loamz.runtime`, in loamz-agents (`loamz-runtime`) |
| `loamz.server.data`, `loamz.server.reader` | stay, in loamz-data (`loamz-data-server`, `loamz-reader`) |
| `loamz.server.agents` | stays, in loamz-agents (`loamz-agents-server`) |
| `loamz.server.identity` | `loamz.identity.server`, in loamz-python (`loamz-identity[server]`) |
| `loamz.server.metrics`, `securitytxt` | a copy in each product |
| `loamz-server` combined CLI | `loamz-data-server` and `loamz-agents-server` |

Checked locally before the split was committed: loamz-data (engine 704 tests, data server 227, import rules) and loamz-agents
(runtime 1593, agents server 167, import rules) are green; loamz-identity (60 tests) is green on branch `identity-server`.

## Still to do (not done by the split)
- Create the two GitHub repositories, push, then `uv lock` in each (no `uv.lock` is committed yet: the `loamz-engine` and
  `loamz-identity[server]` git sources need the pushed branches).
- Merge the loamz-python branch `identity-server` first.
- `deploy/` (compose, Helm, Terraform, Keycloak realm, observability) still describes the combined platform in both copies.
  Product-specific parts should be pruned, and the shared parts moved to loamz-deploy.
- CI workflows were renamed mechanically; prune the jobs for the other product (reader/data-workspace in agents,
  agents-console in data) and fix the image workflows' Dockerfile list.
- Archive `loamz-server` once both pass the cross-product end-to-end run.
