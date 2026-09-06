use crate::config::DatabaseConfig;
use std::sync::Arc;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::{Result as SurrealResult, Surreal};
use tokio::sync::RwLock;

/// Type alias for local SurrealDB instance.
pub type LocalDb = Surreal<Db>;

/// Normalizes database endpoint URLs by converting WebSocket schemes (`ws://`, `wss://`)
/// to HTTP schemes (`http://`, `https://`) and stripping trailing `/rpc` or `/sql` paths.
pub fn normalize_base_url(endpoint: &str) -> String {
    let ep = endpoint.trim().trim_end_matches('/');
    let mut url_str = if let Some(stripped) = ep.strip_prefix("wss://") {
        format!("https://{stripped}")
    } else if let Some(stripped) = ep.strip_prefix("ws://") {
        format!("http://{stripped}")
    } else {
        ep.to_string()
    };

    if url_str.ends_with("/rpc") {
        url_str.truncate(url_str.len() - 4);
    } else if url_str.ends_with("/sql") {
        url_str.truncate(url_str.len() - 4);
    }
    url_str.trim_end_matches('/').to_string()
}

/// HTTP client for remote SurrealDB instances (supports SurrealDB 2.x Cloud and 3.x).
#[derive(Clone, Debug)]
pub struct HttpDbClient {
    pub endpoint: String,
    pub namespace: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub client: reqwest::Client,
    pub auth_token: Arc<RwLock<Option<String>>>,
}

impl HttpDbClient {
    pub fn new(config: &DatabaseConfig) -> Self {
        let base_url = normalize_base_url(&config.endpoint);
        Self {
            endpoint: base_url,
            namespace: config.namespace.clone(),
            database: config.database.clone(),
            username: config.username.clone(),
            password: config.password.clone(),
            client: reqwest::Client::new(),
            auth_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Performs authentication against the remote SurrealDB instance via the `/rpc` signin method.
    /// Obtains a JWT token and caches it in `self.auth_token`.
    pub async fn signin(&self) -> Result<String, Box<dyn std::error::Error>> {
        let (username, password) = match (&self.username, &self.password) {
            (Some(u), Some(p)) => (u.as_str(), p.as_str()),
            _ => return Err("Database credentials (username and password) are required for authentication".into()),
        };

        let rpc_url = format!("{}/rpc", self.endpoint);
        let payload = serde_json::json!({
            "method": "signin",
            "params": [{
                "user": username,
                "pass": password,
                "ns": self.namespace,
                "db": self.database,
            }]
        });

        let response = self
            .client
            .post(&rpc_url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| -> Box<dyn std::error::Error> {
                format!("Failed to connect to SurrealDB endpoint at {rpc_url}: {e}").into()
            })?;

        let status = response.status();
        let body: serde_json::Value = response.json().await.map_err(|e| -> Box<dyn std::error::Error> {
            format!("Invalid JSON response from SurrealDB RPC at {rpc_url}: {e}").into()
        })?;

        if !status.is_success() {
            return Err(format!("SurrealDB authentication HTTP error {status}: {body}").into());
        }

        if let Some(err) = body.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Authentication failed");
            return Err(format!("Database authentication failed: {msg}").into());
        }

        let token = body
            .get("result")
            .and_then(|r| r.as_str())
            .ok_or_else(|| -> Box<dyn std::error::Error> {
                "Failed to obtain auth token from SurrealDB signin response".into()
            })?
            .to_string();

        let mut token_lock = self.auth_token.write().await;
        *token_lock = Some(token.clone());

        Ok(token)
    }

    /// Verifies authentication and authorization against the target SurrealDB instance.
    pub async fn check_auth(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.username.is_some() && self.password.is_some() {
            self.signin().await?;
        }

        // Test database access with an introspection query
        let res = self.query_raw("INFO FOR DB").await.map_err(|e| -> Box<dyn std::error::Error> {
            format!("Database authentication check failed: {e}").into()
        })?;

        if res.is_null() {
            let _ = self.query_raw("RETURN 1;").await.map_err(|e| -> Box<dyn std::error::Error> {
                format!("Database authentication check failed: {e}").into()
            })?;
        }

        Ok(())
    }

    /// Executes a SurrealQL query against the remote `/sql` endpoint.
    pub async fn query_raw(&self, sql: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let url = format!("{}/sql", self.endpoint);
        let full_query = format!("USE NS {} DB {}; {}", self.namespace, self.database, sql);

        let make_request = |token: Option<String>| {
            let mut req = self
                .client
                .post(&url)
                .header("surreal-ns", &self.namespace)
                .header("surreal-db", &self.database)
                .header("Accept", "application/json")
                .body(full_query.clone());

            if let Some(t) = token {
                req = req.bearer_auth(t);
            } else if let (Some(u), Some(p)) = (&self.username, &self.password) {
                req = req.basic_auth(u, Some(p));
            }
            req
        };

        let current_token = self.auth_token.read().await.clone();
        let mut response = make_request(current_token).send().await?;

        // If unauthorized and we have credentials, attempt signin refresh once
        if response.status() == reqwest::StatusCode::UNAUTHORIZED && self.username.is_some() && self.password.is_some() {
            let refreshed_token = {
                let res = self.signin().await;
                res.ok()
            };
            if let Some(new_token) = refreshed_token {
                response = make_request(Some(new_token)).send().await?;
            }
        }

        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(format!("SurrealDB HTTP error {status}: {text}").into());
        }

        let parsed: serde_json::Value = serde_json::from_str(&text)?;
        if let Some(arr) = parsed.as_array() {
            for item in arr {
                if item.get("status").and_then(|s| s.as_str()) == Some("ERR") {
                    let err_msg = item.get("result").and_then(|r| r.as_str()).unwrap_or("Unknown SurrealDB error");
                    return Err(format!("SurrealDB Query Error: {err_msg}").into());
                }
            }
            if let Some(last) = arr.last() {
                return Ok(last.get("result").cloned().unwrap_or(serde_json::Value::Null));
            }
        }
        Ok(parsed)
    }
}

