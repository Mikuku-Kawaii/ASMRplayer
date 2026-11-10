use super::fail;
use crate::domain::AppResult;
use reqwest::blocking::Client;
use std::time::Duration;
pub(super) fn client() -> AppResult<Client> {
    Client::builder()
        .user_agent("ASMR-Player-Sync")
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| fail("无法创建同步连接"))
}
