use futures_util::StreamExt;
use serde_json::Value;
use std::{error::Error, io, net::IpAddr, time::Duration};
use tokio_util::sync::CancellationToken;

use ip_network::IpNetwork;
use reqwest::{
    Client, ClientBuilder,
    dns::{Addrs, Name, Resolve, Resolving},
    redirect::Policy,
};

const DNS_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
struct PublicHostResolver {
    expected_host: String,
}

pub(crate) fn fixed_endpoint_client() -> Result<Client, reqwest::Error> {
    fixed_endpoint_builder()
        .read_timeout(Duration::from_mins(3))
        .build()
}

pub(crate) fn fixed_catalog_client() -> Result<Client, reqwest::Error> {
    fixed_endpoint_builder()
        .timeout(Duration::from_secs(8))
        .build()
}

pub(crate) fn custom_endpoint_client(expected_host: String) -> Result<Client, reqwest::Error> {
    fixed_endpoint_builder()
        .no_proxy()
        .dns_resolver(PublicHostResolver { expected_host })
        .read_timeout(Duration::from_mins(3))
        .build()
}

fn fixed_endpoint_builder() -> ClientBuilder {
    Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .https_only(true)
        .user_agent("AgentsAssemble/1.0")
}

impl Resolve for PublicHostResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let requested = name.as_str().trim_end_matches('.').to_ascii_lowercase();
        let expected = self.expected_host.clone();
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

pub(crate) fn public_unicast(address: IpAddr) -> bool {
    let address = match address {
        IpAddr::V6(value) => value.to_ipv4_mapped().map_or(IpAddr::V6(value), IpAddr::V4),
        value @ IpAddr::V4(_) => value,
    };
    !address.is_multicast() && IpNetwork::from(address).is_global()
}

fn resolution_error(kind: io::ErrorKind, message: &'static str) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(kind, message))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteReadError {
    Cancelled,
    Timeout,
    Authentication,
    Malformed,
    Failed,
    TooLarge,
}

pub(crate) async fn fetch_bounded_json(
    request: reqwest::RequestBuilder,
    max_bytes: usize,
    cancellation: &CancellationToken,
) -> Result<Value, RemoteReadError> {
    if cancellation.is_cancelled() {
        return Err(RemoteReadError::Cancelled);
    }
    let response = tokio::select! {
        biased;
        () = cancellation.cancelled() => return Err(RemoteReadError::Cancelled),
        result = request.header(reqwest::header::ACCEPT, "application/json").send() => {
            result.map_err(|error| {
                if error.is_timeout() {
                    RemoteReadError::Timeout
                } else {
                    RemoteReadError::Failed
                }
            })?
        }
    };
    match response.status() {
        status if status.is_success() => {}
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            return Err(RemoteReadError::Authentication);
        }
        reqwest::StatusCode::REQUEST_TIMEOUT | reqwest::StatusCode::GATEWAY_TIMEOUT => {
            return Err(RemoteReadError::Timeout);
        }
        _ => return Err(RemoteReadError::Failed),
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(RemoteReadError::TooLarge);
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    loop {
        let next = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(RemoteReadError::Cancelled),
            next = stream.next() => next,
        };
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(|error| {
            if error.is_timeout() {
                RemoteReadError::Timeout
            } else {
                RemoteReadError::Failed
            }
        })?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(RemoteReadError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| RemoteReadError::Malformed)
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::public_unicast;

    #[test]
    fn custom_credentials_accept_only_public_unicast_addresses() {
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
