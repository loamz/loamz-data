//! The one place that decides whether someone may do something with data. Every feature asks here, and no other code
//! looks at grants. It is pure: facts in, yes or no out.
//!
//! The model is Unity Catalog's:
//! - Privileges are granted to a principal (a person, a group, a service principal) on a securable and inherit downward.
//! - The owner of a securable has every privilege on it and on everything inside it.
//! - To reach something inside a catalog you need `USE CATALOG`, and inside a schema `USE SCHEMA`, as well as the
//!   privilege itself. Inheriting `SELECT` from a catalog does not include them.
//! - An account admin is the metastore admin: everything, everywhere in their organization.

use std::collections::HashSet;

use super::auth::Identity;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Privilege {
    /// Create catalogs (granted on the metastore).
    CreateCatalog,
    UseCatalog,
    UseSchema,
    /// See that it exists and read its description, without reading data.
    Browse,
    CreateSchema,
    CreateTable,
    CreateVolume,
    CreateView,
    Select,
    Modify,
    ReadVolume,
    WriteVolume,
    Execute,
    /// Grant and revoke, hand over ownership, drop.
    Manage,
    AllPrivileges,
}

use Privilege::{
    AllPrivileges, Browse, CreateCatalog, CreateSchema, CreateTable, CreateView, CreateVolume,
    Execute, Manage, Modify, ReadVolume, Select, UseCatalog, UseSchema, WriteVolume,
};

const ALL: [Privilege; 15] = [
    CreateCatalog,
    UseCatalog,
    UseSchema,
    Browse,
    CreateSchema,
    CreateTable,
    CreateVolume,
    CreateView,
    Select,
    Modify,
    ReadVolume,
    WriteVolume,
    Execute,
    Manage,
    AllPrivileges,
];

impl Privilege {
    /// The name as Unity Catalog writes it, which is also how it is stored.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CreateCatalog => "CREATE_CATALOG",
            UseCatalog => "USE_CATALOG",
            UseSchema => "USE_SCHEMA",
            Browse => "BROWSE",
            CreateSchema => "CREATE_SCHEMA",
            CreateTable => "CREATE_TABLE",
            CreateVolume => "CREATE_VOLUME",
            CreateView => "CREATE_VIEW",
            Select => "SELECT",
            Modify => "MODIFY",
            ReadVolume => "READ_VOLUME",
            WriteVolume => "WRITE_VOLUME",
            Execute => "EXECUTE",
            Manage => "MANAGE",
            AllPrivileges => "ALL_PRIVILEGES",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        ALL.into_iter().find(|p| p.as_str() == s)
    }

    /// What it reads like on screen.
    #[must_use]
    pub fn label(self) -> String {
        self.as_str().replace('_', " ")
    }
}

/// Named bundles, for granting in one step. What is stored is always the privileges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Reader,
    Contributor,
    Owner,
}

impl Role {
    #[must_use]
    pub fn privileges(self) -> &'static [Privilege] {
        match self {
            Self::Reader => &[UseCatalog, UseSchema, Browse, Select, ReadVolume, Execute],
            Self::Contributor => &[
                UseCatalog,
                UseSchema,
                Browse,
                Select,
                ReadVolume,
                Execute,
                Modify,
                WriteVolume,
                CreateSchema,
                CreateTable,
                CreateVolume,
                CreateView,
            ],
            Self::Owner => &[
                UseCatalog,
                UseSchema,
                Browse,
                Select,
                ReadVolume,
                Execute,
                Modify,
                WriteVolume,
                CreateSchema,
                CreateTable,
                CreateVolume,
                CreateView,
                Manage,
            ],
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
        [Self::Reader, Self::Contributor, Self::Owner]
            .into_iter()
            .find(|r| r.as_str() == s)
    }

    /// The bundle a stored set of privileges amounts to, if it is exactly one of them.
    #[must_use]
    pub fn of(privileges: &[String]) -> Option<Self> {
        [Self::Reader, Self::Contributor, Self::Owner]
            .into_iter()
            .find(|r| {
                let want = r.privileges();
                want.len() == privileges.len()
                    && want
                        .iter()
                        .all(|p| privileges.iter().any(|q| q == p.as_str()))
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Metastore,
    Catalog,
    Schema,
    Leaf,
}

/// One step of a securable's path to the top, with who owns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub id: String,
    pub level: Level,
    pub owner: String,
}

/// What a request is about: the thing itself first, then every container above it up to the organization's metastore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub chain: Vec<Link>,
    /// What the audit trail calls it.
    pub label: String,
}

impl Target {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.chain[0].id
    }
}

/// The id the organization's metastore goes by in grants. It is not a stored securable.
#[must_use]
pub fn metastore_id(org: &str) -> String {
    format!("metastore:{org}")
}

#[must_use]
pub fn metastore(org: &str) -> Target {
    Target {
        chain: vec![Link {
            id: metastore_id(org),
            level: Level::Metastore,
            owner: String::new(),
        }],
        label: "metastore".into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub principal: String,
    pub securable: String,
    pub privilege: Privilege,
}

/// One identity's side of a request: the ids grants may name, and whether it is an account admin of the organization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub principals: Vec<String>,
    pub account_admin: bool,
}

