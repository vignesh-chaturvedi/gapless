use anyhow::{Context, Result};

pub const PUMP_FUN: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Settings read from the environment (and `.env`, if present).
pub struct Env {
    pub api_key: String,
    pub session_token: Option<String>,
    pub grpc_url: String,
    pub rpc_url: String,
    pub api_url: String,
    pub program: String,
}

impl Env {
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();
        let api_key = var("SOLAMI_API_KEY")
            .context("SOLAMI_API_KEY is not set. Copy .env.example to .env and paste your key.")?;
        Ok(Self {
            api_key,
            session_token: var("SOLAMI_SESSION_TOKEN"),
            grpc_url: var("SOLAMI_GRPC_URL").unwrap_or_else(|| "https://grpc.solami.dev".into()),
            rpc_url: var("SOLAMI_RPC_URL").unwrap_or_else(|| "https://rpc.solami.dev/sol".into()),
            api_url: var("SOLAMI_API_URL").unwrap_or_else(|| "https://api.solami.dev".into()),
            program: var("GAPLESS_PROGRAM").unwrap_or_else(|| PUMP_FUN.into()),
        })
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}
