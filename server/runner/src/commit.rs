//! Commits what a job left in `/workspace/out` to the person's volume, and says exactly which files made it.
//!
//! The sandbox is untrusted and the runner is the boundary between it and persistent storage, so the folder is read as carefully as
//! it can be: every open is relative to an already open directory and resolved with `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`,
//! so a link planted in the folder, or a folder swapped for a link while this runs, cannot lead out of it. Only regular files are
//! taken (checked on the open file, not on its name), and names must be plain. Each file goes to a path made for this run, with
//! "create only" (`If-None-Match: *`) so nothing is ever overwritten. Files are committed one by one, not as one transaction: a failed
//! run can leave the files that did make it, and the report lists both.

use std::{fs::File, os::fd::OwnedFd, path::Path, sync::Arc};

use rustix::fs::{Dir, FileType, Mode, OFlags, ResolveFlags, fstat, open, openat2};
use serde::Serialize;
use serde_json::Value;

/// How many files, how large each, and how large all together, a run may commit.
pub const MAX_FILES: usize = 200;
pub const MAX_FILE: u64 = 256 << 20;
pub const MAX_TOTAL: u64 = 1 << 30;
const MAX_DEPTH: usize = 4;
/// Up to this size a file is sent in one request with the "create only" condition; a larger one is sent in parts (its path is
/// this run's own, so nothing else can be there).
const ONE_REQUEST: u64 = 64 << 20;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Committed {
    /// Where it is in the volume.
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct Outcome {
    pub committed: Vec<Committed>,
    /// Files that were not committed, and why (a sentence for the person).
    pub failed: Vec<String>,
}

/// A file found in the output folder, opened safely.
struct Found {
    name: String,
    file: File,
    size: u64,
}

fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '))
}

fn beneath() -> ResolveFlags {
    ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS
}

/// Lists the regular files under `dir`, by names relative to it. Anything else (links, devices, sockets, too-deep folders, odd names)
/// is reported and skipped, never followed.
fn walk(
    dir: &OwnedFd,
    prefix: &str,
    depth: usize,
    found: &mut Vec<Found>,
    skipped: &mut Vec<String>,
) {
    let Ok(listing) = Dir::read_from(dir) else {
        skipped.push(format!("{prefix}: the folder could not be read"));
        return;
    };
    for entry in listing.flatten() {
        let raw = entry.file_name().to_bytes();
        if raw == b"." || raw == b".." {
            continue;
        }
        let Ok(name) = std::str::from_utf8(raw) else {
            skipped.push(format!("{prefix}: a name that is not text"));
            continue;
        };
        let shown = format!("{prefix}{name}");
        if !plain(name) {
            skipped.push(format!(
                "{shown}: the name has characters a file name may not have here"
            ));
            continue;
        }
        match entry.file_type() {
            FileType::RegularFile => {
                let opened = openat2(
                    dir,
                    entry.file_name(),
                    OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                    Mode::empty(),
                    beneath(),
                );
                let Ok(fd) = opened else {
                    skipped.push(format!("{shown}: it could not be opened safely"));
                    continue;
                };
                // The type is checked on the open file: a name that changed after the listing cannot pass for a regular file.
                let Ok(stat) = fstat(&fd) else {
                    skipped.push(format!("{shown}: it could not be inspected"));
                    continue;
                };
                if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
                    skipped.push(format!("{shown}: only regular files are kept"));
                    continue;
                }
                found.push(Found {
                    name: shown,
                    size: u64::try_from(stat.st_size).unwrap_or(u64::MAX),
                    file: File::from(fd),
                });
            }
            FileType::Directory if depth < MAX_DEPTH => {
                let opened = openat2(
                    dir,
                    entry.file_name(),
                    OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                    Mode::empty(),
                    beneath(),
                );
                match opened {
                    Ok(sub) => walk(&sub, &format!("{shown}/"), depth + 1, found, skipped),
                    Err(_) => {
                        skipped.push(format!("{shown}: the folder could not be opened safely"));
                    }
                }
            }
            FileType::Directory => skipped.push(format!(
                "{shown}: folders are kept at most {MAX_DEPTH} deep"
            )),
            _ => skipped.push(format!(
                "{shown}: only regular files are kept (not links or devices)"
            )),
        }
    }
}

