use std::time::Duration;

use reqwest::{Client, ClientBuilder, redirect::Policy};

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

fn fixed_endpoint_builder() -> ClientBuilder {
    Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .https_only(true)
        .user_agent("AgentsAssemble/1.0")
}
