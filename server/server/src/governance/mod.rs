//! Governance: the only door. Every request is authenticated, authorized by IAM, and recorded.
//!
//! Services form one line (a service uses only those before it): audit, orgs, workspaces, iam, catalogs, volumes (in
//! `storage`), lifecycle. `context` works out what a request is about, for the api files.

pub mod audit;
pub mod catalogs;
pub mod context;
pub mod iam;
pub mod lifecycle;
pub mod orgs;
pub mod workspaces;
