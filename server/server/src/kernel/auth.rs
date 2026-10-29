//! Who is calling. People and services sign in through Keycloak; this checks the token and says who the caller is.
//! Every feature past the edge sees only a [`Caller`].
//!
//! An agent acting for a person arrives with a token-exchange token: the `sub` is the person and the `act` claim names
//! the agent. Both are kept, because what such a call may do is only what both are allowed to do.

use std::{
    collections::HashMap,
    sync::{Mutex, RwLock},
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

use super::settings::KeySource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Person,
    Service,
}

/// The verified identity behind a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    /// Whose data this is: the person, or the service itself when nobody is delegating.
    pub subject: String,
    pub kind: Kind,
    /// `u-<subject>` for a personal account, `o-<organization>` for an organization.
    pub tenant: String,
    /// The agent or service acting for the subject, if any.
    pub actor: Option<String>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("the token is malformed, expired or not meant for this service")]
    Invalid,
    #[error("the token was signed by a key we do not know")]
    UnknownKey,
    #[error("the token names several organizations; request exactly one")]
    AmbiguousOrganization,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    organization: Option<Organizations>,
    /// `"agent"` or `"service"` on a service account's token (set by a mapper on its Keycloak client); absent for people.
    loamz_kind: Option<String>,
    act: Option<Act>,
}

#[derive(Deserialize)]
struct Act {
    sub: String,
}

/// Keycloak lists a person's organizations as `{alias: {...}}` or as a list of aliases.
#[derive(Deserialize)]
#[serde(untagged)]
enum Organizations {
    Map(std::collections::BTreeMap<String, serde_json::Value>),
    List(Vec<String>),
}

#[derive(Deserialize)]
struct RawSet {
    keys: Vec<RawKey>,
}

#[derive(Deserialize)]
struct RawKey {
    kid: Option<String>,
    kty: String,
    #[serde(rename = "use")]
    usage: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

pub struct Verifier {
    keys: RwLock<HashMap<String, (String, String)>>,
    issuer: String,
    audience: String,
    source: KeySource,
    last_fetch: Mutex<Option<Instant>>,
    http: reqwest::Client,
}

impl Verifier {
    #[must_use]
    pub fn new(issuer: &str, audience: &str, source: KeySource) -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            issuer: issuer.to_owned(),
            audience: audience.to_owned(),
            source,
            last_fetch: Mutex::new(None),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Installs a key set as the identity provider publishes it. Only RSA signing keys are kept.
    pub fn set_keys(&self, json: &str) -> Result<(), AuthError> {
        let raw: RawSet = serde_json::from_str(json).map_err(|_| AuthError::Invalid)?;
        let keys = raw
            .keys
            .into_iter()
            .filter(|k| k.kty == "RSA" && k.usage.as_deref() != Some("enc"))
            .filter_map(|k| Some((k.kid?, (k.n?, k.e?))))
            .collect();
        if let Ok(mut slot) = self.keys.write() {
            *slot = keys;
        }
        Ok(())
    }

    /// Loads the keys from their source. From a URL it is rate limited, so a flood of tokens with unknown key ids cannot
    /// turn into a flood of requests to the identity provider.
    pub async fn refresh(&self) -> Result<(), String> {
        if matches!(self.source, KeySource::Url(_)) {
            let mut last = self.last_fetch.lock().map_err(|_| "lock".to_owned())?;
            if last.is_some_and(|t| t.elapsed() < Duration::from_secs(30)) {
                return Ok(());
            }
            *last = Some(Instant::now());
        }
        let json = match &self.source {
            KeySource::File(path) => tokio::fs::read_to_string(path)
                .await
                .map_err(|e| e.to_string())?,
            KeySource::Url(url) => self
                .http
                .get(url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| e.to_string())?
                .text()
                .await
                .map_err(|e| e.to_string())?,
        };
        self.set_keys(&json).map_err(|e| e.to_string())
    }

    fn key(&self, kid: &str) -> Option<DecodingKey> {
        let keys = self.keys.read().ok()?;
        let (n, e) = keys.get(kid)?;
        DecodingKey::from_rsa_components(n, e).ok()
    }

    /// # Errors
    /// The token is not valid for this service.
    pub fn verify(&self, token: &str) -> Result<Caller, AuthError> {
        let header = decode_header(token).map_err(|_| AuthError::Invalid)?;
        // Exactly RS256: never the algorithm the token asks for, which is how forged tokens get accepted.
        if header.alg != Algorithm::RS256 {
            return Err(AuthError::Invalid);
        }
        let key = self
            .key(&header.kid.ok_or(AuthError::Invalid)?)
            .ok_or(AuthError::UnknownKey)?;
        let mut rules = Validation::new(Algorithm::RS256);
        rules.set_issuer(&[&self.issuer]);
        rules.set_audience(&[&self.audience]);
        rules.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let claims = decode::<Claims>(token, &key, &rules)
            .map_err(|_| AuthError::Invalid)?
            .claims;
        if claims.sub.is_empty() || claims.sub.len() > 128 {
            return Err(AuthError::Invalid);
        }
        let mut orgs: Vec<String> = match claims.organization {
            Some(Organizations::Map(m)) => m.into_keys().collect(),
            Some(Organizations::List(l)) => l,
            None => vec![],
        };
        orgs.sort();
        orgs.dedup();
        let tenant = match orgs.len() {
            0 => format!("u-{}", claims.sub),
            1 => format!("o-{}", orgs.remove(0)),
            _ => return Err(AuthError::AmbiguousOrganization),
        };
        let kind = match claims.loamz_kind.as_deref() {
            Some("agent" | "service") => Kind::Service,
            _ => Kind::Person,
        };
        Ok(Caller {
            subject: claims.sub,
            kind,
            tenant,
            actor: claims.act.map(|a| a.sub).filter(|s| !s.is_empty()),
        })
    }
}
