//! Row filters and column masks of a table: the rules, and what they become for one person.
//!
//! A rule is a small SQL expression written by whoever manages the table. It may use the table's columns, literals, a short list of
//! plain functions, and two words that mean the person asking: `current_principal()` (their name, as in grants) and
//! `in_group('name')` (true when they are in that group). Everything else, including sub-queries, is refused when the rule is
//! saved, so a rule can never read another table or run anything.

use std::{collections::BTreeMap, ops::ControlFlow};

use sqlparser::{
    ast::{
        Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Value, Visit, VisitMut, Visitor,
        VisitorMut,
    },
    dialect::DuckDbDialect,
    parser::Parser,
    tokenizer::Token,
};

const MAX_RULE: usize = 1000;
const MAX_MASKS: usize = 64;

/// Functions a rule may call. They only compute a value from their arguments.
const FUNCTIONS: &[&str] = &[
    "lower",
    "upper",
    "length",
    "concat",
    "coalesce",
    "nullif",
    "md5",
    "sha256",
    "left",
    "right",
    "substr",
    "substring",
    "replace",
    "regexp_replace",
    "repeat",
    "lpad",
    "rpad",
    "trim",
    "ltrim",
    "rtrim",
    "round",
    "abs",
    "hash",
    "year",
    "month",
    "date_trunc",
    "current_principal",
    "in_group",
];

/// The rules saved for a table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// Only rows where this is true are visible. Empty: all rows.
    pub row_filter: String,
    /// Column name to the expression shown in its place.
    pub masks: BTreeMap<String, String>,
}

impl Policy {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.row_filter.trim().is_empty() && self.masks.is_empty()
    }
}

/// A policy worked out for one person: nothing left to look up, every word about the person replaced by a value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolved {
    pub filter: Option<String>,
    pub masks: Vec<(String, String)>,
}

fn parse(text: &str) -> Result<Expr, String> {
    if text.len() > MAX_RULE {
        return Err("a rule is at most 1000 characters".into());
    }
    let mut parser = Parser::new(&DuckDbDialect {})
        .try_with_sql(text)
        .map_err(|e| format!("this rule is not valid SQL: {e}"))?;
    let expr = parser
        .parse_expr()
        .map_err(|e| format!("this rule is not valid SQL: {e}"))?;
    if parser.peek_token().token != Token::EOF {
        return Err("a rule is one expression, nothing after it".into());
    }
    Ok(expr)
}

