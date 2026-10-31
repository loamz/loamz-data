//! Short-lived storage credentials for a machine that must read (or write) some of the bucket: valid for minutes, limited to the
//! folders asked for, read-only unless writing was asked for. The bucket's own key never leaves the server.
//!
//! Cloudflare R2 issues these (`temp-access-credentials`); the server holds a management token for that and nothing else.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::files::FileError;

pub struct Issuer {
    http: reqwest::Client,
    url: String,
    token: String,
    bucket: String,
    parent_key_id: String,
}

/// What a machine is given: an S3 key pair and session token, good until they expire.
#[derive(Debug, Clone, Deserialize)]
pub struct Credentials {
    #[serde(rename = "accessKeyId")]
    pub key_id: String,
    #[serde(rename = "secretAccessKey")]
    pub secret: String,
    #[serde(rename = "sessionToken")]
    pub session_token: String,
}

#[derive(Serialize)]
struct Ask<'a> {
    bucket: &'a str,
    #[serde(rename = "parentAccessKeyId")]
    parent_access_key_id: &'a str,
    permission: &'a str,
    #[serde(rename = "ttlSeconds")]
    ttl_seconds: u64,
    prefixes: &'a [String],
}

#[derive(Deserialize)]
struct Answer {
    success: bool,
    result: Option<Credentials>,
}

impl Issuer {
    /// `endpoint` is the bucket's S3 address, whose first label is the Cloudflare account.
    ///
    /// # Errors
    /// The endpoint is not a Cloudflare R2 address.
    pub fn new(
        endpoint: &str,
        bucket: &str,
        parent_key_id: &str,
        token: &str,
    ) -> Result<Self, FileError> {
        let account = endpoint
            .strip_prefix("https://")
            .and_then(|h| h.split('.').next())
            .filter(|a| !a.is_empty() && a.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or(FileError::BadPath("endpoint is not an R2 address"))?;
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|e| FileError::Io(std::io::Error::other(e.to_string())))?,
            url: format!(
                "https://api.cloudflare.com/client/v4/accounts/{account}/r2/temp-access-credentials"
            ),
            token: token.to_owned(),
            bucket: bucket.to_owned(),
            parent_key_id: parent_key_id.to_owned(),
        })
    }

    /// Credentials for the given volume folders (each `<id>/`), for `ttl`.
    ///
    /// # Errors
    /// A folder name is not allowed, or the provider refused.
    pub async fn issue(
        &self,
        volumes: &[&str],
        write: bool,
        ttl: Duration,
    ) -> Result<Credentials, FileError> {
        let mut prefixes = Vec::new();
        for v in volumes {
            crate::files::check_volume(v)?;
            prefixes.push(format!("{v}/"));
        }
        if prefixes.is_empty() {
            return Err(FileError::BadPath("no folders"));
        }
        let ask = Ask {
            bucket: &self.bucket,
            parent_access_key_id: &self.parent_key_id,
            permission: if write {
                "object-read-write"
            } else {
                "object-read-only"
            },
            ttl_seconds: ttl.as_secs().clamp(60, 3600),
            prefixes: &prefixes,
        };
        let io = |e: String| FileError::Io(std::io::Error::other(e));
        let reply = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .json(&ask)
            .send()
            .await
            .map_err(|e| io(e.to_string()))?;
        let status = reply.status();
        let answer: Answer = reply.json().await.map_err(|e| {
            io(format!(
                "the provider answered {status} with something unreadable: {e}"
            ))
        })?;
        match answer.result {
            Some(c) if answer.success => Ok(c),
            _ => Err(io(format!(
                "the provider refused to issue credentials ({status})"
            ))),
        }
    }
}