/// Unified database enum supporting local in-memory SurrealDB and remote HTTP SurrealDB.
#[derive(Clone)]
pub enum AppDb {
    Local(LocalDb),
    Remote(HttpDbClient),
}

impl AppDb {
    /// Checks authentication and connectivity for either local or remote SurrealDB instance.
    pub async fn check_auth(&self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            AppDb::Local(local) => {
                let mut response = local.query("INFO FOR DB").await?;
                let _: Option<serde_json::Value> = response.take(0)?;
                Ok(())
            }
            AppDb::Remote(remote) => remote.check_auth().await,
        }
    }
}

/// Initializes an in-memory SurrealDB database instance with a given namespace and database.
pub async fn init_memory_db(namespace: &str, database: &str) -> SurrealResult<LocalDb> {
    let db = Surreal::new::<Mem>(()).await?;
    db.use_ns(namespace).use_db(database).await?;
    Ok(db)
}

/// Initializes an AppDb connection based on DatabaseConfig.
/// Uses in-memory embedded SurrealDB if endpoint starts with "mem", or HTTP client for remote endpoints.
/// Performs authentication and connectivity check against the target database.
pub async fn init_db_from_config(config: &DatabaseConfig) -> Result<AppDb, Box<dyn std::error::Error>> {
    let endpoint = config.endpoint.trim();
    if endpoint.is_empty() {
        return Err("Database endpoint is not configured. Please specify 'db.endpoint' in config.toml or set SURREAL_URL (or explicitly set 'mem://' for in-memory embedded DB).".into());
    }
    let db = if endpoint.starts_with("mem") {
        let db = init_memory_db(&config.namespace, &config.database).await?;
        AppDb::Local(db)
    } else {
        let http_client = HttpDbClient::new(config);
        AppDb::Remote(http_client)
    };
    db.check_auth().await?;
    Ok(db)
}
