//! Who may be given what. Roles are bundles of privileges; what is stored are the privileges themselves.

use crate::kernel::authz::{Privilege, Role};

/// A principal id is whatever the identity provider says: a person, an agent or a service.
#[must_use]
pub fn valid_principal(p: &str) -> bool {
    !p.is_empty() && p.len() <= 128 && !p.chars().any(char::is_control)
}

/// What a role stands for, as the names stored.
#[must_use]
pub fn privileges_of(role: Role) -> Vec<&'static str> {
    role.privileges()
        .iter()
        .map(|p: &Privilege| p.as_str())
        .collect()
}

/// The role a set of stored privileges amounts to, if it is exactly one of the three bundles.
#[must_use]
pub fn role_of(privileges: &[String]) -> Option<Role> {
    [Role::Reader, Role::Contributor, Role::Owner]
        .into_iter()
        .find(|r| {
            let want = r.privileges();
            want.len() == privileges.len()
                && want
                    .iter()
                    .all(|p| privileges.iter().any(|q| q == p.as_str()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_set_maps_back_to_its_role() {
        for r in [Role::Reader, Role::Contributor, Role::Owner] {
            let stored: Vec<String> = privileges_of(r).into_iter().map(str::to_owned).collect();
            assert_eq!(role_of(&stored), Some(r));
        }
        assert_eq!(role_of(&["read".into()]), None);
    }

    #[test]
    fn principals_must_be_plain() {
        assert!(valid_principal("ana"));
        assert!(!valid_principal(""));
        assert!(!valid_principal("a\nb"));
        assert!(!valid_principal(&"x".repeat(129)));
    }
}
