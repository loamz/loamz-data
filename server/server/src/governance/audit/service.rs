//! Writing and checking the trail. This is the bottom of the stack: it knows no other feature.
//!
//! Every change that must be recorded is a [`Uow`]: the change and its audit entry commit together or not at all, so
//! there is never a change without a record or a record of a change that did not happen.

use sqlx::{Any, Transaction};
use tokio::sync::MutexGuard;

use super::{
    model::{Entry, GENESIS, Who, first_broken, hash},
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
        let mut e = Entry {
            org: org.to_owned(),
            seq: prev.as_ref().map_or(1, |p| p.seq + 1),
            at_ms: now_ms(),
            actor: who.actor.clone(),
            on_behalf_of: who.on_behalf_of.clone(),
            action: action.to_owned(),
            target: target.to_owned(),
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
    store::recent(app.db.pool(), org, limit.clamp(1, 500)).await
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
