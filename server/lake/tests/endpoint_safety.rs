#![allow(clippy::unwrap_used)]

use loamz_lake::endpoint::addresses_allowed;
use std::net::IpAddr;

#[test]
fn refuses_private_metadata_and_mapped_addresses() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "192.168.1.1",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "::192.168.1.1",
        "64:ff9b::a00:1",
        "2002:0a00:0001::",
        "fec0::1",
    ] {
        assert!(
            !addresses_allowed(&[address.parse::<IpAddr>().unwrap()], false),
            "{address}"
        );
    }
    assert!(!addresses_allowed(
        &["8.8.8.8".parse().unwrap(), "127.0.0.1".parse().unwrap()],
        false
    ));
    assert!(addresses_allowed(&["8.8.8.8".parse().unwrap()], false));
    assert!(addresses_allowed(&["10.0.0.1".parse().unwrap()], true));
    assert!(!addresses_allowed(
        &["169.254.169.254".parse().unwrap()],
        true
    ));
    assert!(!addresses_allowed(&["127.0.0.1".parse().unwrap()], true));
    assert!(!addresses_allowed(
        &["fd00:ec2::254".parse().unwrap()],
        true
    ));
    assert!(!addresses_allowed(&["fd20:ce::254".parse().unwrap()], true));
    assert!(!addresses_allowed(&[], true));
}

#[tokio::test]
async fn endpoint_rejects_unsafe_url_before_connecting() {
    use loamz_lake::endpoint::PinnedEndpoint;
    for url in [
        "http://example.com",
        "https://user:pass@example.com",
        "https://example.com/path",
        "https://example.com/?q=1",
        "https://127.0.0.1",
        "https://[::ffff:127.0.0.1]",
    ] {
        assert!(
            PinnedEndpoint::resolve(url, false, false).await.is_err(),
            "{url}"
        );
    }
}
