//! The directory of people and organizations: who exists and who belongs where. Keycloak keeps it; the rest of the server sees only
//! [`Directory`], so another identity provider is one more implementation, and tests use a fake.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::error::{Error, Result};

/// A person to add: the email is who they are, the name is for display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPerson {
    pub email: String,
    pub name: String,
}

/// A person in the directory: `subject` is the id every token of theirs carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub subject: String,
    /// False when the person already existed: their password is theirs, and the one offered was not used.
    pub created: bool,
}

#[async_trait]
pub trait Directory: Send + Sync {
    /// Makes the organization. One that already exists is fine: a retried request must be able to run again.
    async fn create_organization(&self, alias: &str) -> Result<()>;

    /// Finds the person by email, or makes them with a password they must change at their first sign-in.
    async fn ensure_person(&self, person: &NewPerson, temporary_password: &str) -> Result<Person>;

    /// Makes the person a member of the organization (already a member is fine).
    async fn add_member(&self, alias: &str, subject: &str) -> Result<()>;

    /// Gets the person's current group paths from the identity provider.
    async fn groups(&self, subject: &str) -> Result<Vec<String>>;
}

/// Keycloak's admin API, reached as a service account (client credentials) that can manage users and organizations.
pub struct Keycloak {
    http: reqwest::Client,
    token_url: String,
    admin: String,
    client_id: String,
    secret: String,
    token: Mutex<Option<(String, Instant)>>,
}

impl Keycloak {
    /// `issuer` is the realm's address, such as `https://login.example.com/realms/loamz`.
    ///
    /// # Errors
    /// The issuer is not a realm address.
    pub fn new(issuer: &str, client_id: &str, secret: &str) -> std::result::Result<Self, String> {
        let issuer = issuer.trim_end_matches('/');
        let (host, realm) = issuer
            .rsplit_once("/realms/")
            .ok_or("the issuer is not a Keycloak realm address")?;
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|e| e.to_string())?,
            token_url: format!("{issuer}/protocol/openid-connect/token"),
            admin: format!("{host}/admin/realms/{realm}"),
            client_id: client_id.to_owned(),
            secret: secret.to_owned(),
            token: Mutex::new(None),
        })
    }

    async fn bearer(&self) -> Result<String> {
        let mut held = self.token.lock().await;
        if let Some((t, until)) = &*held
            && Instant::now() < *until
        {
            return Ok(t.clone());
        }
        let reply: Value = self
            .http
            .post(&self.token_url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.secret.as_str()),
            ])
            .send()
            .await
            .map_err(unreachable)?
            .error_for_status()
            .map_err(unreachable)?
            .json()
            .await
            .map_err(unreachable)?;
        let token = reply["access_token"]
            .as_str()
            .ok_or_else(|| Error::Internal("the directory gave no token".into()))?
            .to_owned();
        let seconds = reply["expires_in"]
            .as_u64()
            .unwrap_or(60)
            .saturating_sub(10);
        *held = Some((token.clone(), Instant::now() + Duration::from_secs(seconds)));
        Ok(token)
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::Response> {
        let mut ask = self
            .http
            .request(method, format!("{}{path}", self.admin))
            .bearer_auth(self.bearer().await?);
        if let Some(b) = body {
            ask = ask.json(&b);
        }
        ask.send().await.map_err(unreachable)
    }

    async fn organization_id(&self, alias: &str) -> Result<String> {
        let found: Vec<Value> = self
            .send(
                reqwest::Method::GET,
                &format!("/organizations?search={}&exact=true", encode(alias)),
                None,
            )
            .await?
            .error_for_status()
            .map_err(unreachable)?
            .json()
            .await
            .map_err(unreachable)?;
        found
            .iter()
            .find(|o| o["alias"] == alias)
            .and_then(|o| o["id"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| Error::NotFound("the organization is not in the directory".into()))
    }
}

