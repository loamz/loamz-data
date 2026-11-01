//! A small in-memory cache of immutable bytes, for things that are named by a unique id and never change once written (a table's
//! metadata files). It holds at most `limit` bytes and, when full, starts over: simple, bounded, and never wrong.

use std::{collections::HashMap, sync::Arc};

pub struct BoundedCache {
    limit: usize,
    used: usize,
    items: HashMap<String, Arc<Vec<u8>>>,
}

impl BoundedCache {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            used: 0,
            items: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        self.items.get(key).cloned()
    }

    /// Keeps `bytes` under `key`. Something bigger than the whole cache is not kept.
    pub fn put(&mut self, key: String, bytes: Arc<Vec<u8>>) {
        if bytes.len() > self.limit {
            return;
        }
        if self.used + bytes.len() > self.limit {
            self.items.clear();
            self.used = 0;
        }
        self.used += bytes.len();
        if let Some(old) = self.items.insert(key, bytes) {
            self.used = self.used.saturating_sub(old.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keeps_what_fits_and_starts_over_when_full() {
        let mut c = BoundedCache::new(10);
        c.put("a".into(), Arc::new(vec![0; 6]));
        assert!(c.get("a").is_some());
        c.put("b".into(), Arc::new(vec![0; 6]));
        assert!(
            c.get("a").is_none() && c.get("b").is_some(),
            "full: starts over"
        );
        c.put("huge".into(), Arc::new(vec![0; 11]));
        assert!(
            c.get("huge").is_none(),
            "bigger than the whole cache: not kept"
        );
    }
}
