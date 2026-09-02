use std::time::Duration;

use reqwest::{Client, redirect::Policy};

pub(crate) fn fixed_endpoint_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_mins(3))
        .https_only(true)
        .user_agent("AgentsAssemble/1.0")
        .build()
}
