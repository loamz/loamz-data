//! Compute: where work runs. A target is a machine an organization has attached; it connects out to this server and asks for work
//! (it never accepts connections). The work is a SQL statement or a job, run under the rights of the person who asked, narrowed to
//! what the work declared.

pub mod api;
pub mod model;
pub mod service;
pub mod sql;
mod store;
