//! Runs code an agent wrote, with nothing it was not given.
//!
//! - `spec`: what a sandbox may have, and the OCI config that says so. Pure.
//! - `check`: reads a finished config back and refuses it unless every limit is there.
//! - `exec`: starts the sandbox, waits, and kills it hard when time is up.
//!
//! The limits live in the sandbox's own OCI config because gVisor's `runsc` makes its own cgroup: limits set from
//! outside (a systemd unit, for example) do not reach it.

pub mod check;
pub mod exec;
pub mod spec;
