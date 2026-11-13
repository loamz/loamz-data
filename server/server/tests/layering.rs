//! The layering rules, checked on the source so they cannot drift:
//! - `model.rs` is pure: no database, no web, no files, no clock.
//! - Only a feature's own `service.rs` reads its `store.rs`; other features go through its service.
//! - The kernel knows no feature.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{fs, path::Path};

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

fn all() -> Vec<(String, String)> {
    let mut out = Vec::new();
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    assert!(out.len() > 20, "found the sources");
    out
}

/// Code only, without comments, so a sentence about `sqlx` does not count as using it.
fn code(src: &str) -> String {
    src.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn models_are_pure() {
    let banned = [
        "sqlx",
        "axum",
        "tokio",
        "std::fs",
        "std::time",
        "now_ms",
        "loamz_lake",
        "reqwest",
    ];
    for (path, src) in all().into_iter().filter(|(p, _)| p.ends_with("/model.rs")) {
        // Only the part before the tests counts; tests may do what they like.
        let body = code(src.split("#[cfg(test)]").next().unwrap());
        for word in banned {
            assert!(
                !body.contains(word),
                "{path} is a model and must not use {word}"
            );
        }
    }
}

#[test]
fn only_a_features_own_service_reads_its_store() {
    for (path, src) in all() {
        let body = code(&src);
        // `store` is a private module of each feature, so the compiler already stops outsiders; this also stops a
        // feature's api from skipping its service.
        if path.ends_with("/api.rs") {
            assert!(
                !body.contains("store::"),
                "{path}: an api goes through the service, not the store"
            );
            assert!(!body.contains("sqlx"), "{path}: an api holds no SQL");
        }
        if path.contains("/kernel/") && !path.ends_with("/system.rs") {
            assert!(
                !body.contains("governance::")
                    && !body.contains("storage::")
                    && !body.contains("compute::"),
                "{path}: the kernel knows no feature"
            );
        }
    }
}

#[test]
fn services_form_one_line_with_no_loop() {
    // audit < orgs < workspaces < locations < iam < catalogs < volumes < tables < compute < queries < lifecycle: a service may use only those before it.
    let order = [
        "audit",
        "orgs",
        "workspaces",
        "locations",
        "iam",
        "catalogs",
        "volumes",
        "tables",
        "compute",
        "queries",
        "lifecycle",
    ];
    let mut checked = 0;
    for (path, src) in all()
        .into_iter()
        .filter(|(p, _)| p.ends_with("/service.rs"))
    {
        let me = order
            .iter()
            .position(|o| path.contains(&format!("/{o}/")))
            .unwrap_or_else(|| panic!("{path} is not in the order"));
        let body = code(&src);
        for above in &order[me + 1..] {
            assert!(
                !body.contains(&format!("{above}::")),
                "{path} must not use {above}, which is above it"
            );
        }
        checked += 1;
    }
    assert_eq!(checked, order.len());
}

/// Data works without Agents, and never depends on it: Agents is a customer of Data's public API. The only link is configuration
/// (the names of clients whose tokens are an agent acting for a person), which is a string, not a dependency.
#[test]
fn data_depends_on_no_agents_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let is_agents = |name: &str| name.to_ascii_lowercase().contains("agents");

    // No Rust crate of ours or anyone's with "agents" in its name is a dependency.
    for manifest in [
        "Cargo.toml",
        "server/lake/Cargo.toml",
        "server/runner/Cargo.toml",
        "server/server/Cargo.toml",
    ] {
        let text = fs::read_to_string(root.join(manifest)).unwrap();
        let mut in_dependencies = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_dependencies = line.contains("dependencies");
            } else if in_dependencies {
                let name = line.split(['=', '.']).next().unwrap_or("").trim();
                assert!(
                    !is_agents(name),
                    "{manifest} depends on {name}: Data must not depend on Agents"
                );
            }
        }
    }

    // No source file reaches for an Agents crate.
    for crate_dir in ["server/lake/src", "server/runner/src", "server/server/src"] {
        let mut files = Vec::new();
        sources(&root.join(crate_dir), &mut files);
        for (path, src) in files {
            assert!(
                !code(&src).contains("loamz_agents"),
                "{path} uses an Agents crate: Data must not depend on Agents"
            );
        }
    }

    // The web app does not depend on an Agents package either (talking to an Agents address, when one is configured, is a call, not a dependency).
    let package: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("web/package.json")).unwrap()).unwrap();
    for section in ["dependencies", "devDependencies"] {
        for name in package[section]
            .as_object()
            .into_iter()
            .flat_map(|m| m.keys())
        {
            assert!(
                !is_agents(name),
                "web/package.json depends on {name}: Data must not depend on Agents"
            );
        }
    }
}
