//! The session policy that limits issued credentials to some folders of one bucket. Pure: facts in, JSON out.
//!
//! The policy has no `Id` element: `RustFS` rejects one. Reading needs `GetObject` on the folders and `ListBucket` limited to their
//! prefixes; writing adds what an Iceberg writer uses (put, delete, multipart).

use serde_json::{Value, json};

const READ: &[&str] = &["s3:GetObject"];
const WRITE: &[&str] = &[
    "s3:GetObject",
    "s3:PutObject",
    "s3:DeleteObject",
    "s3:AbortMultipartUpload",
    "s3:ListMultipartUploadParts",
];

/// The policy for `folders` (each `<id>/`) of `bucket`.
#[must_use]
pub fn session_policy(bucket: &str, folders: &[String], write: bool) -> Value {
    let objects: Vec<String> = folders
        .iter()
        .map(|f| format!("arn:aws:s3:::{bucket}/{f}*"))
        .collect();
    let prefixes: Vec<String> = folders.iter().map(|f| format!("{f}*")).collect();
    json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Action": if write { WRITE } else { READ },
                "Resource": objects,
            },
            {
                "Effect": "Allow",
                "Action": ["s3:ListBucket"],
                "Resource": [format!("arn:aws:s3:::{bucket}")],
                "Condition": { "StringLike": { "s3:prefix": prefixes } },
            },
            {
                "Effect": "Allow",
                "Action": ["s3:GetBucketLocation"],
                "Resource": [format!("arn:aws:s3:::{bucket}")],
            },
        ],
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn folders() -> Vec<String> {
        vec!["a1/".into(), "b2/".into()]
    }

    #[test]
    fn reading_names_the_folders_and_only_reads() {
        let p = session_policy("data", &folders(), false);
        assert_eq!(p["Statement"][0]["Action"], json!(["s3:GetObject"]));
        assert_eq!(
            p["Statement"][0]["Resource"],
            json!(["arn:aws:s3:::data/a1/*", "arn:aws:s3:::data/b2/*"])
        );
        assert_eq!(
            p["Statement"][1]["Condition"]["StringLike"]["s3:prefix"],
            json!(["a1/*", "b2/*"])
        );
    }

    #[test]
    fn writing_adds_put_delete_and_multipart() {
        let p = session_policy("data", &folders(), true);
        let actions = p["Statement"][0]["Action"].as_array().expect("a list");
        for a in ["s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload"] {
            assert!(actions.iter().any(|x| x == a), "{a}");
        }
    }

    #[test]
    fn there_is_no_id_element() {
        assert!(
            session_policy("data", &folders(), false)
                .get("Id")
                .is_none()
        );
    }
}
