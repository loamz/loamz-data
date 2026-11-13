//! Who is calling. People and services sign in through Keycloak; this checks the token and says who the caller is.
//! Every feature past the edge sees only an [`Identity`].
//!
//! Keys come from the issuer itself: its `OpenID` discovery document names the key set, and the set is refreshed when a
//! token names a key we have not seen (rate limited). There is no other way to give the server keys.
//!
//! An agent acting for a person arrives with a token-exchange token: the `sub` is the person and the `act` claim names
//! the agent. Both are kept, because what such a call may do is only what both are allowed to do.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, RwLock},
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

/// The audience every token for this service must carry. It is a protocol constant, not a setting: a token for another
/// Loamz service must never work here.
pub const AUDIENCE: &str = "loamz-data";
/// The Keycloak client role that may create an organization's Data account.
pub const ACCOUNT_ADMIN_ROLE: &str = "account-admin";
/// The Keycloak client role of the installation's global admin: creates organizations and adds people to them.
pub const PLATFORM_ADMIN_ROLE: &str = "platform-admin";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Person,
    Service,
}

/// An organization named in the token. Keycloak never lets an organization's alias change and keeps it unique in the realm, so
/// the alias is the organization's identity here; `alias` is kept apart from `id` only so the two can be told apart in the code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrgRef {
    pub id: String,
    pub alias: String,
}

/// What a running job may touch, on top of what the person it runs for may touch: it reads the securables in `reads` and writes
/// the ones in `writes`, and nothing else. A request carries one only when it came with a job's own token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub run: String,
    pub reads: Vec<String>,
    pub writes: Vec<String>,
}

/// The verified identity behind a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// The person (or service account) the request is for.
    pub subject: String,
    pub kind: Kind,
    /// The client the token was issued to (`azp`): for a service account, its client id.
    pub client: String,
    pub name: String,
    pub email: String,
    /// Group paths from Keycloak, such as `/finance/analysts`.
    pub groups: Vec<String>,
    /// Organizations the person belongs to (empty for a person with none, and for service accounts).
    pub orgs: Vec<OrgRef>,
    /// Client roles on `loamz-data`.
    pub roles: Vec<String>,
    /// The agent or service acting for the subject, if any.
    pub actor: Option<String>,
    /// Set only for a job running on someone's behalf: the person's rights, narrowed to what the job declared.
    pub scope: Option<Scope>,
    /// Set for a service principal: the one workspace it belongs to, and the only one it can open.
    pub confined_to: Option<String>,
}

impl Identity {
    /// The ids that grants may name for this identity: the person, then their groups.
    #[must_use]
    pub fn principals(&self) -> Vec<String> {
        let own = match self.kind {
            Kind::Person => format!("user:{}", self.subject),
            Kind::Service => format!("sp:{}", self.subject),
        };
        std::iter::once(own)
            .chain(self.groups.iter().map(|g| format!("group:{g}")))
            .collect()
    }

    /// The ids of the agent acting for the subject, if there is one. It has no groups of its own in the token.
    #[must_use]
    pub fn actor_principals(&self) -> Option<Vec<String>> {
        self.actor.as_ref().map(|a| vec![format!("sp:{a}")])
    }

    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    #[must_use]
    pub fn member_of(&self, org: &str) -> bool {
        self.orgs.iter().any(|o| o.id == org)
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("the token is malformed, expired or not meant for this service")]
    Invalid,
    #[error("the token was signed by a key we do not know")]
    UnknownKey,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    azp: Option<String>,
    name: Option<String>,
    preferred_username: Option<String>,
    email: Option<String>,
    #[serde(default)]
    groups: Vec<String>,
    organization: Option<Organizations>,
    resource_access: Option<HashMap<String, ClientAccess>>,
    /// `"agent"` or `"service"` on a service account's token (set by a mapper on its Keycloak client); absent for people.
    loamz_kind: Option<String>,
    act: Option<Act>,
}

/// Keycloak writes `"organization": ["acme"]`, or `{"acme": {...}}` when asked to add attributes; the aliases are what matters.
#[derive(Deserialize)]
#[serde(untagged)]
enum Organizations {
    List(Vec<String>),
    Map(BTreeMap<String, serde_json::Value>),
}

#[derive(Deserialize)]
struct ClientAccess {
    #[serde(default)]
    roles: Vec<String>,
}

#[derive(Deserialize)]
struct Act {
    sub: String,
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

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
    token_endpoint: String,
}

pub struct Verifier {
    keys: RwLock<HashMap<String, (String, String)>>,
    issuer: String,
    token_endpoint: RwLock<Option<String>>,
    /// Clients that act for people (the agents server). A person's token issued to one of them is that agent acting for the person.
    agent_clients: Vec<String>,
    last_fetch: Mutex<Option<Instant>>,
    http: reqwest::Client,
}

