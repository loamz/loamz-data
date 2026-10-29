//! Audit: who did what to which thing, with what result. Append-only and hash-chained; survives an erase.

pub mod api;
pub mod model;
pub mod service;
mod store;
