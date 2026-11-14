//! The `MLflow` gateway: the standard `mlflow` client points at `/api/v1/workspaces/{ws}/mlflow` with a Loamz access token, and this
//! decides what each request may do before an upstream `MLflow` server (which is never reachable otherwise) sees it. Deny by default: only
//! the requests in [`model::route`] exist.

pub mod api;
pub mod model;
pub mod service;
mod store;
