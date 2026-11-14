//! Build a store for a single customer location, with its destination pinned.
use crate::{
    endpoint::PinnedEndpoint,
    grants::{Issuer, StsIssuer},
    objects::{ScopedSigner, Signing, Store, install_tls_provider},
};
use object_store::{
    aws::AmazonS3Builder,
    client::{
        ClientOptions, HttpClient, HttpConnector, HttpError, HttpRequest, HttpResponse, HttpService,
    },
};
use std::sync::Arc;
use zeroize::Zeroize;

/// Non-secret configuration for one location.
pub struct LocationCredential {
    pub provider: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub generation: i64,
    pub allow_private: bool,
    pub allow_http: bool,
}

/// One-use input signing material. Building a handle empties it, even though it is borrowed.
pub struct LocationSecret {
    value: std::sync::Mutex<Option<SecretFields>>,
}
struct SecretFields {
    key_id: String,
    secret: String,
    token: Option<String>,
}
impl LocationSecret {
    #[must_use]
    pub fn new(key_id: String, secret: String, token: Option<String>) -> Self {
        Self {
            value: std::sync::Mutex::new(Some(SecretFields {
                key_id,
                secret,
                token,
            })),
        }
    }
    fn take(&self) -> Result<SecretFields, LocationError> {
        self.value
            .lock()
            .map_err(|_| LocationError::Unavailable)?
            .take()
            .ok_or(LocationError::Rejected)
    }
}
impl Drop for SecretFields {
    fn drop(&mut self) {
        self.key_id.zeroize();
        self.secret.zeroize();
        self.token.zeroize();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LocationError {
    #[error("the store rejected the credential")]
    Rejected,
    #[error("the store is unavailable")]
    Unavailable,
    #[error("the store or endpoint is unsupported")]
    Unsupported,
}

/// Public location details needed for a lease.
#[derive(Clone)]
pub struct Place {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
}
/// A store and vending capability tied to a single location generation.
pub struct LocationHandle {
    pub store: Store,
    pub vendor: Option<Arc<dyn Issuer>>,
    pub generation: i64,
    place: Place,
    prefix: String,
    check_client: reqwest13::Client,
}
impl LocationHandle {
    #[must_use]
    pub fn place(&self) -> Place {
        self.place.clone()
    }
}

fn redact_signing_headers(headers: &mut http::HeaderMap) {
    for (name, value) in headers.iter_mut() {
        if name == http::header::AUTHORIZATION || name.as_str() == "x-amz-security-token" {
            value.set_sensitive(true);
        }
    }
}

struct RedactedService(reqwest13::Client);
impl std::fmt::Debug for RedactedService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RedactedService")
    }
}
#[async_trait::async_trait]
impl HttpService for RedactedService {
    async fn call(&self, mut request: HttpRequest) -> Result<HttpResponse, HttpError> {
        redact_signing_headers(request.headers_mut());
        HttpService::call(&self.0, request).await
    }
}

pub(crate) struct PinnedConnector(pub(crate) reqwest13::Client);
impl std::fmt::Debug for PinnedConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PinnedConnector")
    }
}
impl HttpConnector for PinnedConnector {
    fn connect(&self, _: &ClientOptions) -> object_store::Result<HttpClient> {
        Ok(HttpClient::new(RedactedService(self.0.clone())))
    }
}

