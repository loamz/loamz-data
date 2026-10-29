//! Writing and checking the trail. This is the bottom of the stack: it knows no other feature.

use super::{
    model::{Entry, GENESIS, Who, first_broken, hash},
    store,
};
use crate::kernel::{error::Result, now_ms, state::App};

/// Appends one entry. Writers take turns so the chain stays a single line.
///
/// # Errors
/// The database could not be written.
pub async fn record(
    app: &App,
    tenant: &str,
    who: &Who,
    action: &str,
    target: &str,
    outcome: &str,
) -> Result<()> {
    let _turn = app.write.lock().await;
    let prev = store::last(&app.db).await?;
    let mut e = Entry {
        seq: prev.as_ref().map_or(1, |p| p.seq + 1),
        at_ms: now_ms(),
        tenant: tenant.to_owned(),
        actor: who.actor.clone(),
        on_behalf_of: who.on_behalf_of.clone(),
        action: action.to_owned(),
        target: target.to_owned(),
        outcome: outcome.to_owned(),
        prev_hash: prev.map_or_else(|| GENESIS.to_owned(), |p| p.hash),
        hash: String::new(),
    };
    e.hash = hash(&e);
    store::insert(&app.db, &e).await
}

/// # Errors
/// The database could not be read.
pub async fn recent(app: &App, tenant: &str, limit: i64) -> Result<Vec<Entry>> {
    store::recent(&app.db, tenant, limit.clamp(1, 500)).await
}

/// Result of walking the whole chain.
#[derive(Debug, PartialEq, Eq)]
pub struct Verdict {
    pub entries: i64,
    pub first_broken: Option<i64>,
}

/// # Errors
/// The database could not be read.
pub async fn verify(app: &App) -> Result<Verdict> {
    let mut after = 0;
    let mut prev = GENESIS.to_owned();
    loop {
        let batch = store::since(&app.db, after, 1000).await?;
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
