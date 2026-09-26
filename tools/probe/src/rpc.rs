use std::time::Duration;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::env::Env;

/// Minimal JSON-RPC client. Solami reports rate limits inside an HTTP 200 body,
/// so every response is checked for an `error` object, not just the status code.
#[derive(Clone)]
pub struct Rpc {
    http: reqwest::Client,
    url: String,
}

#[derive(Debug)]
pub enum RpcError {
    RateLimited,
    /// The slot was skipped by the leader or is not available as a block.
    NoBlock {
        code: i64,
        message: String,
    },
    Other {
        code: i64,
        message: String,
    },
    Transport(String),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimited => write!(f, "rate limited (-32005)"),
            Self::NoBlock { code, message } | Self::Other { code, message } => {
                write!(f, "{code}: {message}")
            }
            Self::Transport(e) => write!(f, "transport: {e}"),
        }
    }
}

impl std::error::Error for RpcError {}

impl Rpc {
    pub fn new(env: &Env) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        // The key rides in the query string; never log `url`.
        let url = format!("{}?api_key={}", env.rpc_url, env.api_key);
        Self { http, url }
    }

    /// Returns the `result` and the response size in bytes.
    pub async fn call(&self, method: &str, params: Value) -> Result<(Value, usize), RpcError> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let resp = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| RpcError::Transport(e.without_url().to_string()))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| RpcError::Transport(e.without_url().to_string()))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| RpcError::Transport(format!("HTTP {status}, non-JSON body")))?;
        if let Some(err) = value.get("error") {
            let code = err.get("code").and_then(Value::as_i64).unwrap_or_default();
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            return Err(match code {
                -32005 => RpcError::RateLimited,
                // -32004 block not available, -32007 slot skipped, -32009 missing in long-term storage
                -32004 | -32007 | -32009 => RpcError::NoBlock { code, message },
                _ => RpcError::Other { code, message },
            });
        }
        Ok((
            value.get("result").cloned().unwrap_or(Value::Null),
            bytes.len(),
        ))
    }

    pub async fn get_slot(&self, commitment: &str) -> Result<u64> {
        let (v, _) = self
            .call("getSlot", json!([{ "commitment": commitment }]))
            .await?;
        v.as_u64().ok_or_else(|| anyhow!("getSlot returned {v}"))
    }

    /// `getBlock` with full JSON transactions. Solami rejects `transactionDetails: accounts`,
    /// so account keys come from `message.accountKeys` plus `meta.loadedAddresses`.
    /// Mainnet carries v1 transactions, and recent confirmed blocks come back with an empty
    /// transaction list until Solami finishes indexing them, so read at `finalized`.
    pub async fn get_block(&self, slot: u64) -> Result<(Value, usize), RpcError> {
        self.call(
            "getBlock",
            json!([slot, {
                "encoding": "json",
                "transactionDetails": "full",
                "rewards": false,
                "commitment": "finalized",
                "maxSupportedTransactionVersion": 1
            }]),
        )
        .await
    }
}
