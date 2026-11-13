//! What a SQL statement reads, worked out by a real parser. Authorization of a query rests on this: every table it names is looked
//! up in the catalog and checked, and the statement is refused if it names anything that is not a catalog table.

use std::{collections::HashMap, collections::HashSet, ops::ControlFlow};

use sqlparser::{
    ast::{
        Expr, ObjectName, ObjectNamePart, Query, SetExpr, Statement, TableFactor, Visit, VisitMut,
        Visitor, VisitorMut,
    },
    dialect::DuckDbDialect,
    parser::Parser,
};

use crate::governance::catalogs::policy::Resolved;

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

    fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<()> {
        let Expr::Function(function) = e else {
            return ControlFlow::Continue(());
        };
        let Some(ObjectNamePart::Identifier(name)) = function.name.0.last() else {
            return ControlFlow::Continue(());
        };
        if matches!(name.value.to_ascii_lowercase().as_str(), "read_blob" | "read_text") {
            self.refused = Some("file-reading functions are not allowed; read tables by their catalog.schema.table name".into());
            return ControlFlow::Break(());
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

/// Puts a table's row filter and column masks in front of every place a statement reads it. Each such table becomes
/// `(SELECT * REPLACE (mask AS col) FROM table WHERE filter) AS name`: the rules apply before anything the person wrote, to
/// `*`, to joins, to sub-queries and to `WITH` alike. The table's own name stays the alias, so the rest of the statement is
/// unchanged. The statement must already have passed [`analyze`].
///
/// # Errors
/// The statement could not be rewritten; it is then not run.
pub fn guard<S: std::hash::BuildHasher>(
    sql: &str,
    rules: &HashMap<Relation, Resolved, S>,
) -> Result<String, String> {
    if rules.is_empty() {
        return Ok(sql.to_owned());
    }
    let mut statements = Parser::parse_sql(&DuckDbDialect {}, sql)
        .map_err(|e| format!("this is not valid SQL: {e}"))?;
    let mut wrap = Wrap {
        rules,
        failed: None,
    };
    for statement in &mut statements {
        let _ = VisitMut::visit(statement, &mut wrap);
    }
    match wrap.failed {
        Some(why) => Err(why),
        None => Ok(statements
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")),
    }
}

struct Wrap<'a, S> {
    rules: &'a HashMap<Relation, Resolved, S>,
    failed: Option<String>,
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The table as a derived table: its masks and filter applied.
fn derived(name: &ObjectName, alias: &str, rule: &Resolved) -> Result<TableFactor, String> {
    let replace = if rule.masks.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = rule
            .masks
            .iter()
            .map(|(column, expr)| format!("({expr}) AS {}", quote(column)))
            .collect();
        format!(" REPLACE ({})", items.join(", "))
    };
    let filter = rule
        .filter
        .as_ref()
        .map_or_else(String::new, |f| format!(" WHERE ({f})"));
    let text = format!(
        "SELECT 1 FROM (SELECT *{replace} FROM {name}{filter}) AS {}",
        quote(alias)
    );
    let parsed = Parser::parse_sql(&DuckDbDialect {}, &text)
        .map_err(|_| "the table's rules could not be applied".to_owned())?;
    if let [Statement::Query(q)] = parsed.as_slice()
        && let SetExpr::Select(select) = q.body.as_ref()
        && let Some(from) = select.from.first()
    {
        return Ok(from.relation.clone());
    }
    Err("the table's rules could not be applied".into())
}

impl<S: std::hash::BuildHasher> VisitorMut for Wrap<'_, S> {
    type Break = ();

    fn post_visit_table_factor(&mut self, t: &mut TableFactor) -> ControlFlow<()> {
        let TableFactor::Table { name, alias, .. } = t else {
            return ControlFlow::Continue(());
        };
        let Some(p) = parts(name) else {
            return ControlFlow::Continue(());
        };
        let Ok(key) = <Relation>::try_from(p) else {
            return ControlFlow::Continue(());
        };
        let Some(rule) = self.rules.get(&key) else {
            return ControlFlow::Continue(());
        };
        let shown = alias
            .as_ref()
            .map_or_else(|| key[2].clone(), |a| a.name.value.clone());
        match derived(name, &shown, rule) {
            Ok(mut factor) => {
                // Keep the statement's own alias (with any column names it gave).
                if let (TableFactor::Derived { alias: new, .. }, Some(old)) = (&mut factor, alias) {
                    *new = Some(old.clone());
                }
                *t = factor;
            }
            Err(why) => {
                self.failed = Some(why);
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn rule(filter: Option<&str>, masks: &[(&str, &str)]) -> HashMap<Relation, Resolved> {
        HashMap::from([(
            ["sales".to_owned(), "raw".to_owned(), "orders".to_owned()],
            Resolved {
                filter: filter.map(Into::into),
                masks: masks
                    .iter()
                    .map(|(c, e)| ((*c).into(), (*e).into()))
                    .collect(),
            },
        )])
    }

    #[test]
    fn a_table_with_rules_is_read_only_through_them() {
        let rules = rule(Some("region = 'EU'"), &[("email", "md5(email)")]);
        let sql = guard("SELECT * FROM sales.raw.orders", &rules).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM (SELECT * REPLACE ((md5(email)) AS \"email\") FROM sales.raw.orders WHERE (region = 'EU')) AS \"orders\""
        );
    }

    #[test]
    fn every_way_to_name_the_table_is_covered() {
        let rules = rule(Some("region = 'EU'"), &[]);
        let sql = guard(
            "WITH x AS (SELECT * FROM sales.raw.orders) SELECT o.id FROM sales.raw.orders AS o JOIN x ON x.id = o.id WHERE o.id IN (SELECT id FROM SALES.RAW.ORDERS)",
            &rules,
        )
        .unwrap();
        assert_eq!(sql.matches("WHERE (region = 'EU')").count(), 3, "{sql}");
        assert!(!sql.contains("FROM sales.raw.orders AS o"), "{sql}");
        assert!(
            sql.contains("AS o"),
            "the statement's own alias is kept: {sql}"
        );
    }

    #[test]
    fn a_statement_without_such_a_table_is_unchanged() {
        let rules = rule(Some("a = 1"), &[]);
        let sql = "SELECT * FROM sales.raw.other";
        assert_eq!(guard(sql, &rules).unwrap(), sql);
        assert_eq!(guard(sql, &HashMap::new()).unwrap(), sql);
    }

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
            "SELECT read_blob('/inputs/tables/x/a.parquet') FROM a.b.c",
            "SELECT Main.\"READ_BLOB\"('/inputs/tables/x/a.parquet') FROM a.b.c",
            "SELECT read_text('/inputs/tables/x/data.txt') FROM a.b.c",
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
