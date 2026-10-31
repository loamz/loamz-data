//! Tables: Iceberg tables. This server is their REST catalog: it checks who may load or change one, keeps the pointer to the table's
//! current metadata file, and hands a client short-lived credentials for that table's folder only. Clients (DuckDB) write the
//! data and manifest files themselves; the server writes the metadata file, so a commit is one atomic move of the pointer.

pub mod api;
pub mod model;
pub mod service;