#[async_trait]
impl Directory for Keycloak {
    async fn create_organization(&self, alias: &str) -> Result<()> {
        let reply = self
            .send(
                reqwest::Method::POST,
                "/organizations",
                Some(json!({"name": alias, "alias": alias, "domains": []})),
            )
            .await?;
        match reply.status().as_u16() {
            200..=299 | 409 => Ok(()),
            s => Err(Error::Internal(format!(
                "the directory refused to create an organization ({s})"
            ))),
        }
    }

    async fn ensure_person(&self, person: &NewPerson, temporary_password: &str) -> Result<Person> {
        let found: Vec<Value> = self
            .send(
                reqwest::Method::GET,
                &format!("/users?email={}&exact=true", encode(&person.email)),
                None,
            )
            .await?
            .error_for_status()
            .map_err(unreachable)?
            .json()
            .await
            .map_err(unreachable)?;
        if let Some(id) = found.first().and_then(|u| u["id"].as_str()) {
            return Ok(Person {
                subject: id.to_owned(),
                created: false,
            });
        }
        let (first, last) = person.name.split_once(' ').unwrap_or((&person.name, ""));
        let reply = self
            .send(
                reqwest::Method::POST,
                "/users",
                Some(json!({
                    "username": person.email, "email": person.email,
                    "firstName": first, "lastName": last,
                    "enabled": true, "emailVerified": true,
                    "credentials": [{"type": "password", "value": temporary_password, "temporary": true}],
                })),
            )
            .await?;
        if !reply.status().is_success() {
            return Err(Error::Internal(format!(
                "the directory refused to create a person ({})",
                reply.status().as_u16()
            )));
        }
        let subject = reply
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|l| l.to_str().ok())
            .and_then(|l| l.rsplit('/').next())
            .ok_or_else(|| Error::Internal("the directory did not say who it made".into()))?
            .to_owned();
        Ok(Person {
            subject,
            created: true,
        })
    }

    async fn add_member(&self, alias: &str, subject: &str) -> Result<()> {
        let org = self.organization_id(alias).await?;
        let reply = self
            .send(
                reqwest::Method::POST,
                &format!("/organizations/{org}/members"),
                Some(Value::String(subject.to_owned())),
            )
            .await?;
        match reply.status().as_u16() {
            200..=299 | 409 => Ok(()),
            s => Err(Error::Internal(format!(
                "the directory refused to add a member ({s})"
            ))),
        }
    }

    async fn groups(&self, subject: &str) -> Result<Vec<String>> {
        let mut url = reqwest::Url::parse(&format!("{}/", self.admin))
            .map_err(|e| Error::Internal(format!("the directory URL: {e}")))?;
        url.path_segments_mut()
            .map_err(|()| Error::Internal("the directory URL cannot hold path segments".into()))?
            .pop_if_empty()
            .push("users")
            .push(subject)
            .push("groups");
        let groups: Vec<Value> = self
            .http
            .get(url)
            .bearer_auth(self.bearer().await?)
            .send()
            .await
            .map_err(unreachable)?
            .error_for_status()
            .map_err(unreachable)?
            .json()
            .await
            .map_err(unreachable)?;
        groups
            .iter()
            .map(|group| {
                group["path"].as_str().map(str::to_owned).ok_or_else(|| {
                    Error::Internal("the directory returned a group without a path".into())
                })
            })
            .collect()
    }
}

/// The directory could not be reached or answered badly: the detail is logged, never sent.
#[allow(clippy::needless_pass_by_value)] // used as `map_err(unreachable)`
fn unreachable(e: reqwest::Error) -> Error {
    Error::Internal(format!("the directory: {e}"))
}

fn encode(s: &str) -> String {
    reqwest::Url::parse_with_params("http://x/", [("q", s)])
        .ok()
        .and_then(|u| u.query().map(|q| q[2..].to_owned()))
        .unwrap_or_default()
}
