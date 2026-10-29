//! The audit entry and its hash chain. Pure: no database, no clock.

use sha2::{Digest, Sha256};

use crate::kernel::auth::Caller;

/// Who did it, and for whom. An agent acting for a person has both recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    pub actor: String,
    pub on_behalf_of: String,
}

impl Who {
    #[must_use]
    pub fn of(caller: &Caller) -> Self {
        match &caller.actor {
            Some(agent) => Self {
                actor: agent.clone(),
                on_behalf_of: caller.subject.clone(),
            },
            None => Self {
                actor: caller.subject.clone(),
                on_behalf_of: String::new(),
            },
        }
    }

    /// The server itself, finishing work someone asked for.
    #[must_use]
    pub fn system(on_behalf_of: &str) -> Self {
        Self {
            actor: "system".into(),
            on_behalf_of: on_behalf_of.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub seq: i64,
    pub at_ms: i64,
    pub tenant: String,
    pub actor: String,
    pub on_behalf_of: String,
    pub action: String,
    pub target: String,
    pub outcome: String,
    pub prev_hash: String,
    pub hash: String,
}

/// The first entry's `prev_hash`.
pub const GENESIS: &str = "";

/// Each field is length-prefixed, so two different entries can never produce the same bytes.
#[must_use]
pub fn hash(e: &Entry) -> String {
    let mut h = Sha256::new();
    for part in [
        &e.prev_hash,
        &e.seq.to_string(),
        &e.at_ms.to_string(),
        &e.tenant,
        &e.actor,
        &e.on_behalf_of,
        &e.action,
        &e.target,
        &e.outcome,
    ] {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part.as_bytes());
    }
    h.finalize().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The chain is intact if every entry links to the one before and carries the hash of its own content. Returns the
/// first `seq` that does not, or `None` if all is well. `prev` is what the first entry must link to.
#[must_use]
pub fn first_broken(entries: &[Entry], prev: &str, first_seq: i64) -> Option<i64> {
    let mut prev = prev.to_owned();
    for (seq, e) in (first_seq..).zip(entries) {
        if e.seq != seq || e.prev_hash != prev || e.hash != hash(e) {
            return Some(e.seq);
        }
        prev.clone_from(&e.hash);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(n: i64) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        for seq in 1..=n {
            let mut e = Entry {
                seq,
                at_ms: seq * 10,
                tenant: "t".into(),
                actor: "ana".into(),
                on_behalf_of: String::new(),
                action: "create".into(),
                target: format!("thing-{seq}"),
                outcome: "ok".into(),
                prev_hash: out
                    .last()
                    .map_or_else(|| GENESIS.to_owned(), |p| p.hash.clone()),
                hash: String::new(),
            };
            e.hash = hash(&e);
            out.push(e);
        }
        out
    }

    #[test]
    fn an_untouched_chain_is_intact() {
        assert_eq!(first_broken(&chain(5), GENESIS, 1), None);
    }

    #[test]
    fn changing_an_entry_is_found_at_that_entry() {
        let mut c = chain(5);
        c[2].outcome = "denied".into();
        assert_eq!(first_broken(&c, GENESIS, 1), Some(3));
    }

    #[test]
    fn removing_or_reordering_entries_is_found() {
        let mut c = chain(5);
        c.remove(1);
        assert_eq!(first_broken(&c, GENESIS, 1), Some(3));
        let mut c = chain(5);
        c.swap(1, 2);
        assert!(first_broken(&c, GENESIS, 1).is_some());
    }

    #[test]
    fn the_same_text_split_differently_hashes_differently() {
        let mut a = chain(1).remove(0);
        let mut b = a.clone();
        a.actor = "ab".into();
        a.on_behalf_of = "c".into();
        b.actor = "a".into();
        b.on_behalf_of = "bc".into();
        assert_ne!(hash(&a), hash(&b));
    }
}
