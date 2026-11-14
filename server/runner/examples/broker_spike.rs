#![allow(clippy::expect_used)]
//! A measuring tool for the storage broker, not part of the product: starts a broker for one folder of a bucket with the given key,
//! and writes how many bytes it has read from storage, and how many reads its cache answered, to `<dir>/stats.json` every 100 ms.
//!
//!     `broker_spike <dir> <endpoint> <bucket> <key id> <secret> <folder>`
//!
//! `bench/broker_readers.py` puts a loopback port in front of `<dir>/socket` (as the sandbox does) and reads Parquet through it with
//! Polars, `DuckDB` and pyarrow.

use std::{path::Path, time::Duration};

use loamz_runner::broker::Broker;
use serde_json::json;

#[tokio::main]
async fn main() {
    // As the runner does at start: the object-store client needs a TLS provider installed before it is built.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 7 {
        eprintln!("usage: broker_spike <dir> <endpoint> <bucket> <key id> <secret> <folder>");
        std::process::exit(2);
    }
    let dir = Path::new(&a[1]);
    let broker = Broker::start(dir).expect("the broker could not start");
    let storage = json!({
        "bucket": a[3], "endpoint": a[2], "key_id": a[4], "secret": a[5], "session_token": "",
        "tables": {"t": {"folder": a[6]}},
    });
    broker
        .authorize(Some(&storage), &json!({"tables": [{"id": "t"}]}))
        .await
        .expect("the grant was refused");
    loop {
        let stats = json!({"bytes": broker.bytes_read(), "hits": broker.cache_hits()});
        let _ = std::fs::write(dir.join("stats.json"), stats.to_string());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
