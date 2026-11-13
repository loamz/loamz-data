#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::duration_suboptimal_units
)]
//! Against a real S3-compatible store that has STS, only when `LOAMZ_TEST_STS_ENDPOINT` is set (`RustFS` from `deploy/dev/compose.yml`
//! is `http://127.0.0.1:9000`; the bucket `loamz-data` must exist, key `loamz-dev` / `loamz-dev-only`). Proves a machine given
//! credentials from `StsIssuer` can read one volume and nothing else, and can write only when write was asked for.

use std::time::Duration;

use futures::StreamExt;
use loamz_lake::{
    grants::{Issuer, StsIssuer},
    objects::{Bucket, Store, install_tls_provider},
};
use object_store::{ObjectStore, ObjectStoreExt, aws::AmazonS3Builder, path::Path};

#[tokio::test]
#[ignore = "needs LOAMZ_TEST_STS_ENDPOINT"]
async fn issued_credentials_reach_one_volume_only() {
    install_tls_provider();
    let endpoint = std::env::var("LOAMZ_TEST_STS_ENDPOINT").expect("set LOAMZ_TEST_STS_ENDPOINT");
    let (key_id, secret, bucket) = ("loamz-dev", "loamz-dev-only", "loamz-data");
    let store = Store::bucket(&Bucket {
        endpoint: endpoint.clone(),
        name: bucket.into(),
        key_id: key_id.into(),
        secret: secret.into(),
    })
    .unwrap();
    let id = std::process::id();
    let (a, b) = (format!("sts{id}a"), format!("sts{id}b"));
    for v in [&a, &b] {
        let mut up = store.begin_put(v, "f.txt", 100).await.unwrap();
        up.write(b"hello").await.unwrap();
        up.commit().await.unwrap();
    }
    let issuer = StsIssuer::new(&endpoint, "us-east-1", bucket, key_id, secret, None).unwrap();
    let theirs = |c: &loamz_lake::grants::Credentials| {
        AmazonS3Builder::new()
            .with_endpoint(&endpoint)
            .with_allow_http(true)
            .with_bucket_name(bucket)
            .with_access_key_id(&c.key_id)
            .with_secret_access_key(&c.secret)
            .with_token(&c.session_token)
            .with_region("us-east-1")
            .build()
            .unwrap()
    };

    let read = theirs(
        &issuer
            .issue(&[&a], false, Duration::from_secs(900))
            .await
            .unwrap(),
    );
    let own = read
        .get(&Path::from(format!("{a}/f.txt")))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let other = read.get(&Path::from(format!("{b}/f.txt"))).await;
    let refused_write = read
        .put(&Path::from(format!("{a}/g.txt")), "x".into())
        .await;
    let list_own: Vec<_> = read.list(Some(&Path::from(a.as_str()))).collect().await;
    let list_other: Vec<_> = read.list(Some(&Path::from(b.as_str()))).collect().await;

    let rw = theirs(
        &issuer
            .issue(&[&a], true, Duration::from_secs(900))
            .await
            .unwrap(),
    );
    let written = rw.put(&Path::from(format!("{a}/g.txt")), "x".into()).await;
    let wrote_other = rw.put(&Path::from(format!("{b}/g.txt")), "x".into()).await;

    store.remove_volume(&a).await.unwrap();
    store.remove_volume(&b).await.unwrap();
    assert_eq!(&own[..], b"hello");
    assert!(other.is_err(), "another volume must not be readable");
    assert!(
        refused_write.is_err(),
        "read-only credentials must not write"
    );
    assert!(
        list_own.iter().all(Result::is_ok) && !list_own.is_empty(),
        "own volume is listable"
    );
    assert!(
        list_other.iter().all(Result::is_err) || list_other.is_empty(),
        "another volume must not be listable"
    );
    assert!(written.is_ok(), "write credentials write: {written:?}");
    assert!(
        wrote_other.is_err(),
        "write credentials stay in their folder"
    );
}
