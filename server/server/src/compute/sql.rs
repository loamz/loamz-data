//! What a SQL statement reads, worked out by a real parser. Authorization of a query rests on this: every table it names is looked
//! up in the catalog and checked, and the statement is refused if it names anything that is not a catalog table.

use std::{collections::HashSet, ops::ControlFlow};

use sqlparser::{
    ast::{ObjectName, ObjectNamePart, Query, Statement, TableFactor, Visit, Visitor},
    dialect::DuckDbDialect,
    parser::Parser,
};

/// A table a statement reads, as `catalog.schema.table`, in lower case.
pub type Relation = [String; 3];

#[derive(Debug, PartialEq, Eq)]
pub struct Analysis {
    pub relations: Vec<Relation>,
}

#[derive(Default)]
struct Collector {
    /// Names introduced by `WITH`, which are not tables.
    ctes: HashSet<String>,
    names: Vec<ObjectName>,
    refused: Option<String>,
}

impl Visitor for Collector {
    type Break = ();

    fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<()> {
        if let Some(with) = &q.with {
            for cte in &with.cte_tables {
                self.ctes.insert(cte.alias.name.value.to_lowercase());
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, t: &TableFactor) -> ControlFlow<()> {
        // A table function (`read_csv('…')`) is a way to read files, never a catalog table.
        let function = matches!(
            t,
            TableFactor::Table { args: Some(_), .. }
                | TableFactor::TableFunction { .. }
                | TableFactor::Function { .. }
        );
        if function && self.refused.is_none() {
            self.refused = Some(
                "table functions are not allowed: read tables by their catalog.schema.table name"
                    .into(),
            );
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, name: &ObjectName) -> ControlFlow<()> {
        self.names.push(name.clone());
        ControlFlow::Continue(())
    }
}

fn parts(name: &ObjectName) -> Option<Vec<String>> {
    name.0
        .iter()
        .map(|p| match p {
            ObjectNamePart::Identifier(i) => Some(i.value.to_lowercase()),
            ObjectNamePart::Function(_) => None,
        })
        .collect()
}

/// Checks a statement is one plain query, and lists the catalog tables it reads.
///
/// # Errors
/// Why the statement is refused, in words for the person who wrote it.
pub fn analyze(sql: &str) -> Result<Analysis, String> {
    let statements = Parser::parse_sql(&DuckDbDialect {}, sql)
        .map_err(|e| format!("this is not valid SQL: {e}"))?;
    let [statement] = statements.as_slice() else {
        return Err(if statements.is_empty() {
            "there is no statement to run".into()
        } else {
            "run one statement at a time".into()
        });
    };
    if !matches!(statement, Statement::Query(_)) {
        return Err("only queries (SELECT, WITH, VALUES) can be run here; nothing can be created, changed or removed".into());
    }
    let mut c = Collector::default();
    let _ = statement.visit(&mut c);
    if let Some(why) = c.refused {
        return Err(why);
    }
    let mut relations: Vec<Relation> = Vec::new();
    for name in &c.names {
        let p = parts(name).ok_or("this table name is not allowed")?;
        // A name from `WITH` is not a table.
        if matches!(p.as_slice(), [one] if c.ctes.contains(one)) {
            continue;
        }
        let [catalog, schema, table] = <[String; 3]>::try_from(p).map_err(|p: Vec<String>| {
            format!(
                "name tables in full, as catalog.schema.table (found {:?})",
                p.join(".")
            )
        })?;
        let rel = [catalog, schema, table];
        if !relations.contains(&rel) {
            relations.push(rel);
        }
    }
    Ok(Analysis { relations })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn rel(a: &str, b: &str, c: &str) -> Relation {
        [a.into(), b.into(), c.into()]
    }

    #[test]
    fn finds_every_table_a_query_reads() {
        let a = analyze("SELECT o.*, c.name FROM Sales.Raw.Orders o JOIN sales.raw.customers c ON c.id = o.cid WHERE o.id IN (SELECT id FROM sales.clean.vip)").unwrap();
        assert_eq!(
            a.relations,
            vec![
                rel("sales", "raw", "orders"),
                rel("sales", "raw", "customers"),
                rel("sales", "clean", "vip")
            ]
        );
    }

    #[test]
    fn a_name_from_with_is_not_a_table_but_the_tables_inside_it_are() {
        let a = analyze(
            "WITH big AS (SELECT * FROM a.b.c WHERE n > 1) SELECT * FROM big JOIN a.b.d USING (id)",
        )
        .unwrap();
        assert_eq!(a.relations, vec![rel("a", "b", "c"), rel("a", "b", "d")]);
        assert_eq!(
            analyze("SELECT 1 AS one").unwrap().relations,
            Vec::<Relation>::new()
        );
    }

    #[test]
    fn only_one_plain_query_is_accepted() {
        for sql in [
            "DROP TABLE a.b.c",
            "CREATE TABLE a.b.c AS SELECT 1",
            "INSERT INTO a.b.c VALUES (1)",
            "COPY a.b.c TO '/tmp/x.csv'",
            "ATTACH '/etc/passwd' AS x",
            "INSTALL httpfs",
            "PRAGMA database_list",
            "SET enable_external_access = true",
            "SELECT 1; SELECT 2",
            "SELECT 1; DROP TABLE a.b.c",
            "",
            "   ",
        ] {
            assert!(analyze(sql).is_err(), "{sql:?} should be refused");
        }
    }

    #[test]
    fn reading_files_is_not_reading_tables() {
        for sql in [
            "SELECT * FROM read_csv('/etc/passwd')",
            "SELECT * FROM read_parquet('/inputs/tables/x/a.parquet')",
            "SELECT * FROM glob('/*')",
            "SELECT * FROM '/etc/passwd'",
            "SELECT * FROM 'file.csv'",
            "SELECT * FROM a.b.c, read_text('/etc/hosts')",
        ] {
            assert!(analyze(sql).is_err(), "{sql:?} should be refused");
        }
    }

    #[test]
    fn tables_must_be_named_in_full() {
        for sql in [
            "SELECT * FROM orders",
            "SELECT * FROM raw.orders",
            "SELECT * FROM a.b.c.d",
            "SELECT * FROM (SELECT * FROM orders)",
        ] {
            let e = analyze(sql).unwrap_err();
            assert!(e.contains("catalog.schema.table"), "{sql:?}: {e}");
        }
    }

    #[test]
    fn duckdb_syntax_is_understood() {
        assert!(analyze("SELECT * EXCLUDE (secret) FROM a.b.c").is_ok());
        assert!(analyze("FROM a.b.c SELECT count(*)").is_ok());
        assert!(analyze("SELECT list_transform([1,2,3], x -> x * 2)").is_ok());
    }
}
