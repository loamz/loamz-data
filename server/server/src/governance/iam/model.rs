//! Where the privileges of a role land. Pure.
//!
//! Unity Catalog grants `USE CATALOG` on the catalog and `USE SCHEMA` on the schema, and the data privilege on the thing
//! itself. A role given on a table must therefore put its USE privileges on the table's catalog and schema, or it would
//! reach nothing.

use crate::kernel::authz::{Level, Privilege, Role, Target};

/// `(securable id, privilege)` for every privilege of the role, placed where it works.
#[must_use]
pub fn placement(target: &Target, role: Role) -> Vec<(String, Privilege)> {
    let at = |level: Level| {
        target
            .chain
            .iter()
            .find(|l| l.level == level)
            .map(|l| l.id.clone())
    };
    let mut out: Vec<(String, Privilege)> = role
        .privileges()
        .iter()
        .map(|&p| {
            let id = match p {
                Privilege::UseCatalog => at(Level::Catalog),
                Privilege::UseSchema => at(Level::Schema),
                _ => None,
            };
            (id.unwrap_or_else(|| target.id().to_owned()), p)
        })
        .collect();
    // Only the metastore can have catalogs created in it.
    if target.chain[0].level == Level::Metastore && role != Role::Reader {
        out.push((target.id().to_owned(), Privilege::CreateCatalog));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::authz::Link;

    fn link(id: &str, level: Level) -> Link {
        Link {
            id: id.into(),
            level,
            owner: "user:ana".into(),
        }
    }

    fn table() -> Target {
        Target {
            chain: vec![
                link("t", Level::Leaf),
                link("s", Level::Schema),
                link("c", Level::Catalog),
                link("metastore:o", Level::Metastore),
            ],
            label: String::new(),
        }
    }

    #[test]
    fn use_privileges_land_on_the_catalog_and_the_schema() {
        let p = placement(&table(), Role::Reader);
        assert!(p.contains(&("c".into(), Privilege::UseCatalog)));
        assert!(p.contains(&("s".into(), Privilege::UseSchema)));
        assert!(p.contains(&("t".into(), Privilege::Select)));
        assert!(!p.iter().any(
            |(id, pr)| id == "t" && matches!(pr, Privilege::UseCatalog | Privilege::UseSchema)
        ));
    }

    #[test]
    fn on_a_catalog_use_schema_stays_on_the_catalog_and_inherits() {
        let catalog = Target {
            chain: vec![
                link("c", Level::Catalog),
                link("metastore:o", Level::Metastore),
            ],
            label: String::new(),
        };
        let p = placement(&catalog, Role::Reader);
        assert!(p.contains(&("c".into(), Privilege::UseCatalog)));
        assert!(p.contains(&("c".into(), Privilege::UseSchema)));
    }

    #[test]
    fn only_the_metastore_gets_create_catalog() {
        let m = crate::kernel::authz::metastore("o");
        assert!(
            placement(&m, Role::Contributor)
                .iter()
                .any(|(_, p)| *p == Privilege::CreateCatalog)
        );
        assert!(
            !placement(&m, Role::Reader)
                .iter()
                .any(|(_, p)| *p == Privilege::CreateCatalog)
        );
        assert!(
            !placement(&table(), Role::Owner)
                .iter()
                .any(|(_, p)| *p == Privilege::CreateCatalog)
        );
    }
}