/// The files in the output folder, up to the limits. What was left out and why is in the second answer.
///
/// # Errors
/// The folder cannot be opened.
fn collect(out: &Path) -> Result<(Vec<Found>, Vec<String>), String> {
    let root = open(
        out,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|e| format!("the output folder could not be opened: {e}"))?;
    let (mut found, mut skipped) = (Vec::new(), Vec::new());
    walk(&root, "", 0, &mut found, &mut skipped);
    found.sort_by(|a, b| a.name.cmp(&b.name));
    let mut kept = Vec::new();
    let mut total = 0_u64;
    for f in found {
        if kept.len() == MAX_FILES {
            skipped.push(format!("{}: more than {MAX_FILES} files", f.name));
        } else if f.size > MAX_FILE {
            skipped.push(format!("{}: larger than {} MiB", f.name, MAX_FILE >> 20));
        } else if total.saturating_add(f.size) > MAX_TOTAL {
            skipped.push(format!(
                "{}: the files together are over {} MiB",
                f.name,
                MAX_TOTAL >> 20
            ));
        } else {
            total += f.size;
            kept.push(f);
        }
    }
    Ok((kept, skipped))
}

/// Puts one file in the store under `key`, which must not exist yet.
async fn put(
    store: &Arc<dyn object_store::ObjectStore>,
    key: &object_store::path::Path,
    file: File,
    size: u64,
) -> Result<(), String> {
    use object_store::{PutMode, PutOptions, PutPayload, buffered::BufWriter};
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::from_std(file);
    if size <= ONE_REQUEST {
        let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
        tokio::io::AsyncReadExt::read_to_end(&mut file, &mut bytes)
            .await
            .map_err(|e| e.to_string())?;
        let options = PutOptions {
            mode: PutMode::Create,
            ..PutOptions::default()
        };
        store
            .put_opts(key, PutPayload::from(bytes), options)
            .await
            .map(|_| ())
            .map_err(|e| match e {
                object_store::Error::AlreadyExists { .. } => {
                    "a file with this name was already committed".to_owned()
                }
                _ => "the storage did not accept it".to_owned(),
            })
    } else {
        let mut writer = BufWriter::new(Arc::clone(store), key.clone());
        tokio::io::copy(&mut file, &mut writer)
            .await
            .map_err(|_| "the storage did not accept it".to_owned())?;
        writer
            .shutdown()
            .await
            .map_err(|_| "the storage did not accept it".to_owned())
    }
}

