use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

use crate::Error;

/// Solami's account API: live gRPC connections, their history, and killing one.
/// The API key goes in `Authorization: Bearer`; `?api_key=` is rejected on these routes.
#[derive(Clone)]
pub struct AccountApi {
    http: reqwest::Client,
    base: String,
    api_key: String,
}

/// A live gRPC stream on the account. The client IP Solami reports is deliberately not kept.
#[derive(Clone, Debug, Deserialize)]
pub struct LiveConnection {
    pub conn_id: String,
    pub started_at: u64,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub bytes_streamed: u64,
    #[serde(default)]
    pub throughput_bps: u64,
    /// Capacity of the server-side send buffer, in messages.
    #[serde(default)]
    pub buffer_size: u64,
    /// Messages waiting in that buffer at the last five-second sample.
    #[serde(default)]
    pub buffer_pending: u64,
    #[serde(default, deserialize_with = "bool_or_int")]
    pub is_paygo: bool,
}

/// A stream that has closed, with Solami's reason for closing it.
#[derive(Clone, Debug, Deserialize)]
pub struct ClosedConnection {
    pub conn_id: String,
    pub started_at: u64,
    #[serde(default)]
    pub ended_at: Option<u64>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub bytes_streamed: u64,
    #[serde(default)]
    pub termination_reason: Option<String>,
}

#[derive(Deserialize)]
struct Killed {
    killed: bool,
}

impl AccountApi {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            http,
            base: base_url.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
        }
    }

    pub async fn live(&self) -> Result<Vec<LiveConnection>, Error> {
        self.send(self.http.get(self.url("/auth/connections/grpc")))
            .await
    }

    pub async fn history(&self) -> Result<Vec<ClosedConnection>, Error> {
        self.send(self.http.get(self.url("/auth/connections/grpc/history")))
            .await
    }

    /// Kill one live stream. Returns `false` if it had already closed.
    pub async fn kill(&self, conn_id: &str) -> Result<bool, Error> {
        let url = self.url(&format!("/auth/connections/grpc/{conn_id}"));
        let body: Killed = self.send(self.http.delete(url)).await?;
        Ok(body.killed)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    async fn send<T: DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T, Error> {
        let resp = req
            .bearer_auth(&self.api_key)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| Error::Account(e.without_url().to_string()))?;
        let status = resp.status();
        let body = resp
            .bytes()
            .await
            .map_err(|e| Error::Account(e.without_url().to_string()))?;
        if !status.is_success() {
            let text = String::from_utf8_lossy(&body);
            return Err(Error::Account(format!("HTTP {status}: {}", text.trim())));
        }
        serde_json::from_slice(&body)
            .map_err(|e| Error::Account(format!("unexpected response: {e}")))
    }
}

/// `is_paygo` is a bool on live connections and 0/1 in history.
fn bool_or_int<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Flag {
        Bool(bool),
        Int(i64),
    }
    Ok(match Flag::deserialize(d)? {
        Flag::Bool(b) => b,
        Flag::Int(n) => n != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_and_history_rows_without_keeping_ips() {
        let live: Vec<LiveConnection> = serde_json::from_str(
            r#"[{"conn_id":"iWu5UZltjWDm","ip":"203.0.113.7","started_at":1790380681,"region":"ams",
                "bytes_streamed":6489966,"throughput_bps":1297993,"buffer_size":8192,"buffer_pending":0,"is_paygo":false}]"#,
        )
        .unwrap();
        assert_eq!(live[0].buffer_size, 8192);

        let history: Vec<ClosedConnection> = serde_json::from_str(
            r#"[{"uid":"u","region":"ams","conn_id":"OiAIK_RI3aGd","conn_type":"grpc","ip":"203.0.113.7","backend":"",
                "backend_region":"","is_paygo":0,"bytes_streamed":0,"started_at":1790380320,"ended_at":1790380325,
                "termination_reason":"backpressure"}]"#,
        )
        .unwrap();
        assert_eq!(
            history[0].termination_reason.as_deref(),
            Some("backpressure")
        );
    }
}