impl Verifier {
    #[must_use]
    pub fn new(issuer: &str) -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            issuer: issuer.trim_end_matches('/').to_owned(),
            token_endpoint: RwLock::new(None),
            agent_clients: Vec::new(),
            last_fetch: Mutex::new(None),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Names the clients that act for people. The identity provider issues a person's token to such a client only by exchange
    /// (they have no sign-in of their own), so it means: this agent, for this person.
    #[must_use]
    pub fn with_agent_clients(mut self, clients: Vec<String>) -> Self {
        self.agent_clients = clients;
        self
    }

    /// Installs a key set as the identity provider publishes it. Only RSA signing keys are kept.
    ///
    /// # Errors
    /// The document is not a key set.
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

    /// Loads the keys the issuer publishes. Rate limited, so a flood of tokens with unknown key ids cannot turn into a
    /// flood of requests to the identity provider. The discovery document must name the same issuer we were given.
    ///
    /// # Errors
    /// The issuer could not be reached, or does not say what we expect.
    pub async fn refresh(&self) -> Result<(), String> {
        {
            let mut last = self.last_fetch.lock().map_err(|_| "lock".to_owned())?;
            if last.is_some_and(|t| t.elapsed() < Duration::from_secs(30)) {
                return Ok(());
            }
            *last = Some(Instant::now());
        }
        let get = |url: String| async {
            self.http
                .get(url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| e.to_string())
        };
        let doc: Discovery = get(format!("{}/.well-known/openid-configuration", self.issuer))
            .await?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if doc.issuer.trim_end_matches('/') != self.issuer {
            return Err(format!(
                "the issuer says it is {}, not {}",
                doc.issuer, self.issuer
            ));
        }
        let keys = get(doc.jwks_uri)
            .await?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        if let Ok(mut slot) = self.token_endpoint.write() {
            *slot = Some(doc.token_endpoint);
        }
        self.set_keys(&keys).map_err(|e| e.to_string())
    }

    /// Sends a token request to the issuer, on behalf of the browser (see `kernel::system`). Returns the issuer's status and body.
    ///
    /// # Errors
    /// The issuer's address is not known yet, or it could not be reached.
    pub async fn exchange(&self, form: &[(&str, &str)]) -> Result<(u16, String), String> {
        let url = self
            .token_endpoint
            .read()
            .ok()
            .and_then(|e| e.clone())
            .ok_or("the sign-in service has not been reached yet")?;
        let res = self
            .http
            .post(url)
            .form(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        Ok((status, res.text().await.map_err(|e| e.to_string())?))
    }

    /// For tests: where token requests go, without a discovery document.
    #[doc(hidden)]
    pub fn set_token_endpoint(&self, url: &str) {
        if let Ok(mut slot) = self.token_endpoint.write() {
            *slot = Some(url.to_owned());
        }
    }

    fn key(&self, kid: &str) -> Option<DecodingKey> {
        let keys = self.keys.read().ok()?;
        let (n, e) = keys.get(kid)?;
        DecodingKey::from_rsa_components(n, e).ok()
    }

    /// # Errors
    /// The token is not valid for this service.
    pub fn verify(&self, token: &str) -> Result<Identity, AuthError> {
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
        rules.set_audience(&[AUDIENCE]);
        rules.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let c = decode::<Claims>(token, &key, &rules)
            .map_err(|_| AuthError::Invalid)?
            .claims;
        if c.sub.is_empty() || c.sub.len() > 128 {
            return Err(AuthError::Invalid);
        }
        let aliases: Vec<String> = match c.organization {
            Some(Organizations::List(l)) => l,
            Some(Organizations::Map(m)) => m.into_keys().collect(),
            None => vec![],
        };
        let mut orgs: Vec<OrgRef> = aliases
            .into_iter()
            .filter(|a| !a.is_empty() && a.len() <= 128)
            .map(|alias| OrgRef {
                id: alias.clone(),
                alias,
            })
            .collect();
        orgs.sort_by(|a, b| a.id.cmp(&b.id));
        orgs.dedup_by(|a, b| a.id == b.id);
        let mut groups: Vec<String> = c
            .groups
            .into_iter()
            .filter(|g| !g.is_empty() && g.len() <= 256)
            .collect();
        groups.sort();
        groups.dedup();
        let kind = if matches!(c.loamz_kind.as_deref(), Some("agent" | "service")) {
            Kind::Service
        } else {
            Kind::Person
        };
        let client = c.azp.unwrap_or_default();
        let actor = c.act.map(|a| a.sub).filter(|s| !s.is_empty()).or_else(|| {
            (kind == Kind::Person && self.agent_clients.contains(&client)).then(|| client.clone())
        });
        Ok(Identity {
            name: c.name.or(c.preferred_username).unwrap_or_default(),
            email: c.email.unwrap_or_default(),
            client,
            subject: c.sub,
            kind,
            groups,
            orgs,
            roles: c
                .resource_access
                .and_then(|mut m| m.remove(AUDIENCE))
                .map(|a| a.roles)
                .unwrap_or_default(),
            actor,
            scope: None,
            confined_to: None,
        })
    }
}