/// Build the in-memory clients without making provider requests.
///
/// # Errors
/// The provider or endpoint is not supported, or DNS cannot resolve it.
pub async fn build_location(
    c: &LocationCredential,
    secret: &LocationSecret,
) -> Result<LocationHandle, LocationError> {
    let secret = secret.take()?;
    let prefix_ok = c.prefix.is_empty()
        || c.prefix.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        });
    if !prefix_ok {
        return Err(LocationError::Unsupported);
    }
    if !["rustfs", "minio", "r2"].contains(&c.provider.as_str()) {
        return Err(LocationError::Unsupported);
    }
    if c.provider == "r2" {
        let url = reqwest13::Url::parse(&c.endpoint).map_err(|_| LocationError::Unsupported)?;
        let host = url.host_str().ok_or(LocationError::Unsupported)?;
        let account = host
            .strip_suffix(".r2.cloudflarestorage.com")
            .ok_or(LocationError::Unsupported)?;
        if account.len() != 32
            || !account.bytes().all(|b| b.is_ascii_hexdigit())
            || secret.token.as_ref().is_none_or(String::is_empty)
        {
            return Err(LocationError::Unsupported);
        }
    }
    let pinned = PinnedEndpoint::resolve(&c.endpoint, c.allow_private, c.allow_http)
        .await
        .map_err(|e| {
            if e == "endpoint resolution failed" {
                LocationError::Unavailable
            } else {
                LocationError::Unsupported
            }
        })?;
    install_tls_provider();
    let client = pinned.client().map_err(|_| LocationError::Unavailable)?;
    let http = pinned.client12().map_err(|_| LocationError::Unavailable)?;
    let s3 = AmazonS3Builder::new()
        .with_endpoint(&c.endpoint)
        .with_allow_http(c.allow_http)
        .with_bucket_name(&c.bucket)
        .with_access_key_id(&secret.key_id)
        .with_secret_access_key(&secret.secret)
        .with_region(&c.region)
        .with_http_connector(PinnedConnector(client.clone()))
        .build()
        .map_err(|_| LocationError::Rejected)?;
    // Enable other vending adapters only after their provider conformance tests pass.
    let vendor: Option<Arc<dyn Issuer>> = if c.provider == "rustfs" {
        Some(Arc::new(
            StsIssuer::with_client(
                &c.endpoint,
                &c.region,
                &c.bucket,
                &secret.key_id,
                &secret.secret,
                None,
                http,
            )
            .map_err(|_| LocationError::Rejected)?
            .with_prefix(&c.prefix),
        ))
    } else {
        None
    };
    let signing = vendor.as_ref().map_or(Signing::Disabled, |vendor| {
        Signing::Scoped(ScopedSigner {
            endpoint: c.endpoint.clone(),
            region: c.region.clone(),
            bucket: c.bucket.clone(),
            client: client.clone(),
            vendor: vendor.clone(),
        })
    });
    Ok(LocationHandle {
        store: Store::customer_bucket(
            Arc::new(s3),
            c.endpoint.clone(),
            c.bucket.clone(),
            c.prefix.clone(),
            signing,
        ),
        vendor,
        generation: c.generation,
        place: Place {
            endpoint: c.endpoint.clone(),
            region: c.region.clone(),
            bucket: c.bucket.clone(),
        },
        prefix: c.prefix.clone(),
        check_client: client,
    })
}

/// Check the location with a non-destructive listing restricted to its prefix.
///
/// # Errors
/// The provider rejected the operation or could not be reached.
pub async fn check(handle: &LocationHandle) -> Result<(), LocationError> {
    let s3 = handle.store.s3().ok_or(LocationError::Unsupported)?;
    let credentials = s3
        .credentials()
        .get_credential()
        .await
        .map_err(|_| LocationError::Unavailable)?;
    let mut url =
        reqwest13::Url::parse(&handle.place.endpoint).map_err(|_| LocationError::Unsupported)?;
    url.set_path(&format!("/{}", handle.place.bucket));
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("list-type", "2")
            .append_pair("max-keys", "1");
        if !handle.prefix.is_empty() {
            query.append_pair("prefix", &format!("{}/", handle.prefix));
        }
    }
    let host = match (url.host_str(), url.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_owned(),
        _ => return Err(LocationError::Unsupported),
    };
    let date = crate::grants::sigv4::amz_date(std::time::SystemTime::now());
    let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let authorization = crate::grants::sigv4::Signer {
        key_id: &credentials.key_id,
        secret: &credentials.secret_key,
        region: &handle.place.region,
        service: "s3",
    }
    .authorization(
        "GET",
        url.path(),
        url.query().unwrap_or_default(),
        &[
            ("host", &host),
            ("x-amz-content-sha256", hash),
            ("x-amz-date", &date),
        ],
        b"",
        &date,
    );
    let mut auth = reqwest13::header::HeaderValue::from_str(&authorization)
        .map_err(|_| LocationError::Rejected)?;
    auth.set_sensitive(true);
    let response = handle
        .check_client
        .get(url)
        .header("x-amz-content-sha256", hash)
        .header("x-amz-date", date)
        .header(reqwest13::header::AUTHORIZATION, auth)
        .send()
        .await
        .map_err(|_| LocationError::Unavailable)?;
    match response.status().as_u16() {
        200..=299 => Ok(()),
        401 | 403 => Err(LocationError::Rejected),
        _ => Err(LocationError::Unavailable),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn signing_headers_are_marked_sensitive() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            "parent-key-canary".parse().unwrap(),
        );
        headers.insert("x-amz-security-token", "token-canary".parse().unwrap());
        super::redact_signing_headers(&mut headers);
        assert!(headers[http::header::AUTHORIZATION].is_sensitive());
        assert!(headers["x-amz-security-token"].is_sensitive());
        assert!(!format!("{headers:?}").contains("canary"));
    }
}
