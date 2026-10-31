//! IAM: who holds which privileges where (Unity Catalog's names). Roles are bundles of grants. People, groups and service
//! principals are all principals.

pub mod api;
pub mod model;
pub mod service;
mod store;
