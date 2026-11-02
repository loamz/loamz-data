//! Exercises the object-store adapter against an isolated S3-compatible test service.
//! Create an empty bucket `loamz-test` on the service and set `LOAMZ_TEST_S3_ENDPOINT`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use futures::TryStreamExt;
use loamz_lake::objects::{Store, install_tls_provider};
use object_store::aws::AmazonS3Builder;

#[tokio::test]
#[ignore = "requires an isolated S3-compatible test service"]
async fn files_and_multipart_uploads_work_on_s3() {
    let endpoint = std::env::var("LOAMZ_TEST_S3_ENDPOINT").expect("set LOAMZ_TEST_S3_ENDPOINT");
    install_tls_provider();
    let bucket = "loamz-test";
    let s3 = AmazonS3Builder::new()
        .with_endpoint(&endpoint)
        .with_allow_http(true)
        .with_bucket_name(bucket)
        .with_access_key_id("test")
        .with_secret_access_key("test")
        .with_region("auto")
        .build()
        .unwrap();
    let store = Store::Bucket {
        s3: Arc::new(s3),
        endpoint,
        name: bucket.into(),
    };
    let a = format!("s3test{}a", std::process::id());
    let b = format!("s3test{}b", std::process::id());
    let mut upload = store.begin_put(&a, "nested/small.txt", 100).await.unwrap();
    upload.write(b"hello").await.unwrap();
    assert_eq!(upload.commit().await.unwrap(), 5);
    let mut other = store.begin_put(&b, "other.txt", 100).await.unwrap();
    other.write(b"other").await.unwrap();
    other.commit().await.unwrap();

    let large = vec![b'x'; 8 * 1024 * 1024 + 1];
    let mut upload = store
        .begin_put(&a, "large.bin", 10 * 1024 * 1024)
        .await
        .unwrap();
    upload.write(&large).await.unwrap();
    assert_eq!(upload.commit().await.unwrap(), large.len() as u64);
    let (stream, size) = store.open(&a, "large.bin").await.unwrap();
    let chunks: Vec<_> = stream.try_collect().await.unwrap();
    let actual: Vec<u8> = chunks.into_iter().flatten().collect();
    assert_eq!((size, actual), (large.len() as u64, large));
    assert_eq!(store.list(&a).await.unwrap().len(), 2);
    store.remove_volume(&a).await.unwrap();
    assert!(!store.exists(&a).await.unwrap());
    assert!(
        store.exists(&b).await.unwrap(),
        "removing one volume must not remove another"
    );
    store.remove_volume(&b).await.unwrap();
}