impl Side {
    #[must_use]
    pub fn of(identity: &Identity, account_admin: bool) -> Self {
        Self {
            principals: identity.principals(),
            account_admin,
        }
    }
}

/// A request is from one side (a person acting alone) or two (an agent acting for a person): both must be allowed.
pub type Sides = Vec<Side>;

fn holds(side: &Side, need: Privilege, from: usize, target: &Target, grants: &[Grant]) -> bool {
    let named: HashSet<&str> = side.principals.iter().map(String::as_str).collect();
    target.chain[from..].iter().any(|link| {
        named.contains(link.owner.as_str())
            || grants.iter().any(|g| {
                g.securable == link.id
                    && named.contains(g.principal.as_str())
                    && (g.privilege == need || g.privilege == AllPrivileges)
            })
    })
}

fn allowed_for(side: &Side, need: Privilege, target: &Target, grants: &[Grant]) -> bool {
    if side.account_admin {
        return true;
    }
    if !holds(side, need, 0, target, grants) {
        return false;
    }
    // Reaching into a catalog or schema needs USE on it, whatever else is held: `SELECT` on a catalog is not `USE CATALOG`.
    if need != UseCatalog && need != UseSchema && need != Browse {
        let mut ok = true;
        for (i, link) in target.chain.iter().enumerate() {
            match link.level {
                Level::Catalog => ok &= holds(side, UseCatalog, i, target, grants),
                Level::Schema => ok &= holds(side, UseSchema, i, target, grants),
                _ => {}
            }
        }
        if !ok {
            return false;
        }
    }
    true
}

/// Whether every side of the request holds `need` on `target`.
#[must_use]
pub fn authorize(sides: &[Side], need: Privilege, target: &Target, grants: &[Grant]) -> bool {
    !sides.is_empty() && sides.iter().all(|s| allowed_for(s, need, target, grants))
}

