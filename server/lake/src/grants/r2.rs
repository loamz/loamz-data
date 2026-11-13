//! Cloudflare R2 issues temporary credentials itself (`temp-access-credentials`); the server holds a management token for that
//! and nothing else.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Credentials, Issuer, folders, provider_error};
use crate::files::FileError;

pub struct R2Issuer {
    http: reqwest::Client,
    url: String,
    token: String,
    bucket: String,
    parent_key_id: String,
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

impl R2Issuer {
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
}

#[async_trait]
impl Issuer for R2Issuer {
    async fn issue(
        &self,
        volumes: &[&str],
        write: bool,
        ttl: Duration,
    ) -> Result<Credentials, FileError> {
        let prefixes = folders(volumes)?;
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
        let reply = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .json(&ask)
            .send()
            .await
            .map_err(|e| provider_error(e.to_string()))?;
        let status = reply.status();
        let answer: Answer = reply.json().await.map_err(|e| {
            provider_error(format!(
                "the provider answered {status} with something unreadable: {e}"
            ))
        })?;
        match answer.result {
            Some(c) if answer.success => Ok(c),
            _ => Err(provider_error(format!(
                "the provider refused to issue credentials ({status})"
            ))),
        }
    }
}
