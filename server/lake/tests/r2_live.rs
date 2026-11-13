#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::doc_markdown,
    clippy::duration_suboptimal_units
)]
//! Against the real bucket, only when LOAMZ_TEST_R2_DIR names a folder with endpoint, key-id, secret and api-token files
//! (`~/.config/loamz/r2`). Proves a machine given issued credentials can read one volume and nothing else.

use std::time::Duration;

use futures::StreamExt;
use loamz_lake::{
    grants::{Issuer, R2Issuer},
    objects::{Bucket, Store},
};
use object_store::{ObjectStore, ObjectStoreExt, aws::AmazonS3Builder, path::Path};

#[tokio::test]
#[ignore = "needs LOAMZ_TEST_R2_DIR"]
async fn issued_credentials_reach_one_volume_only() {
    let dir = std::env::var("LOAMZ_TEST_R2_DIR")
        .expect("set LOAMZ_TEST_R2_DIR for the R2 integration test");
    let r = |n: &str| {
        std::fs::read_to_string(format!("{dir}/{n}"))
            .unwrap()
            .trim()
            .to_string()
    };
    let (endpoint, key_id) = (r("endpoint"), r("key-id"));
    let store = Store::bucket(&Bucket {
        endpoint: endpoint.clone(),
        name: "loamz-data".into(),
        key_id: key_id.clone(),
        secret: r("secret"),
    })
    .unwrap();
    let (a, b) = ("scope-test-a", "scope-test-b");
    for v in [a, b] {
        let mut up = store.begin_put(v, "f.txt", 100).await.unwrap();
        up.write(b"hello").await.unwrap();
        up.commit().await.unwrap();
    }
    let issuer = R2Issuer::new(&endpoint, "loamz-data", &key_id, &r("api-token")).unwrap();
    let c = issuer
        .issue(&[a], false, Duration::from_secs(120))
        .await
        .unwrap();
    let theirs = AmazonS3Builder::new()
        .with_endpoint(&endpoint)
        .with_bucket_name("loamz-data")
        .with_access_key_id(&c.key_id)
        .with_secret_access_key(&c.secret)
        .with_token(&c.session_token)
        .with_region("auto")
        .build()
        .unwrap();
    let own = theirs
        .get(&Path::from(format!("{a}/f.txt")))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let other = theirs.get(&Path::from(format!("{b}/f.txt"))).await;
    let write = theirs
        .put(&Path::from(format!("{a}/g.txt")), "x".into())
        .await;
    let list: Vec<_> = theirs.list(Some(&Path::from(b))).collect().await;
    store.remove_volume(a).await.unwrap();
    store.remove_volume(b).await.unwrap();
    assert_eq!(&own[..], b"hello");
    assert!(other.is_err(), "another volume must not be readable");
    assert!(write.is_err(), "read-only credentials must not write");
    assert!(
        list.iter().all(Result::is_err) && !list.is_empty() || list.is_empty(),
        "another volume must not be listable"
    );
}
