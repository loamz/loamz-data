//! Writing and checking the trail. This is the bottom of the stack: it knows no other feature.
//!
//! Every change that must be recorded is a [`Uow`]: the change and its audit entry commit together or not at all, so
//! there is never a change without a record or a record of a change that did not happen.

use sqlx::{Any, Transaction};
use tokio::sync::MutexGuard;

use super::{
    model::{Entry, GENESIS, Who, first_broken, hash, subject_token_len},
    store,
};
use crate::kernel::{error::Result, now_ms, state::App};

/// One unit of work: a transaction, and the right to extend the audit chains. With `SQLite` the pool has one connection,
/// so while this is alive all reads and writes must go through [`Uow::tx`]; read what you need first, then begin.
pub struct Uow<'a> {
    pub tx: Transaction<'static, Any>,
    _turn: MutexGuard<'a, ()>,
}

impl<'a> Uow<'a> {
    /// # Errors
    /// The database could not be reached.
    pub async fn begin(app: &'a App) -> Result<Self> {
        let turn = app.write.lock().await;
        Ok(Self {
            tx: app.db.begin().await?,
            _turn: turn,
        })
    }

    /// Adds an entry to the organization's chain, inside this unit of work.
    ///
    /// # Errors
    /// The database could not be written.
    pub async fn audit(
        &mut self,
        org: &str,
        who: &Who,
        action: &str,
        target: &str,
        outcome: &str,
    ) -> Result<()> {
        let prev = store::last(&mut *self.tx, org).await?;
        let actor = if who.actor == "system" {
            who.actor.clone()
        } else {
            store::name_ref(&mut self.tx, org, &who.actor).await?
        };
        let on_behalf_of = if who.on_behalf_of.is_empty() {
            String::new()
        } else {
            store::name_ref(&mut self.tx, org, &who.on_behalf_of).await?
        };
        let target = target_refs(&mut self.tx, org, target).await?;
        let mut e = Entry {
            org: org.to_owned(),
            seq: prev.as_ref().map_or(1, |p| p.seq + 1),
            at_ms: now_ms(),
            actor,
            on_behalf_of,
            action: action.to_owned(),
            target,
            outcome: outcome.to_owned(),
            prev_hash: prev.map_or_else(|| GENESIS.to_owned(), |p| p.hash),
            hash: String::new(),
        };
        e.hash = hash(&e);
        store::insert(&mut *self.tx, &e).await
    }

    /// # Errors
    /// The database could not commit.
    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await?;
        Ok(())
    }
}

async fn target_refs(conn: &mut sqlx::AnyConnection, org: &str, target: &str) -> Result<String> {
    let mut out = String::with_capacity(target.len());
    let mut rest = target;
    while let Some(start) = rest.find("user:") {
        out.push_str(&rest[..start + 5]);
        let tail = &rest[start + 5..];
        if let Some(len) = legacy_tombstone_len(tail) {
            out.push_str("erased person");
            rest = &tail[len..];
            continue;
        }
        let len = subject_token_len(tail);
        if len == 0 {
            rest = tail;
            continue;
        }
        out.push_str(&store::name_ref(conn, org, &tail[..len]).await?);
        rest = &tail[len..];
    }
    out.push_str(rest);
    Ok(out)
}

fn legacy_tombstone_len(value: &str) -> Option<usize> {
    let digest = value.strip_prefix("erased:")?;
    digest
        .get(..64)
        .filter(|s| s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|_| value.len() - digest.len() + 64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_hashed_target_refs_display_as_erased_person() {
        let names = std::collections::HashMap::new();
        assert_eq!(
            display_target(
                &format!("catalog <- user:erased:{}.", "a".repeat(64)),
                &names
            ),
            format!("catalog <- user:erased person.")
        );
    }
}

fn display_ref(reference: &str, names: &std::collections::HashMap<String, String>) -> String {
    if reference.starts_with("p_") {
        names
            .get(reference)
            .cloned()
            .unwrap_or_else(|| "erased person".into())
    } else if reference.starts_with("erased:") {
        "erased person".into()
    } else {
        reference.to_owned()
    }
}

fn display_target(target: &str, names: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::with_capacity(target.len());
    let mut rest = target;
    while let Some(start) = rest.find("user:") {
        out.push_str(&rest[..start + 5]);
        let tail = &rest[start + 5..];
        if let Some(len) = legacy_tombstone_len(tail) {
            out.push_str("erased person");
            rest = &tail[len..];
            continue;
        }
        let len = subject_token_len(tail);
        if len == 0 {
            rest = tail;
            continue;
        }
        let reference = &tail[..len];
        if reference.starts_with("p_") || reference.starts_with("erased:") {
            out.push_str(&display_ref(reference, names));
        } else {
            out.push_str(reference);
        }
        rest = &tail[len..];
    }
    out.push_str(rest);
    out
}

/// Removes a person's readable name while keeping every stored audit row unchanged.
///
/// # Errors
/// The database could not be written.
pub async fn forget_person(uow: &mut Uow<'_>, org: &str, subject: &str) -> Result<()> {
    store::forget_name(&mut *uow.tx, org, subject).await
}

/// Records something that changed nothing else (a refusal, a read).
///
/// # Errors
/// The database could not be written.
pub async fn record(
    app: &App,
    org: &str,
    who: &Who,
    action: &str,
    target: &str,
    outcome: &str,
) -> Result<()> {
    let mut uow = Uow::begin(app).await?;
    uow.audit(org, who, action, target, outcome).await?;
    uow.commit().await
}

/// # Errors
/// The database could not be read.
pub async fn recent(app: &App, org: &str, limit: i64) -> Result<Vec<Entry>> {
    let names = store::names(app.db.pool(), org).await?;
    let mut entries = store::recent(app.db.pool(), org, limit.clamp(1, 500)).await?;
    for entry in &mut entries {
        entry.actor = display_ref(&entry.actor, &names);
        entry.on_behalf_of = display_ref(&entry.on_behalf_of, &names);
        entry.target = display_target(&entry.target, &names);
    }
    Ok(entries)
}

/// Result of walking one organization's chain.
#[derive(Debug, PartialEq, Eq)]
pub struct Verdict {
    pub entries: i64,
    pub first_broken: Option<i64>,
}

/// # Errors
/// The database could not be read.
pub async fn verify(app: &App, org: &str) -> Result<Verdict> {
    let mut after = 0;
    let mut prev = GENESIS.to_owned();
    loop {
        let batch = store::since(app.db.pool(), org, after, 1000).await?;
        let Some(last) = batch.last() else {
            return Ok(Verdict {
                entries: after,
                first_broken: None,
            });
        };
        if let Some(seq) = first_broken(&batch, &prev, after + 1) {
            return Ok(Verdict {
                entries: after,
                first_broken: Some(seq),
            });
        }
        after = last.seq;
        prev.clone_from(&last.hash);
    }
}