fn literal(arg: &FunctionArg) -> Option<&str> {
    match arg {
        FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Value(v))) => match &v.value {
            Value::SingleQuotedString(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

struct Check;

impl Visitor for Check {
    type Break = String;

    fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<String> {
        let allowed = match e {
            Expr::Identifier(_)
            | Expr::Value(_)
            | Expr::BinaryOp { .. }
            | Expr::UnaryOp { .. }
            | Expr::Nested(_)
            | Expr::IsNull(_)
            | Expr::IsNotNull(_)
            | Expr::InList { .. }
            | Expr::Between { .. }
            | Expr::Like { .. }
            | Expr::ILike { .. }
            | Expr::Case { .. }
            | Expr::Cast { .. }
            | Expr::Substring { .. }
            | Expr::Trim { .. }
            | Expr::Extract { .. } => true,
            Expr::Function(f) => {
                let name = f.name.to_string().to_lowercase();
                if !FUNCTIONS.contains(&name.as_str()) {
                    return ControlFlow::Break(format!(
                        "the function {name} is not allowed in a rule"
                    ));
                }
                if f.over.is_some() || f.filter.is_some() || !f.within_group.is_empty() {
                    return ControlFlow::Break("a rule cannot use window or filter clauses".into());
                }
                match (name.as_str(), &f.args) {
                    ("current_principal", FunctionArguments::List(l)) if l.args.is_empty() => true,
                    ("in_group", FunctionArguments::List(l))
                        if l.args.len() == 1 && literal(&l.args[0]).is_some() =>
                    {
                        true
                    }
                    ("current_principal", _) => {
                        return ControlFlow::Break("current_principal() takes nothing".into());
                    }
                    ("in_group", _) => {
                        return ControlFlow::Break(
                            "in_group takes one group name in quotes: in_group('analysts')".into(),
                        );
                    }
                    _ => true,
                }
            }
            _ => false,
        };
        if allowed {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(
                "a rule may use columns, values, comparisons and simple functions only".into(),
            )
        }
    }
}

fn check(text: &str) -> Result<Expr, String> {
    let expr = parse(text)?;
    match Visit::visit(&expr, &mut Check) {
        ControlFlow::Break(why) => Err(why),
        ControlFlow::Continue(()) => Ok(expr),
    }
}

fn valid_column(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether the rules can be saved.
///
/// # Errors
/// Why not, in words for the person who wrote them.
pub fn validate(p: &Policy) -> Result<(), String> {
    if !p.row_filter.trim().is_empty() {
        check(&p.row_filter)?;
    }
    if p.masks.len() > MAX_MASKS {
        return Err("at most 64 columns can be masked".into());
    }
    for (column, rule) in &p.masks {
        if !valid_column(column) {
            return Err(format!("{column} is not a column name"));
        }
        check(rule).map_err(|why| format!("mask of {column}: {why}"))?;
    }
    Ok(())
}

struct Substitute<'a> {
    principal: &'a str,
    groups: &'a [String],
}

impl VisitorMut for Substitute<'_> {
    type Break = ();

    fn post_visit_expr(&mut self, e: &mut Expr) -> ControlFlow<()> {
        let Expr::Function(f) = e else {
            return ControlFlow::Continue(());
        };
        let name = f.name.to_string().to_lowercase();
        let value = match (name.as_str(), &f.args) {
            ("current_principal", _) => Value::SingleQuotedString(self.principal.to_owned()),
            ("in_group", FunctionArguments::List(l)) => {
                let wanted = l.args.first().and_then(literal).unwrap_or_default();
                let same =
                    |a: &str, b: &str| a.trim_start_matches('/') == b.trim_start_matches('/');
                Value::Boolean(self.groups.iter().any(|g| same(g, wanted)))
            }
            _ => return ControlFlow::Continue(()),
        };
        *e = Expr::Value(value.with_empty_span());
        ControlFlow::Continue(())
    }
}

fn substitute(text: &str, principal: &str, groups: &[String]) -> Result<String, String> {
    let mut expr = check(text)?;
    let mut s = Substitute { principal, groups };
    let _ = VisitMut::visit(&mut expr, &mut s);
    Ok(expr.to_string())
}

/// The policy as one person sees it.
///
/// # Errors
/// A saved rule is no longer valid (never expected: they are checked when saved).
pub fn resolve(p: &Policy, principal: &str, groups: &[String]) -> Result<Resolved, String> {
    let filter = if p.row_filter.trim().is_empty() {
        None
    } else {
        Some(substitute(&p.row_filter, principal, groups)?)
    };
    let mut masks = Vec::new();
    for (column, rule) in &p.masks {
        masks.push((column.clone(), substitute(rule, principal, groups)?));
    }
    Ok(Resolved { filter, masks })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn policy(filter: &str, masks: &[(&str, &str)]) -> Policy {
        Policy {
            row_filter: filter.into(),
            masks: masks
                .iter()
                .map(|(c, r)| ((*c).into(), (*r).into()))
                .collect(),
        }
    }

    #[test]
    fn plain_rules_are_saved() {
        let p = policy(
            "region = 'EU' AND (amount < 1000 OR in_group('finance'))",
            &[
                ("email", "md5(email)"),
                ("card", "concat('****', right(card, 4))"),
            ],
        );
        validate(&p).unwrap();
    }

    #[test]
    fn a_rule_can_never_read_or_run_anything_else() {
        for bad in [
            "x IN (SELECT id FROM other.schema.t)",
            "EXISTS (SELECT 1)",
            "read_text('/etc/passwd') = ''",
            "read_blob('/inputs/tables/x/a.parquet') IS NOT NULL",
            "(SELECT 1) = 1",
            "x = 1; DROP TABLE t",
            "sum(x) OVER () > 1",
            "current_principal(1) = 'a'",
            "in_group(name) ",
            "",
        ] {
            let result = validate(&policy(bad, &[]));
            // The empty rule means no filter, so it is fine; everything else is refused.
            assert_eq!(result.is_ok(), bad.is_empty(), "{bad}");
        }
        assert!(validate(&policy("", &[("a b", "1")])).is_err());
        assert!(validate(&policy("", &[("a", "x; y")])).is_err());
    }

    #[test]
    fn words_about_the_person_become_values() {
        let p = policy(
            "owner = current_principal() OR in_group('finance')",
            &[("ssn", "CASE WHEN in_group('hr') THEN ssn ELSE '***' END")],
        );
        let r = resolve(&p, "user:ana", &["finance".into()]).unwrap();
        assert_eq!(
            r.filter.unwrap(),
            "owner = 'user:ana' OR true",
            "the person's name and groups are values now"
        );
        assert_eq!(r.masks[0].1, "CASE WHEN false THEN ssn ELSE '***' END");
    }

    #[test]
    fn a_name_cannot_close_its_quote() {
        let p = policy("owner = current_principal()", &[]);
        let r = resolve(&p, "user:o'brien", &[]).unwrap();
        assert_eq!(r.filter.unwrap(), "owner = 'user:o''brien'");
    }
}