/// Whether every side may know the thing exists. Any privilege that reaches it will do, and so will `BROWSE`. `USE CATALOG`
/// and `USE SCHEMA` let you into that catalog or schema, but do not by themselves show what is inside it.
#[must_use]
pub fn can_see(sides: &[Side], target: &Target, grants: &[Grant]) -> bool {
    const SEEING: [Privilege; 6] = [Browse, Select, Modify, ReadVolume, WriteVolume, Manage];
    let own_use = match target.chain[0].level {
        Level::Catalog => Some(UseCatalog),
        Level::Schema => Some(UseSchema),
        _ => None,
    };
    !sides.is_empty()
        && sides.iter().all(|s| {
            s.account_admin
                || SEEING
                    .iter()
                    .chain(own_use.iter())
                    .any(|p| holds(s, *p, 0, target, grants))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(who: &str) -> Side {
        Side {
            principals: vec![format!("user:{who}")],
            account_admin: false,
        }
    }

    fn link(id: &str, level: Level, owner: &str) -> Link {
        Link {
            id: id.into(),
            level,
            owner: if owner.is_empty() {
                String::new()
            } else {
                format!("user:{owner}")
            },
        }
    }

    /// volume v in schema s in catalog c, all owned by ana, in the organization's metastore.
    fn volume() -> Target {
        Target {
            chain: vec![
                link("v", Level::Leaf, "ana"),
                link("s", Level::Schema, "ana"),
                link("c", Level::Catalog, "ana"),
                link("metastore:o", Level::Metastore, ""),
            ],
            label: "volume v".into(),
        }
    }

    fn give(who: &str, on: &str, ps: &[Privilege]) -> Vec<Grant> {
        ps.iter()
            .map(|&privilege| Grant {
                principal: format!("user:{who}"),
                securable: on.into(),
                privilege,
            })
            .collect()
    }

    #[test]
    fn the_owner_has_everything_inside_what_they_own() {
        assert!(authorize(&[side("ana")], WriteVolume, &volume(), &[]));
        assert!(authorize(&[side("ana")], Manage, &volume(), &[]));
        assert!(
            !authorize(&[side("bob")], ReadVolume, &volume(), &[]),
            "nobody else has anything"
        );
    }

    #[test]
    fn an_account_admin_is_the_metastore_admin() {
        let admin = Side {
            principals: vec!["user:root".into()],
            account_admin: true,
        };
        assert!(authorize(&[admin], Manage, &volume(), &[]));
    }

    #[test]
    fn a_privilege_without_use_on_the_parents_reaches_nothing() {
        let only_read = give("bob", "c", &[ReadVolume]);
        assert!(
            !authorize(&[side("bob")], ReadVolume, &volume(), &only_read),
            "READ VOLUME on the catalog is not USE CATALOG"
        );
        let with_use = [only_read, give("bob", "c", &[UseCatalog, UseSchema])].concat();
        assert!(
            authorize(&[side("bob")], ReadVolume, &volume(), &with_use),
            "with USE on the catalog (inherited by the schema) it works"
        );
        assert!(
            !authorize(&[side("bob")], WriteVolume, &volume(), &with_use),
            "and only what was granted"
        );
    }

    #[test]
    fn use_alone_lets_you_in_but_not_to_the_data_or_to_see_what_is_inside() {
        let g = give("bob", "c", &[UseCatalog, UseSchema]);
        assert!(!authorize(&[side("bob")], ReadVolume, &volume(), &g));
        assert!(
            !can_see(&[side("bob")], &volume(), &g),
            "USE does not show what is inside"
        );
        let schema = Target {
            chain: vec![
                link("s", Level::Schema, "ana"),
                link("c", Level::Catalog, "ana"),
                link("metastore:o", Level::Metastore, ""),
            ],
            label: String::new(),
        };
        assert!(
            can_see(&[side("bob")], &schema, &g),
            "but the schema you may use is there to see"
        );
        let browse = give("bob", "c", &[Browse]);
        assert!(
            can_see(&[side("bob")], &volume(), &browse),
            "BROWSE shows what is inside"
        );
    }

    #[test]
    fn a_grant_on_a_sibling_or_below_does_not_reach_up_or_across() {
        let other = Target {
            chain: vec![
                link("w", Level::Leaf, "ana"),
                link("s2", Level::Schema, "ana"),
                link("c", Level::Catalog, "ana"),
                link("metastore:o", Level::Metastore, ""),
            ],
            label: String::new(),
        };
        let g = give("bob", "v", &[ReadVolume, UseCatalog, UseSchema]);
        assert!(!authorize(&[side("bob")], ReadVolume, &other, &g));
        assert!(!can_see(&[side("bob")], &other, &g));
    }

    #[test]
    fn groups_work_like_people() {
        let g = vec![Grant {
            principal: "group:/finance/analysts".into(),
            securable: "c".into(),
            privilege: AllPrivileges,
        }];
        let member = Side {
            principals: vec!["user:cy".into(), "group:/finance/analysts".into()],
            account_admin: false,
        };
        assert!(authorize(&[member], Select, &volume(), &g));
        assert!(
            !authorize(&[side("cy")], Select, &volume(), &g),
            "leaving the group loses it at once"
        );
    }

    #[test]
    fn an_agent_acting_for_a_person_is_limited_by_both() {
        let mut g = give(
            "ana2",
            "c",
            &[UseCatalog, UseSchema, ReadVolume, WriteVolume],
        );
        g.extend(
            vec![UseCatalog, UseSchema, ReadVolume]
                .into_iter()
                .map(|privilege| Grant {
                    principal: "sp:agent".into(),
                    securable: "c".into(),
                    privilege,
                }),
        );
        let person = side("ana2");
        let agent = Side {
            principals: vec!["sp:agent".into()],
            account_admin: false,
        };
        let both = [person.clone(), agent.clone()];
        assert!(authorize(&both, ReadVolume, &volume(), &g));
        assert!(
            !authorize(&both, WriteVolume, &volume(), &g),
            "the agent was given less"
        );
        assert!(authorize(&[person], WriteVolume, &volume(), &g));
        let admin = Side {
            principals: vec!["user:boss".into()],
            account_admin: true,
        };
        assert!(
            !authorize(&[admin, agent], ReadVolume, &volume(), &[]),
            "an admin's agent does not inherit the admin's rights"
        );
    }

    #[test]
    fn create_needs_use_on_the_container_too() {
        let catalog = Target {
            chain: vec![
                link("c", Level::Catalog, "ana"),
                link("metastore:o", Level::Metastore, ""),
            ],
            label: String::new(),
        };
        let g = give("bob", "c", &[CreateSchema]);
        assert!(!authorize(&[side("bob")], CreateSchema, &catalog, &g));
        let g2 = [g, give("bob", "c", &[UseCatalog])].concat();
        assert!(authorize(&[side("bob")], CreateSchema, &catalog, &g2));
    }

    #[test]
    fn what_each_role_allows() {
        for (role, can_write, can_manage) in [
            (Role::Reader, false, false),
            (Role::Contributor, true, false),
            (Role::Owner, true, true),
        ] {
            let g = give("bob", "c", role.privileges());
            assert!(
                authorize(&[side("bob")], ReadVolume, &volume(), &g),
                "{role:?} reads"
            );
            assert_eq!(
                authorize(&[side("bob")], WriteVolume, &volume(), &g),
                can_write,
                "{role:?} writes"
            );
            assert_eq!(
                authorize(&[side("bob")], Manage, &volume(), &g),
                can_manage,
                "{role:?} manages"
            );
        }
    }

    #[test]
    fn names_round_trip() {
        for p in ALL {
            assert_eq!(Privilege::parse(p.as_str()), Some(p));
        }
        for r in [Role::Reader, Role::Contributor, Role::Owner] {
            let stored: Vec<String> = r
                .privileges()
                .iter()
                .map(|p| p.as_str().to_owned())
                .collect();
            assert_eq!(Role::of(&stored), Some(r));
        }
        assert_eq!(Privilege::parse("select"), None);
    }
}
