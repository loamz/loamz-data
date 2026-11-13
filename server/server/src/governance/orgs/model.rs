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

/// An organization's alias: its name in the identity provider. Lower-case letters, digits and dashes, 2 to 40 long, starting and
/// ending with a letter or digit.
#[must_use]
pub fn valid_alias(alias: &str) -> bool {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    (2..=40).contains(&alias.len())
        && alias
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && edge(alias.chars().next())
        && edge(alias.chars().last())
}

/// An email address, checked only as far as telling a typo from an address: one `@`, something on both sides, a dot in the domain.
#[must_use]
pub fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    email.len() <= 254
        && !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// A person's display name: not empty, short, one line.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 100 && !name.chars().any(char::is_control)
}

/// Someone just added to an organization. `temporary_password` is set only when they were made now: it is shown once, and they
/// must change it at their first sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invited {
    pub subject: String,
    pub email: String,
    pub temporary_password: Option<String>,
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
    fn aliases_are_plain_and_short() {
        for ok in ["acme", "a1", "my-team-2"] {
            assert!(valid_alias(ok), "{ok}");
        }
        for bad in [
            "a",
            "-acme",
            "acme-",
            "Acme",
            "ac me",
            "ac_me",
            &"a".repeat(41),
        ] {
            assert!(!valid_alias(bad), "{bad:?}");
        }
    }

    #[test]
    fn emails_are_told_from_typos() {
        assert!(valid_email("ana@acme.test"));
        for bad in [
            "ana",
            "@acme.test",
            "ana@acme",
            "a@b@c.d",
            "ana @acme.test",
            "ana@.test",
            "ana@acme.",
        ] {
            assert!(!valid_email(bad), "{bad:?}");
        }
    }

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
            confined_to: None,
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
