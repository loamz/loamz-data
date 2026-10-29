//! The one place that decides whether someone may do something. Every feature asks here, and no other code looks at
//! grants. It is pure: grants in, yes or no out.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Privilege {
    /// See that it exists and look inside.
    Use,
    Read,
    Write,
    /// Make things inside it.
    Create,
    /// Drop it, and say who else may do what.
    Manage,
}

impl Privilege {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Use => "use",
            Self::Read => "read",
            Self::Write => "write",
            Self::Create => "create",
            Self::Manage => "manage",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "use" => Self::Use,
            "read" => Self::Read,
            "write" => Self::Write,
            "create" => Self::Create,
            "manage" => Self::Manage,
            _ => return None,
        })
    }
}

/// A named bundle of privileges. Roles are only a convenience for granting; what is stored are the privileges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Reader,
    Contributor,
    Owner,
}

impl Role {
    #[must_use]
    pub fn privileges(self) -> &'static [Privilege] {
        use Privilege::{Create, Manage, Read, Use, Write};
        match self {
            Self::Reader => &[Use, Read],
            Self::Contributor => &[Use, Read, Write, Create],
            Self::Owner => &[Use, Read, Write, Create, Manage],
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reader => "reader",
            Self::Contributor => "contributor",
            Self::Owner => "owner",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "reader" => Self::Reader,
            "contributor" => Self::Contributor,
            "owner" => Self::Owner,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub principal: String,
    pub securable: String,
    pub privilege: Privilege,
}

/// What a request is about: the thing itself, then every container above it up to the tenant's root. A grant on any of
/// them reaches the thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub chain: Vec<String>,
    /// What the audit trail calls it.
    pub label: String,
}

impl Target {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.chain[0]
    }
}

/// The id the tenant's root goes by. It is not a stored row: it is where an organization's or a person's catalogs hang.
#[must_use]
pub fn root_id(tenant: &str) -> String {
    format!("tenant:{tenant}")
}

#[must_use]
pub fn root(tenant: &str) -> Target {
    Target {
        chain: vec![root_id(tenant)],
        label: "tenant".into(),
    }
}

fn holds(principal: &str, need: Privilege, chain: &[String], grants: &[Grant]) -> bool {
    grants
        .iter()
        .any(|g| g.principal == principal && g.privilege == need && chain.contains(&g.securable))
}

/// A person alone, or an agent acting for a person: then both must hold the privilege, so an agent can never do more
/// than the person it acts for, nor the person more than the agent has been given.
#[must_use]
pub fn authorize(
    subject: &str,
    actor: Option<&str>,
    need: Privilege,
    chain: &[String],
    grants: &[Grant],
) -> bool {
    holds(subject, need, chain, grants) && actor.is_none_or(|a| holds(a, need, chain, grants))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grants(principal: &str, securable: &str, role: Role) -> Vec<Grant> {
        role.privileges()
            .iter()
            .map(|&privilege| Grant {
                principal: principal.into(),
                securable: securable.into(),
                privilege,
            })
            .collect()
    }

    fn chain(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn what_each_role_allows() {
        use Privilege::{Create, Manage, Read, Use, Write};
        let table = [
            (
                Role::Reader,
                [
                    (Use, true),
                    (Read, true),
                    (Write, false),
                    (Create, false),
                    (Manage, false),
                ],
            ),
            (
                Role::Contributor,
                [
                    (Use, true),
                    (Read, true),
                    (Write, true),
                    (Create, true),
                    (Manage, false),
                ],
            ),
            (
                Role::Owner,
                [
                    (Use, true),
                    (Read, true),
                    (Write, true),
                    (Create, true),
                    (Manage, true),
                ],
            ),
        ];
        for (role, expected) in table {
            let g = grants("ana", "vol", role);
            for (privilege, allowed) in expected {
                assert_eq!(
                    authorize("ana", None, privilege, &chain(&["vol", "schema"]), &g),
                    allowed,
                    "{role:?} {privilege:?}"
                );
            }
        }
    }

    #[test]
    fn a_grant_on_a_container_reaches_what_is_inside_but_not_beside_or_above() {
        let g = grants("ana", "catalog", Role::Reader);
        assert!(authorize(
            "ana",
            None,
            Privilege::Read,
            &chain(&["table", "schema", "catalog", "tenant:t"]),
            &g
        ));
        assert!(!authorize(
            "ana",
            None,
            Privilege::Read,
            &chain(&["other-table", "other-schema", "other-catalog"]),
            &g
        ));
        assert!(!authorize(
            "ana",
            None,
            Privilege::Read,
            &chain(&["tenant:t"]),
            &g
        ));
    }

    #[test]
    fn nobody_is_allowed_without_a_grant() {
        assert!(!authorize("ana", None, Privilege::Use, &chain(&["x"]), &[]));
        let g = grants("bob", "x", Role::Owner);
        assert!(!authorize("ana", None, Privilege::Use, &chain(&["x"]), &g));
    }

    #[test]
    fn an_agent_acting_for_a_person_is_limited_by_both() {
        let mut g = grants("ana", "vol", Role::Contributor);
        g.extend(grants("agent", "vol", Role::Reader));
        let c = chain(&["vol"]);
        assert!(
            authorize("ana", Some("agent"), Privilege::Read, &c, &g),
            "both may read"
        );
        assert!(
            !authorize("ana", Some("agent"), Privilege::Write, &c, &g),
            "the agent was given less"
        );
        assert!(
            authorize("ana", None, Privilege::Write, &c, &g),
            "the person alone may write"
        );
        let g2 = grants("agent", "vol", Role::Owner);
        assert!(
            !authorize("ana", Some("agent"), Privilege::Read, &c, &g2),
            "the person has nothing, so the agent cannot borrow"
        );
    }

    #[test]
    fn role_and_privilege_names_round_trip() {
        for r in [Role::Reader, Role::Contributor, Role::Owner] {
            assert_eq!(Role::parse(r.as_str()), Some(r));
        }
        for p in [
            Privilege::Use,
            Privilege::Read,
            Privilege::Write,
            Privilege::Create,
            Privilege::Manage,
        ] {
            assert_eq!(Privilege::parse(p.as_str()), Some(p));
        }
        assert_eq!(Role::parse("admin"), None);
    }
}
