//! Runs code someone wrote, with nothing it was not given.
//!
//! - `spec`: what a sandbox may have, and the OCI config that says so. Pure.
//! - `check`: reads a finished config back and refuses it unless every limit is there.
//! - `door`: the two narrow doors a write lease gives a sandbox (the catalog's routes, and signed storage requests).
//! - `exec`: starts the sandbox, waits, and kills it hard when time is up or when asked to.
//! - `work`: one piece of work: fetch what it declared with its own token, run it, say how it ended.
//! - `agent`: a machine's connection to a Data server: ask for work, do it, report.
//!
//! The limits live in the sandbox's own OCI config because gVisor's `runsc` makes its own cgroup: limits set from
//! outside (a systemd unit, for example) do not reach it.

pub mod agent;
pub mod broker;
pub mod check;
pub mod door;
pub mod exec;
pub mod fetch;
pub mod pool;
pub mod spec;
pub mod work;
pub mod worker_sandbox;
