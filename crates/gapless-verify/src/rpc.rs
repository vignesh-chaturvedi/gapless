use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::Error;

/// A JSON-RPC client for Solami. Solami reports rate limits inside an HTTP 200 body
/// (`-32005`), so every response is checked for an `error` object and retried with backoff.
#[derive(Clone)]
pub struct Rpc {
    http: reqwest::Client,
    url: String,
    stats: Arc<Counters>,
}

#[derive(Default)]
struct Counters {
    calls: AtomicU64,
    bytes: AtomicU64,
    retries: AtomicU64,
}

/// RPC usage, for reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RpcStats {
    pub calls: u64,
    pub bytes: u64,
    pub retries: u64,
}

impl RpcStats {
    pub fn since(self, earlier: RpcStats) -> RpcStats {
        RpcStats {
            calls: self.calls - earlier.calls,
            bytes: self.bytes - earlier.bytes,
            retries: self.retries - earlier.retries,
        }
    }
}

const MAX_ATTEMPTS: u32 = 7;

impl Rpc {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        // The key rides in the query string; never log `url`.
        let sep = if base_url.contains('?') { '&' } else { '?' };
        Self {
            http,
            url: format!("{base_url}{sep}api_key={api_key}"),
            stats: Arc::default(),
        }
    }

    pub fn stats(&self) -> RpcStats {
        RpcStats {
            calls: self.stats.calls.load(Ordering::Relaxed),
            bytes: self.stats.bytes.load(Ordering::Relaxed),
            retries: self.stats.retries.load(Ordering::Relaxed),
        }
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, Error> {
        let mut delay = Duration::from_millis(100);
        for attempt in 1..=MAX_ATTEMPTS {
            match self.call_once(method, &params).await {
                Err(Error::RateLimited | Error::Transport(_)) if attempt < MAX_ATTEMPTS => {
                    self.stats.retries.fetch_add(1, Ordering::Relaxed);
                    let jitter =
                        Duration::from_millis(fastrand::u64(0..=delay.as_millis() as u64 / 2));
                    tokio::time::sleep(delay + jitter).await;
                    delay = (delay * 2).min(Duration::from_secs(5));
                }
                other => return other,
            }
        }
        unreachable!("the last attempt returns")
    }

    async fn call_once(&self, method: &str, params: &Value) -> Result<Value, Error> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        self.stats.calls.fetch_add(1, Ordering::Relaxed);
        let resp = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Transport(e.without_url().to_string()))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::Transport(e.without_url().to_string()))?;
        self.stats
            .bytes
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Transport(format!("HTTP {status} with a non-JSON body")))?;
        if let Some(err) = value.get("error") {
            let code = err.get("code").and_then(Value::as_i64).unwrap_or_default();
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            return Err(match code {
                -32005 => Error::RateLimited,
                // -32004 block not available, -32007 slot skipped, -32009 missing in long-term storage
                -32004 | -32007 | -32009 => Error::NoBlock { code, message },
                _ => Error::Rpc {
                    method: method.to_owned(),
                    code,
                    message,
                },
            });
        }
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn slot(&self, commitment: &str) -> Result<u64, Error> {
        let v = self
            .call("getSlot", json!([{ "commitment": commitment }]))
            .await?;
        v.as_u64()
            .ok_or_else(|| Error::Unexpected(format!("getSlot returned {v}")))
    }

    /// Slots in the range that have a finalized block. The rest were skipped by their leader.
    pub async fn finalized_blocks(&self, first: u64, last: u64) -> Result<Vec<u64>, Error> {
        let v = self
            .call(
                "getBlocks",
                json!([first, last, { "commitment": "finalized" }]),
            )
            .await?;
        v.as_array()
            .map(|slots| slots.iter().filter_map(Value::as_u64).collect())
            .ok_or_else(|| Error::Unexpected(format!("getBlocks returned {v}")))
    }

    /// A finalized transaction, or `None` if RPC doesn't know it.
    pub async fn transaction(&self, signature: &str) -> Result<Option<Value>, Error> {
        let params = json!([signature, {
            "encoding": "json",
            "commitment": "finalized",
            "maxSupportedTransactionVersion": 1
        }]);
        let tx = self.call("getTransaction", params).await?;
        Ok((!tx.is_null()).then_some(tx))
    }

    /// A finalized block with full JSON transactions, or `None` if the slot has no block.
    /// Mainnet carries v1 transactions, so `maxSupportedTransactionVersion` is 1.
    pub async fn block(&self, slot: u64) -> Result<Option<Value>, Error> {
        let params = json!([slot, {
            "encoding": "json",
            "transactionDetails": "full",
            "rewards": false,
            "commitment": "finalized",
            "maxSupportedTransactionVersion": 1
        }]);
        match self.call("getBlock", params).await {
            Ok(block) => Ok(Some(block)),
            Err(Error::NoBlock { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
