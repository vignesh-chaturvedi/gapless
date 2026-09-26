use anyhow::{Result, bail};
use reqwest::{Method, StatusCode};
use serde_json::Value;

use crate::env::Env;

/// The ways a credential can be presented to the account API.
#[derive(Clone, Copy, Debug)]
pub enum Auth {
    QueryApiKey,
    BearerApiKey,
    HeaderApiKey,
    BearerSession,
}

impl Auth {
    pub const ALL: [Auth; 4] = [
        Auth::QueryApiKey,
        Auth::BearerApiKey,
        Auth::HeaderApiKey,
        Auth::BearerSession,
    ];

    fn label(self) -> &'static str {
        match self {
            Auth::QueryApiKey => "?api_key=<key>",
            Auth::BearerApiKey => "Authorization: Bearer <key>",
            Auth::HeaderApiKey => "x-api-key: <key>",
            Auth::BearerSession => "Authorization: Bearer <session JWT>",
        }
    }
}

pub struct Api {
    http: reqwest::Client,
    base: String,
    api_key: String,
    session: Option<String>,
}

impl Api {
    pub fn new(env: &Env) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: env.api_url.trim_end_matches('/').to_owned(),
            api_key: env.api_key.clone(),
            session: env.session_token.clone(),
        }
    }

    pub async fn request(
        &self,
        method: Method,
        path: &str,
        auth: Auth,
    ) -> Result<Option<(StatusCode, String)>> {
        let mut url = format!("{}{}", self.base, path);
        let mut req;
        match auth {
            Auth::QueryApiKey => {
                url = format!("{url}?api_key={}", self.api_key);
                req = self.http.request(method, &url);
            }
            Auth::BearerApiKey => {
                req = self.http.request(method, &url).bearer_auth(&self.api_key);
            }
            Auth::HeaderApiKey => {
                req = self
                    .http
                    .request(method, &url)
                    .header("x-api-key", &self.api_key);
            }
            Auth::BearerSession => {
                let Some(token) = &self.session else {
                    return Ok(None);
                };
                req = self.http.request(method, &url).bearer_auth(token);
            }
        }
        req = req.header("accept", "application/json");
        let resp = req.send().await.map_err(|e| e.without_url())?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Ok(Some((status, body)))
    }

    /// The first auth style the account API accepts for listing gRPC connections.
    pub async fn find_auth(&self) -> Result<Auth> {
        for auth in Auth::ALL {
            if let Some((status, _)) = self
                .request(Method::GET, "/auth/connections/grpc", auth)
                .await?
                && status.is_success()
            {
                return Ok(auth);
            }
        }
        bail!("the account API rejected every auth style; set SOLAMI_SESSION_TOKEN")
    }

    pub async fn list_grpc(&self, auth: Auth) -> Result<Vec<Value>> {
        match self
            .request(Method::GET, "/auth/connections/grpc", auth)
            .await?
        {
            Some((status, body)) if status.is_success() => {
                Ok(serde_json::from_str::<Value>(&body)?
                    .as_array()
                    .cloned()
                    .unwrap_or_default())
            }
            Some((status, body)) => bail!("list connections: HTTP {status}: {}", clip(&body, 200)),
            None => bail!("no credential for {auth:?}"),
        }
    }

    pub async fn kill(&self, auth: Auth, conn_id: &str) -> Result<(StatusCode, String)> {
        let path = format!("/auth/connections/grpc/{conn_id}");
        self.request(Method::DELETE, &path, auth)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no credential for {auth:?}"))
    }
}

/// Spike 3: which auth style do the account routes take, and what do they return?
pub async fn connections(env: &Env) -> Result<()> {
    let api = Api::new(env);
    let paths = [
        "/auth/connections/grpc",
        "/auth/connections",
        "/auth/connections/grpc/history",
        "/auth/grpc/usage",
        "/auth/grpc-payg",
    ];
    for path in paths {
        println!("\n== GET {path}");
        for auth in Auth::ALL {
            match api.request(Method::GET, path, auth).await? {
                None => println!("  {:<38} (skipped: no SOLAMI_SESSION_TOKEN)", auth.label()),
                Some((status, body)) => {
                    println!(
                        "  {:<38} HTTP {}  {}",
                        auth.label(),
                        status.as_u16(),
                        clip(&body, 400)
                    );
                }
            }
        }
    }
    Ok(())
}

pub fn clip(s: &str, n: usize) -> String {
    let one_line = s.replace('\n', " ");
    if one_line.chars().count() <= n {
        one_line
    } else {
        format!("{}…", one_line.chars().take(n).collect::<String>())
    }
}
