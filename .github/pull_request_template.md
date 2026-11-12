## What and why

<!-- One or two sentences. Link the issue or ADR. The PR title must be a conventional commit: `type(scope): subject`. -->

## Checklist

- [ ] Tests added or updated, and they pass locally for the packages I touched
- [ ] Contract change (anything under `contracts/` or a public API schema)? I ran `oasdiff` and any breaking change is intentional and versioned (`/api/v1`)
- [ ] Docs updated (README, package docs, changelog is automatic via release-please)
- [ ] A decision? One line in `docs/decisions.md`. A change to a boundary, format, the API or the query path? Its RFC in `docs/rfcs/` is agreed first.
- [ ] No secrets, tokens, `.env` files or private keys in the diff
