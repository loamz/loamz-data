//! What the gateway knows, with no database and no network: which requests exist, and how names map between Loamz and `MLflow`.

use crate::governance::catalogs::model::valid_name;

/// Everything the gateway will forward. A request that is not one of these is refused, whatever `MLflow` would have done with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Version,
    ServerInfo,
    ExperimentCreate,
    ExperimentGet,
    ExperimentGetByName,
    ExperimentSearch,
    ExperimentUpdate,
    ExperimentDelete,
    ExperimentRestore,
    ExperimentSetTag,
    ExperimentDeleteTag,
    RunCreate,
    RunGet,
    RunUpdate,
    RunDelete,
    RunRestore,
    RunLogMetric,
    RunLogParameter,
    RunLogBatch,
    RunLogInputs,
    RunSetTag,
    RunDeleteTag,
    RunSearch,
    MetricHistory,
}

/// The request a method and a path (below the gateway's own address) stand for, if it is one the gateway serves.
#[must_use]
pub fn route(method: &str, path: &str) -> Option<Op> {
    let path = path.trim_matches('/');
    if path
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
        && !path.is_empty()
    {
        return None;
    }
    let tracking = path.strip_prefix("api/2.0/mlflow/");
    Some(match (method, path, tracking) {
        ("GET", "version", _) => Op::Version,
        ("GET", "api/3.0/mlflow/server-info", _) => Op::ServerInfo,
        ("POST", _, Some("experiments/create")) => Op::ExperimentCreate,
        ("GET", _, Some("experiments/get")) => Op::ExperimentGet,
        ("GET", _, Some("experiments/get-by-name")) => Op::ExperimentGetByName,
        ("POST", _, Some("experiments/search")) => Op::ExperimentSearch,
        ("POST", _, Some("experiments/update")) => Op::ExperimentUpdate,
        ("POST", _, Some("experiments/delete")) => Op::ExperimentDelete,
        ("POST", _, Some("experiments/restore")) => Op::ExperimentRestore,
        ("POST", _, Some("experiments/set-experiment-tag")) => Op::ExperimentSetTag,
        ("POST", _, Some("experiments/delete-experiment-tag")) => Op::ExperimentDeleteTag,
        ("POST", _, Some("runs/create")) => Op::RunCreate,
        ("GET", _, Some("runs/get")) => Op::RunGet,
        ("POST", _, Some("runs/update")) => Op::RunUpdate,
        ("POST", _, Some("runs/delete")) => Op::RunDelete,
        ("POST", _, Some("runs/restore")) => Op::RunRestore,
        ("POST", _, Some("runs/log-metric")) => Op::RunLogMetric,
        ("POST", _, Some("runs/log-parameter")) => Op::RunLogParameter,
        ("POST", _, Some("runs/log-batch")) => Op::RunLogBatch,
        ("POST", _, Some("runs/log-inputs")) => Op::RunLogInputs,
        ("POST", _, Some("runs/set-tag")) => Op::RunSetTag,
        ("POST", _, Some("runs/delete-tag")) => Op::RunDeleteTag,
        ("POST", _, Some("runs/search")) => Op::RunSearch,
        ("GET", _, Some("metrics/get-history")) => Op::MetricHistory,
        _ => return None,
    })
}

/// An experiment is named `catalog.schema.name`, as a table is: three plain names.
#[must_use]
pub fn parse_full_name(name: &str) -> Option<[String; 3]> {
    let mut parts = name.split('.');
    let (a, b, c) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || ![a, b, c].iter().all(|p| valid_name(p)) {
        return None;
    }
    Some([a.to_owned(), b.to_owned(), c.to_owned()])
}

/// The name `MLflow` keeps: made unique per workspace, so two workspaces can each have `main.ml.churn`.
#[must_use]
pub fn upstream_name(workspace: &str, full: &str) -> String {
    format!("{workspace}:{full}")
}

/// The name a person sees, from the name `MLflow` keeps. None: it is another workspace's.
#[must_use]
pub fn public_name(workspace: &str, upstream: &str) -> Option<String> {
    upstream
        .strip_prefix(workspace)
        .and_then(|rest| rest.strip_prefix(':'))
        .map(str::to_owned)
}

/// The most experiments one search returns, and how many upstream pages it may read to fill that.
pub const MAX_SEARCH: usize = 1000;
pub const PAGE: usize = 100;
pub const MAX_PAGES: usize = 20;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_listed_requests_exist() {
        assert_eq!(
            route("POST", "api/2.0/mlflow/experiments/create"),
            Some(Op::ExperimentCreate)
        );
        assert_eq!(route("GET", "api/2.0/mlflow/runs/get"), Some(Op::RunGet));
        assert_eq!(route("GET", "/version/"), Some(Op::Version));
        // The right path with the wrong method, anything MLflow also has, and anything that tries a trick.
        for (m, p) in [
            ("GET", "api/2.0/mlflow/experiments/create"),
            ("POST", "api/2.0/mlflow/runs/get"),
            ("DELETE", "api/2.0/mlflow/runs/get"),
            ("POST", "api/2.0/mlflow/registered-models/create"),
            ("GET", "ajax-api/2.0/mlflow/experiments/get"),
            ("POST", "api/2.0/mlflow/experiments/create/../delete"),
            ("POST", "api/2.0/mlflow/experiments//create"),
            ("GET", "api/2.0/mlflow-artifacts/artifacts"),
            ("POST", "graphql"),
            ("GET", ""),
            ("GET", "api/2.0/mlflow/experiments/get/extra"),
        ] {
            assert_eq!(route(m, p), None, "{m} {p}");
        }
    }

    #[test]
    fn names_are_three_plain_parts_and_are_kept_apart_by_workspace() {
        assert_eq!(
            parse_full_name("main.ml.churn"),
            Some(["main".into(), "ml".into(), "churn".into()])
        );
        for bad in [
            "churn",
            "main.churn",
            "a.b.c.d",
            "Main.ml.churn",
            "a..c",
            "a.b.c d",
            "",
        ] {
            assert_eq!(parse_full_name(bad), None, "{bad}");
        }
        let kept = upstream_name("w1", "main.ml.churn");
        assert_eq!(kept, "w1:main.ml.churn");
        assert_eq!(public_name("w1", &kept).as_deref(), Some("main.ml.churn"));
        assert_eq!(
            public_name("w2", &kept),
            None,
            "another workspace's experiment is not ours"
        );
        assert_eq!(
            public_name("w", "w1:main.ml.churn"),
            None,
            "a workspace id that is a prefix of another"
        );
    }
}
