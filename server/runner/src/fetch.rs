//! Downloads the one file an import names, on this machine, before the sandbox starts. The sandbox has no network, so this is the only
//! place a person's URL is ever opened, and it is opened with strict rules because this machine is on the customer's own network:
//!
//! * HTTPS on port 443 only, and no user name or password in the address;
//! * every address the name resolves to must be public (no loopback, private, link-local, shared, documentation or multicast range,
//!   and no IPv6 form that wraps one), and the connection is made to the address that was checked, so the name cannot change after;
//! * a redirect is followed at most three times, and each hop goes through the same checks;
//! * a size cap and a time cap, and nothing is kept when either is passed.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::Path,
    time::Duration,
};

use reqwest::{Client, StatusCode, Url, redirect::Policy};
use tokio::{fs::File, io::AsyncWriteExt};

/// The most a download may be, and the longest it may take in all.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
#[allow(clippy::duration_suboptimal_units)]
pub const MAX_TIME: Duration = Duration::from_secs(600);
const MAX_REDIRECTS: usize = 3;

/// Whether an address belongs to the public internet: the only kind an import may reach.
#[must_use]
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        // Shared address space, used by carriers and by some cloud networks (100.64.0.0/10).
        || (a == 100 && (64..128).contains(&b))
        // Protocol assignments (192.0.0.0/24), and benchmarking (198.18.0.0/15).
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b == 18 || b == 19))
        // Reserved for future use (240.0.0.0/4).
        || a >= 240)
}

fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = ip.segments();
    // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) carry an IPv4 address inside; judge that one.
    if s[0] == 0x0064 && s[1] == 0xff9b && s[2..6] == [0; 4] {
        return public_v4(Ipv4Addr::new(
            (s[6] >> 8) as u8,
            (s[6] & 0xff) as u8,
            (s[7] >> 8) as u8,
            (s[7] & 0xff) as u8,
        ));
    }
    if s[0] == 0x2002 {
        return public_v4(Ipv4Addr::new(
            (s[1] >> 8) as u8,
            (s[1] & 0xff) as u8,
            (s[2] >> 8) as u8,
            (s[2] & 0xff) as u8,
        ));
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // Unique local (fc00::/7) and link-local (fe80::/10).
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfe80
        // Documentation (2001:db8::/32).
        || (s[0] == 0x2001 && s[1] == 0x0db8))
}

/// Checks the address as written, before any lookup: the rules that need no network.
///
/// # Errors
/// Says why the address may not be fetched.
pub fn check_url(url: &Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err("the address must start with https://".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("the address must not contain a user name or password".into());
    }
    if url.port().is_some_and(|p| p != 443) {
        return Err("the address must use the standard HTTPS port".into());
    }
    if url.host_str().is_none() {
        return Err("the address has no host".into());
    }
    Ok(())
}

/// Looks the host up and keeps only a result every address of which is public. Returns what to connect to.
async fn resolve(url: &Url) -> Result<(String, Vec<SocketAddr>), String> {
    let host = url.host_str().ok_or("the address has no host")?.to_owned();
    let literal = host.trim_start_matches('[').trim_end_matches(']');
    let found: Vec<SocketAddr> = if let Ok(ip) = literal.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, 443)]
    } else {
        tokio::time::timeout(Duration::from_secs(10), tokio::net::lookup_host((host.as_str(), 443)))
            .await
            .map_err(|_| "looking up the host took too long".to_owned())?
            .map_err(|_| "the host could not be found".to_owned())?
            .collect()
    };
    if found.is_empty() {
        return Err("the host could not be found".into());
    }
    if found.iter().any(|a| !is_public(a.ip())) {
        return Err("the address is not on the public internet".into());
    }
    Ok((host, found))
}

/// Downloads `address` into `dest`, under the rules above. Nothing is left at `dest` unless the whole file arrived.
///
/// # Errors
/// A sentence for the person: it never repeats the address (it may carry a signature).
pub async fn download(address: &str, dest: &Path) -> Result<u64, String> {
    let work = download_inner(address, dest);
    let done = tokio::time::timeout(MAX_TIME, work)
        .await
        .unwrap_or_else(|_| Err("the download took too long".into()));
    if done.is_err() {
        let _ = tokio::fs::remove_file(dest).await;
    }
    done
}

async fn download_inner(address: &str, dest: &Path) -> Result<u64, String> {
    let mut url = Url::parse(address).map_err(|_| "the address is not valid".to_owned())?;
    for _ in 0..=MAX_REDIRECTS {
        check_url(&url)?;
        let (host, addrs) = resolve(&url).await?;
        // Connect only to what was checked, whatever the name says later; redirects are followed here, one hop at a time.
        let client = Client::builder()
            .redirect(Policy::none())
            .resolve_to_addrs(&host, &addrs)
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| "the download could not start".to_owned())?;
        let mut reply = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| "the server could not be reached".to_owned())?;
        let status = reply.status();
        if status.is_redirection() {
            let next = reply
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or("the server redirected without saying where")?;
            url = url
                .join(next)
                .map_err(|_| "the server redirected to an address that is not valid".to_owned())?;
            continue;
        }
        if status != StatusCode::OK {
            return Err(format!("the server answered {}", status.as_u16()));
        }
        if reply.content_length().is_some_and(|n| n > MAX_BYTES) {
            return Err("the file is larger than 2 GiB".into());
        }
        let mut file = File::create(dest)
            .await
            .map_err(|e| format!("the file could not be saved: {e}"))?;
        let mut total = 0u64;
        while let Some(chunk) = reply
            .chunk()
            .await
            .map_err(|_| "the download was cut off".to_owned())?
        {
            total += chunk.len() as u64;
            if total > MAX_BYTES {
                return Err("the file is larger than 2 GiB".into());
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("the file could not be saved: {e}"))?;
        }
        file.flush().await.map_err(|e| e.to_string())?;
        return Ok(total);
    }
    Err("the address redirects too many times".into())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn only_public_addresses_pass() {
        for ok in ["8.8.8.8", "1.1.1.1", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(is_public(ip(ok)), "{ok} should be public");
        }
        for bad in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "100.127.255.255",
            "192.0.0.8",
            "198.18.0.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd00:ec2::254",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::7f00:1",
            "2002:7f00:1::",
        ] {
            assert!(!is_public(ip(bad)), "{bad} should be refused");
        }
    }

    #[test]
    fn the_address_as_written_is_checked() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert!(check_url(&url("https://example.com/data.csv?sig=abc")).is_ok());
        assert!(check_url(&url("https://example.com:443/data.csv")).is_ok());
        for bad in [
            "http://example.com/data.csv",
            "ftp://example.com/data.csv",
            "file:///etc/passwd",
            "https://user:pw@example.com/data.csv",
            "https://user@example.com/data.csv",
            "https://example.com:8443/data.csv",
        ] {
            assert!(check_url(&url(bad)).is_err(), "{bad} should be refused");
        }
    }

    #[tokio::test]
    async fn names_and_literals_for_private_hosts_are_refused_before_any_request() {
        let dir = std::env::temp_dir().join(format!("loamz-fetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("out");
        for bad in [
            "https://127.0.0.1/data.csv",
            "https://[::1]/data.csv",
            "https://169.254.169.254/latest/meta-data/",
            "https://localhost/data.csv",
            "https://10.0.0.5/data.csv",
        ] {
            let err = download(bad, &dest).await.unwrap_err();
            assert!(!err.contains(bad), "the address must not be repeated: {err}");
            assert!(!dest.exists(), "nothing is left behind for {bad}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