/// Commits the output folder with the credential in `grant` (the lease's `commit` block). Never errors: everything that went wrong is
/// in the outcome, so the report can say which files are in the volume and which are not.
pub async fn commit(grant: &Value, out: &Path) -> Outcome {
    let mut outcome = Outcome::default();
    let text = |k: &str| grant[k].as_str().map(str::to_owned);
    let (
        Some(endpoint),
        Some(bucket),
        Some(key_id),
        Some(secret),
        Some(token),
        Some(keys),
        Some(paths),
    ) = (
        text("endpoint"),
        text("bucket"),
        text("key_id"),
        text("secret"),
        text("session_token"),
        text("key_prefix"),
        text("path_prefix"),
    )
    else {
        outcome
            .failed
            .push("the lease has no way to store outputs".into());
        return outcome;
    };
    let out = out.to_owned();
    let listed = tokio::task::spawn_blocking(move || collect(&out)).await;
    let (files, skipped) = match listed {
        Ok(Ok(found)) => found,
        Ok(Err(why)) => {
            outcome.failed.push(why);
            return outcome;
        }
        Err(_) => {
            outcome
                .failed
                .push("the output folder could not be read".into());
            return outcome;
        }
    };
    outcome.failed.extend(skipped);
    if files.is_empty() {
        return outcome;
    }
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let built = object_store::aws::AmazonS3Builder::new()
        .with_endpoint(&endpoint)
        .with_allow_http(endpoint.starts_with("http://"))
        .with_bucket_name(&bucket)
        .with_access_key_id(&key_id)
        .with_secret_access_key(&secret)
        .with_token(&token)
        .with_region(grant["region"].as_str().unwrap_or("auto"))
        // "Create only" is sent as `If-None-Match: *`, which the store must honour.
        .with_conditional_put(object_store::aws::S3ConditionalPut::ETagMatch)
        .build();
    let Ok(s3) = built else {
        outcome
            .failed
            .push("the storage access could not be prepared".into());
        return outcome;
    };
    let store: Arc<dyn object_store::ObjectStore> = Arc::new(s3);
    for f in files {
        let key =
            object_store::path::Path::from(format!("{}/{}", keys.trim_end_matches('/'), f.name));
        match put(&store, &key, f.file, f.size).await {
            Ok(()) => outcome.committed.push(Committed {
                path: format!("{}/{}", paths.trim_end_matches('/'), f.name),
                bytes: f.size,
            }),
            Err(why) => outcome.failed.push(format!("{}: {why}", f.name)),
        }
    }
    outcome
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn names(found: &[Found]) -> Vec<&str> {
        found.iter().map(|f| f.name.as_str()).collect()
    }

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("loamz-commit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_plain_regular_files_are_taken_and_links_are_never_followed() {
        let root = temp("walk");
        let secret = temp("walk-secret");
        std::fs::write(secret.join("passwd"), "root").unwrap();
        let out = root.join("out");
        std::fs::create_dir_all(out.join("charts")).unwrap();
        std::fs::write(out.join("summary.csv"), "a,b\n1,2\n").unwrap();
        std::fs::write(out.join("charts").join("one.png"), "png").unwrap();
        // A link to a host file, a link to a host folder, a link inside a folder, and a name that is not plain.
        symlink(secret.join("passwd"), out.join("leak")).unwrap();
        symlink(&secret, out.join("folder-link")).unwrap();
        symlink(secret.join("passwd"), out.join("charts").join("deep-leak")).unwrap();
        std::fs::write(out.join("bad;name"), "x").unwrap();
        let (found, skipped) = collect(&out).unwrap();
        assert_eq!(names(&found), ["charts/one.png", "summary.csv"]);
        let said = skipped.join("\n");
        for left_out in ["leak", "folder-link", "charts/deep-leak", "bad;name"] {
            assert!(
                said.contains(left_out),
                "{left_out} should be reported: {said}"
            );
        }
        assert!(!said.contains("root"), "no host content is read");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(secret);
    }

    #[test]
    fn the_limits_hold() {
        let out = temp("limits");
        for i in 0..(MAX_FILES + 5) {
            std::fs::write(out.join(format!("f{i:04}.txt")), "x").unwrap();
        }
        let (found, skipped) = collect(&out).unwrap();
        assert_eq!(found.len(), MAX_FILES);
        assert_eq!(skipped.len(), 5);
        let deep = out.join("a").join("b").join("c").join("d").join("e");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("too-deep.txt"), "x").unwrap();
        let (_, skipped) = collect(&out).unwrap();
        assert!(skipped.iter().any(|s| s.contains("deep")), "{skipped:?}");
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn a_folder_that_is_itself_a_link_cannot_be_opened() {
        let root = temp("rootlink");
        let target = temp("rootlink-target");
        std::fs::write(target.join("x"), "x").unwrap();
        let link = root.join("out");
        symlink(&target, &link).unwrap();
        assert!(collect(&link).is_err());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(target);
    }
}
