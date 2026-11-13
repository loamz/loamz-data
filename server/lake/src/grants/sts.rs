//! Credentials from STS `AssumeRole`, the way `RustFS`, `MinIO`-compatible stores and AWS S3 issue them. The server signs the call with
//! the bucket's own key and attaches a session policy, so what comes back opens only the folders asked for.

use std::time::{Duration, SystemTime};

use async_trait::async_trait;

use super::{Credentials, Issuer, folders, policy::session_policy, provider_error, sigv4};
use crate::files::FileError;

/// AWS refuses a shorter session than this.
const MIN_TTL: u64 = 900;
const MAX_TTL: u64 = 3600;

pub struct StsIssuer {
    http: reqwest::Client,
    url: reqwest::Url,
    /// The `Host` header the request is signed for.
    host: String,
    bucket: String,
    key_id: String,
    secret: String,
    region: String,
    role_arn: Option<String>,
}

impl StsIssuer {
    /// `endpoint` is where STS answers (for `RustFS`, its S3 address). The key is the bucket's own; `role_arn` is only for stores
    /// that need one (AWS S3).
    ///
    /// # Errors
    /// The endpoint is not a URL.
    pub fn new(
        endpoint: &str,
        region: &str,
        bucket: &str,
        key_id: &str,
        secret: &str,
        role_arn: Option<&str>,
    ) -> Result<Self, FileError> {
        let url = reqwest::Url::parse(endpoint)
            .map_err(|_| FileError::BadPath("the STS endpoint is not a URL"))?;
        let host = match (url.host_str(), url.port()) {
            (Some(h), Some(p)) => format!("{h}:{p}"),
            (Some(h), None) => h.to_owned(),
            _ => return Err(FileError::BadPath("the STS endpoint has no host")),
        };
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|e| provider_error(e.to_string()))?,
            url,
            host,
            bucket: bucket.to_owned(),
            key_id: key_id.to_owned(),
            secret: secret.to_owned(),
            region: region.to_owned(),
            role_arn: role_arn.map(str::to_owned),
        })
    }
}

#[async_trait]
impl Issuer for StsIssuer {
    async fn issue(
        &self,
        volumes: &[&str],
        write: bool,
        ttl: Duration,
    ) -> Result<Credentials, FileError> {
        let folders = folders(volumes)?;
        let policy = session_policy(&self.bucket, &folders, write).to_string();
        let seconds = ttl.as_secs().clamp(MIN_TTL, MAX_TTL).to_string();
        let mut form = vec![
            ("Action", "AssumeRole"),
            ("Version", "2011-06-15"),
            ("RoleSessionName", "loamz"),
            ("DurationSeconds", &seconds),
            ("Policy", &policy),
        ];
        if let Some(arn) = &self.role_arn {
            form.push(("RoleArn", arn));
        }
        let body = reqwest::Url::parse_with_params("http://x/", &form)
            .map_err(|e| provider_error(e.to_string()))?
            .query()
            .unwrap_or_default()
            .to_owned();
        let date = sigv4::amz_date(SystemTime::now());
        let content_type = "application/x-www-form-urlencoded";
        let authorization = sigv4::Signer {
            key_id: &self.key_id,
            secret: &self.secret,
            region: &self.region,
            service: "sts",
        }
        .authorization(
            "POST",
            "/",
            "",
            &[
                ("content-type", content_type),
                ("host", &self.host),
                ("x-amz-date", &date),
            ],
            body.as_bytes(),
            &date,
        );
        let reply = self
            .http
            .post(self.url.clone())
            .header("content-type", content_type)
            .header("x-amz-date", &date)
            .header("authorization", authorization)
            .body(body)
            .send()
            .await
            .map_err(|e| provider_error(e.to_string()))?;
        let status = reply.status();
        let xml = reply
            .text()
            .await
            .map_err(|e| provider_error(e.to_string()))?;
        if !status.is_success() {
            let why = tag(&xml, "Message").unwrap_or("no reason given");
            return Err(provider_error(format!(
                "the store refused to issue credentials ({status}): {why}"
            )));
        }
        let get = |name: &str| {
            tag(&xml, name)
                .map(str::to_owned)
                .ok_or_else(|| provider_error(format!("the store's answer has no {name}")))
        };
        Ok(Credentials {
            key_id: get("AccessKeyId")?,
            secret: get("SecretAccessKey")?,
            session_token: get("SessionToken")?,
        })
    }
}

/// The text of the first `<name>…</name>` in an XML answer. STS answers are flat; this avoids a full parser.
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(&xml[start..end])
}

#[cfg(test)]
mod tests {
    use super::tag;

    #[test]
    fn finds_a_value_in_an_sts_answer() {
        let xml = "<AssumeRoleResponse><Credentials><AccessKeyId>AK</AccessKeyId><SessionToken>tok</SessionToken></Credentials></AssumeRoleResponse>";
        assert_eq!(tag(xml, "AccessKeyId"), Some("AK"));
        assert_eq!(tag(xml, "SessionToken"), Some("tok"));
        assert_eq!(tag(xml, "SecretAccessKey"), None);
    }
}
