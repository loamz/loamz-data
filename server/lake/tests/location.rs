#![allow(clippy::unwrap_used)]

use loamz_lake::location::{LocationCredential, LocationError, LocationSecret, build_location};

fn secret() -> LocationSecret {
    LocationSecret::new("test-key".into(), "test-secret".into(), None)
}

#[tokio::test]
async fn refuses_unsafe_endpoints_and_unsupported_aws() {
    let mut credential = LocationCredential {
        provider: "rustfs".into(),
        endpoint: "https://127.0.0.1".into(),
        region: "us-east-1".into(),
        bucket: "my-bucket".into(),
        prefix: "data".into(),
        generation: 1,
        allow_private: false,
        allow_http: false,
    };
    assert!(matches!(
        build_location(&credential, &secret()).await,
        Err(LocationError::Unsupported)
    ));
    credential.prefix = "../other".into();
    assert!(matches!(
        build_location(&credential, &secret()).await,
        Err(LocationError::Unsupported)
    ));
    credential.prefix = "data".into();
    credential.provider = "aws".into();
    assert!(matches!(
        build_location(&credential, &secret()).await,
        Err(LocationError::Unsupported)
    ));
}

#[test]
fn issued_credentials_debug_is_redacted() {
    let c = loamz_lake::grants::Credentials {
        key_id: "canary-key".into(),
        secret: "canary-secret".into(),
        session_token: "canary-token".into(),
    };
    assert!(!format!("{c:?}").contains("canary"));
}

#[tokio::test]
async fn the_borrowed_secret_is_consumed_once() {
    let credential = LocationCredential {
        provider: "rustfs".into(),
        endpoint: "http://127.0.0.1:9000".into(),
        region: "us-east-1".into(),
        bucket: "my-bucket".into(),
        prefix: "data".into(),
        generation: 7,
        allow_private: true,
        allow_http: true,
    };
    let input = secret();
    let handle = build_location(&credential, &input).await.unwrap();
    assert_eq!(handle.generation, 7);
    assert!(matches!(
        build_location(&credential, &input).await,
        Err(LocationError::Rejected)
    ));
}
