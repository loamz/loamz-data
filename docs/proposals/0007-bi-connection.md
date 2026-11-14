# Proposal 0007: the BI connection (Arrow Flight SQL)

Status: first part built 14 Nov 2026 (workstream W11 in [0003](0003-beta.md)).

## Decision

A BI tool, a notebook or any ADBC or JDBC client reaches Loamz Data over **Arrow Flight SQL**, on its own port (`LOAMZ_DATA_FLIGHT_LISTEN`,
off unless set). It does nothing new about access: it asks the services that already decide.

- **Sign in:** user name = the workspace's id, password = an access token (a person's, or a service principal's). The answer is a bearer
  token the client sends back (`<workspace>~<token>`). The token is checked on every call by the same code the HTTP API uses
  (`governance::context::identity_of`), so a token means the same thing everywhere.
- **A statement is a run:** authorized as the caller, rewritten with row filters and column masks, audited, run on the workspace's
  compute with its whole result kept as an Arrow file (proposal 0006), then sent to the client as Flight batches.
- **Lists:** catalogs, schemas and tables use the catalog screens' visibility rule: what a caller may not see is not there.

## Not yet

Prepared statements, table schemas in the table list, statement updates (the connection is read-only), `GetSqlInfo`, TLS on the
port (run it behind a TLS-terminating proxy or inside the private network), a statement that runs longer than two minutes (the client is
told to ask again), and sending the result by a signed address instead of through this server.
