//! Validation for customer supplied storage endpoints.
use std::net::IpAddr;

/// Reject an empty or mixed DNS answer if any address is unsafe.
#[must_use]
pub fn addresses_allowed(addresses: &[IpAddr], allow_private: bool) -> bool {
    !addresses.is_empty()
        && addresses.iter().all(|ip| match ip {
            IpAddr::V4(v) => public_v4(*v, allow_private),
            IpAddr::V6(v) => {
                if let Some(v4) = v.to_ipv4_mapped() {
                    return public_v4(v4, allow_private);
                }
                let segments = v.segments();
                if (segments[0] == 0xfd00 && segments[1] == 0x0ec2)
                    || (segments[0] == 0xfd20 && segments[1] == 0x00ce)
                {
                    return false;
                }
                let unique_local = (segments[0] & 0xfe00) == 0xfc00;
                let public_unicast = (segments[0] & 0xe000) == 0x2000;
                if unique_local {
                    return allow_private;
                }
                public_unicast
                    && segments[0] != 0x2002
                    && !(segments[0] == 0x2001 && (segments[1] == 0 || segments[1] == 0x0db8))
            }
        })
}

fn public_v4(v: std::net::Ipv4Addr, allow_private: bool) -> bool {
    let [a, b, _, _] = v.octets();
    !((!allow_private && v.is_private())
        || v.is_loopback()
        || v.is_link_local()
        || v.is_unspecified()
        || v.is_broadcast()
        || v.is_multicast()
        || a == 0
        || a >= 240
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0)
        || (a == 198 && (b == 18 || b == 19)))
}

/// A URL and the address set checked at creation. Clients retain the original host for TLS and signing.
pub struct PinnedEndpoint {
    url: reqwest13::Url,
    addresses: Vec<std::net::SocketAddr>,
}

impl PinnedEndpoint {
    /// Parse and resolve a storage endpoint once, failing closed on mixed or empty answers.
    ///
    /// # Errors
    /// The URL, transport, DNS answer or destination is not permitted.
    pub async fn resolve(
        raw: &str,
        allow_private: bool,
        allow_http: bool,
    ) -> Result<Self, &'static str> {
        let url = reqwest13::Url::parse(raw).map_err(|_| "invalid endpoint")?;
        if !(url.scheme() == "https" || (allow_http && url.scheme() == "http"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("invalid endpoint");
        }
        let host = url.host_str().ok_or("invalid endpoint")?;
        let port = url.port_or_known_default().ok_or("invalid endpoint")?;
        let lookup = tokio::net::lookup_host((host.trim_matches(['[', ']']), port));
        let addresses: Vec<_> = tokio::time::timeout(std::time::Duration::from_secs(10), lookup)
            .await
            .map_err(|_| "endpoint resolution failed")?
            .map_err(|_| "endpoint resolution failed")?
            .collect();
        let ips: Vec<_> = addresses.iter().map(std::net::SocketAddr::ip).collect();
        let allowed = ips.iter().all(|ip| {
            addresses_allowed(&[*ip], allow_private)
                || (allow_private && allow_http && ip.is_loopback())
        });
        if ips.is_empty() || !allowed {
            return Err("endpoint address is not allowed");
        }
        Ok(Self { url, addresses })
    }

    /// Build the HTTP client used by the STS adapter with the same pinned addresses.
    ///
    /// # Errors
    /// The client could not be created.
    pub fn client12(&self) -> Result<reqwest::Client, &'static str> {
        let host = self
            .url
            .host_str()
            .ok_or("invalid endpoint")?
            .trim_matches(['[', ']']);
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(host, &self.addresses)
            .build()
            .map_err(|_| "could not create endpoint client")
    }

    /// Create a client that cannot follow redirects, use a proxy, or re-resolve the host.
    ///
    /// # Errors
    /// The HTTP client could not be created.
    pub fn client(&self) -> Result<reqwest13::Client, &'static str> {
        crate::objects::install_tls_provider();
        let host = self
            .url
            .host_str()
            .ok_or("invalid endpoint")?
            .trim_matches(['[', ']']);
        reqwest13::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .no_proxy()
            .redirect(reqwest13::redirect::Policy::none())
            .resolve_to_addrs(host, &self.addresses)
            .build()
            .map_err(|_| "could not create endpoint client")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn clients_use_the_pinned_address_and_do_not_follow_redirects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let endpoint = PinnedEndpoint {
            url: reqwest13::Url::parse(&format!(
                "http://not-a-real-host.invalid:{}",
                address.port()
            ))
            .unwrap(),
            addresses: vec![address],
        };
        let task = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 1024];
                let _ = stream.read(&mut request).await.unwrap();
                stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/\r\nContent-Length: 0\r\n\r\n").await.unwrap();
            }
        });
        let response = endpoint
            .client()
            .unwrap()
            .get(endpoint.url.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 302);
        let response = endpoint
            .client12()
            .unwrap()
            .get(endpoint.url.as_str())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 302);
        task.await.unwrap();
    }
}
