//! An organization's Data account. Pure.

use crate::kernel::auth::Identity;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Org {
    pub id: String,
    /// `org` (a Keycloak organization) or `personal` (one person's own space).
    pub kind: String,
    pub alias: String,
    pub created_ms: i64,
}

/// The id of a person's own space.
#[must_use]
pub fn personal_id(subject: &str) -> String {
    format!("personal:{subject}")
}

/// Whether this identity belongs to the organization: a member of it in the identity provider, or the person whose own space it is.
#[must_use]
pub fn belongs(identity: &Identity, org: &str) -> bool {
    identity.member_of(org) || org == personal_id(&identity.subject)
}

/// A principal is typed (`user:`, `group:` or `sp:`) and plain.
#[must_use]
pub fn valid_principal(p: &str) -> bool {
    let typed = ["user:", "group:", "sp:"]
        .iter()
        .any(|t| p.starts_with(t) && p.len() > t.len());
    typed && p.len() <= 300 && !p.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::auth::{Kind, OrgRef};

    #[test]
    fn principals_are_typed_and_plain() {
        for ok in ["user:abc", "group:/finance/analysts", "sp:svc"] {
            assert!(valid_principal(ok), "{ok}");
        }
        for bad in [
            "abc",
            "user:",
            "team:x",
            "user:a\nb",
            &format!("user:{}", "x".repeat(300)),
        ] {
            assert!(!valid_principal(bad), "{bad:?}");
        }
    }

    fn who(sub: &str, orgs: &[&str]) -> Identity {
        Identity {
            subject: sub.into(),
            kind: Kind::Person,
            client: String::new(),
            name: String::new(),
            email: String::new(),
            groups: vec![],
            orgs: orgs
                .iter()
                .map(|o| OrgRef {
                    id: (*o).into(),
                    alias: (*o).into(),
                })
                .collect(),
            roles: vec![],
            actor: None,
            scope: None,
        }
    }

    #[test]
    fn a_person_belongs_to_their_organizations_and_their_own_space_only() {
        let ana = who("ana", &["o1"]);
        assert!(belongs(&ana, "o1"));
        assert!(belongs(&ana, "personal:ana"));
        assert!(!belongs(&ana, "o2"));
        assert!(!belongs(&ana, "personal:bob"));
    }
}
