use std::{error::Error, io, net::IpAddr, time::Duration};

use ip_network::IpNetwork;
use reqwest::{
    Client,
    dns::{Addrs, Name, Resolve, Resolving},
    redirect::Policy,
};

const DNS_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
struct PublicHostResolver {
    expected_host: &'static str,
}

pub(crate) fn direct_client(expected_host: &'static str) -> Result<Client, reqwest::Error> {
    Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .dns_resolver(PublicHostResolver { expected_host })
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_mins(3))
        .https_only(true)
        .user_agent("AgentsAssemble/1.0")
        .build()
}

impl Resolve for PublicHostResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let requested = name.as_str().to_ascii_lowercase();
        let expected = self.expected_host;
        Box::pin(async move {
            if requested != expected {
                return Err(resolution_error(
                    io::ErrorKind::PermissionDenied,
                    "credentialed HTTPS resolution requested an unexpected host",
                ));
            }
            let resolved = match tokio::time::timeout(
                DNS_TIMEOUT,
                tokio::net::lookup_host((requested.as_str(), 0)),
            )
            .await
            {
                Ok(Ok(resolved)) => resolved,
                Ok(Err(error)) => return Err(Box::new(error) as Box<dyn Error + Send + Sync>),
                Err(_) => {
                    return Err(resolution_error(
                        io::ErrorKind::TimedOut,
                        "credentialed HTTPS DNS resolution timed out",
                    ));
                }
            };
            let mut addresses = Vec::new();
            for address in resolved {
                if !public_unicast(address.ip()) {
                    return Err(resolution_error(
                        io::ErrorKind::PermissionDenied,
                        "credentialed HTTPS DNS returned a non-public address",
                    ));
                }
                if !addresses.contains(&address) {
                    addresses.push(address);
                }
            }
            if addresses.is_empty() {
                return Err(resolution_error(
                    io::ErrorKind::NotFound,
                    "credentialed HTTPS DNS returned no usable address",
                ));
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

fn public_unicast(address: IpAddr) -> bool {
    let address = match address {
        IpAddr::V6(value) => value.to_ipv4_mapped().map_or(IpAddr::V6(value), IpAddr::V4),
        value @ IpAddr::V4(_) => value,
    };
    !address.is_multicast() && IpNetwork::from(address).is_global()
}

fn resolution_error(kind: io::ErrorKind, message: &'static str) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(kind, message))
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::public_unicast;

    #[test]
    fn credentialed_https_accepts_only_public_unicast_addresses() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "192.0.2.1",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
        ] {
            let address = address
                .parse::<IpAddr>()
                .unwrap_or_else(|error| panic!("parse blocked address: {error}"));
            assert!(!public_unicast(address), "accepted {address}");
        }
        for address in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            let address = address
                .parse::<IpAddr>()
                .unwrap_or_else(|error| panic!("parse public address: {error}"));
            assert!(public_unicast(address), "rejected {address}");
        }
    }
}
