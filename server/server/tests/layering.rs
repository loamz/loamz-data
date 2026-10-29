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
                !body.contains("governance::") && !body.contains("storage::"),
                "{path}: the kernel knows no feature"
            );
        }
    }
}

#[test]
fn services_form_one_line_with_no_loop() {
    // audit < iam < catalogs < volumes < lifecycle: a service may use only those below it.
    let order = ["audit", "iam", "catalogs", "volumes", "lifecycle"];
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
