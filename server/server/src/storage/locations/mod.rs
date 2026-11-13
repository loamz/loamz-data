//! Storage settings of a workspace: credentials (how the server reaches a store) and locations (the part of it a workspace may
//! use). Workspace admins manage them; the bytes themselves are reached through governance.

pub mod api;
pub mod model;
pub mod service;
mod store;
