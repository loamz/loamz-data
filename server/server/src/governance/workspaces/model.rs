//! Workspaces: where people work. Pure.

pub const ENTITLEMENTS: [&str; 3] = ["sql-access", "compute-create", "sandbox-access"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Admin,
    User,
}

impl Role {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Self::Admin),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub org_id: String,
    pub name: String,
    pub created_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub principal: String,
    pub role: Role,
}

/// A workspace name is what people pick it by: words, digits, spaces, dashes, up to 64 characters.
#[must_use]
pub fn valid_name(n: &str) -> bool {
    let t = n.trim();
    !t.is_empty()
        && t == n
        && n.chars().count() <= 64
        && n.chars()
            .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_')
}

/// The most a principal holds in a workspace is the highest role among the principals they speak as (themselves and their groups).
#[must_use]
pub fn best_role(members: &[Member], principals: &[String]) -> Option<Role> {
    members
        .iter()
        .filter(|m| principals.contains(&m.principal))
        .map(|m| m.role)
        .fold(None, |best, r| match (best, r) {
            (Some(Role::Admin), _) | (_, Role::Admin) => Some(Role::Admin),
            _ => Some(Role::User),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_plain() {
        assert!(valid_name("Finance"));
        assert!(valid_name("team-a_2026 east"));
        for bad in ["", " lead", "trail ", "a/b", "<b>", &"x".repeat(65)] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_best_role_among_a_person_and_their_groups_wins() {
        let members = vec![
            Member {
                principal: "group:/eng".into(),
                role: Role::Admin,
            },
            Member {
                principal: "user:cy".into(),
                role: Role::User,
            },
        ];
        assert_eq!(
            best_role(&members, &["user:cy".into(), "group:/eng".into()]),
            Some(Role::Admin)
        );
        assert_eq!(best_role(&members, &["user:cy".into()]), Some(Role::User));
        assert_eq!(best_role(&members, &["user:zed".into()]), None);
    }
}
