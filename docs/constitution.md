# Constitution

Every change to Loamz Data obeys these rules. A change that breaks one changes this file first, with the owner's agreement.

## Purpose
**A fast, governed, modern data platform for the AI era and for traditional data people.**

| Who | What they need from us |
|---|---|
| Analysts, data engineers, BI users | SQL on their tables, answers in seconds or less, pipelines that cost little |
| AI builders: agents, AI apps, ML | the same data, queried by code and models, always as the person and never more |
| The business paying for it | open formats (no lock-in), one place for access and audit, a low cost per query |

Why it exists: today a team picks *fast* (a local engine with no governance) or *governed* (a heavy platform, slow and costly).
Loamz Data is both. Speed comes from design, and every change is measured.

**Every feature serves one of these users and says which. If it serves none, it is not built.**

## Speed
1. Don't move data: send work to the data, read in place, never copy a whole table.
2. Read less: prune partitions, files, row groups and columns before reading.
3. Never compute twice: snapshots and files are immutable, so caches keyed by them are safe.
4. Stay columnar: Parquet at rest, Arrow on the wire, no row-by-row conversion.
5. Keep the engine warm: no process start per query.
6. Keep governance off the hot path: milliseconds per query, no polling, no global lock.

## Boundaries
- Data calls no other product. Agents and Chat are its customers.
- Every write goes through Loamz's catalog. Outside tools may read through the Iceberg REST catalog.
- Only a SQL database (SQLite or Postgres) and a byte store (folder or S3-compatible). No other storage engines.

## Governance
- A caller who may not see a thing gets 404, never 403. Every refusal is audited.
- Every change and its audit entry are one database transaction.
- Only `iam` decides who may do what. Credentials cover exactly what a run reads, for minutes.
- Erase and drop are durable tasks: lease, retry, verify, audit.

## Engineering
- No backward compatibility: delete old code instead of keeping it alive.
- Strict types. Plain, readable code: named steps, ordinary loops, no SQL built by joining strings.
- Every database test runs on SQLite and Postgres.
- Two developers: the smallest change that removes the most work. Nothing is built "for later".

## Documents
A document exists only if someone needs it to work: this file, `decisions.md`, and one folder per piece of work in `specs/`
(`spec.md`, `design.md`, `tasks.md`). Small changes get no document.
