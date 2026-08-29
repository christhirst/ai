use crate::config::DatabaseConfig;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::{Result as SurrealResult, Surreal};

/// Type alias for local SurrealDB instance.
pub type LocalDb = Surreal<Db>;

/// HTTP client for remote SurrealDB instances (supports SurrealDB 2.x Cloud and 3.x).
#[derive(Clone, Debug)]
pub struct HttpDbClient {
    pub endpoint: String,
    pub namespace: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub client: reqwest::Client,
}

impl HttpDbClient {
    pub fn new(config: &DatabaseConfig) -> Self {
        let base_url = config.endpoint.trim_end_matches('/').to_string();
        Self {
            endpoint: base_url,
            namespace: config.namespace.clone(),
            database: config.database.clone(),
            username: config.username.clone(),
            password: config.password.clone(),
            client: reqwest::Client::new(),
        }
    }

    /// Executes a SurrealQL query against the remote `/sql` endpoint.
    pub async fn query_raw(&self, sql: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let url = if self.endpoint.ends_with("/sql") {
            self.endpoint.clone()
        } else {
            format!("{}/sql", self.endpoint)
        };

        let full_query = format!("USE NS {} DB {}; {}", self.namespace, self.database, sql);

        let mut req = self
            .client
            .post(&url)
            .header("surreal-ns", &self.namespace)
            .header("surreal-db", &self.database)
            .header("Accept", "application/json")
            .body(full_query);

        if let (Some(u), Some(p)) = (&self.username, &self.password) {
            req = req.basic_auth(u, Some(p));
        }

        let response = req.send().await?;
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

/// Initializes an in-memory SurrealDB database instance with a given namespace and database.
pub async fn init_memory_db(namespace: &str, database: &str) -> SurrealResult<LocalDb> {
    let db = Surreal::new::<Mem>(()).await?;
    db.use_ns(namespace).use_db(database).await?;
    Ok(db)
}

/// Initializes an AppDb connection based on DatabaseConfig.
/// Uses in-memory embedded SurrealDB if endpoint starts with "mem", or HTTP client for remote endpoints.
pub async fn init_db_from_config(config: &DatabaseConfig) -> Result<AppDb, Box<dyn std::error::Error>> {
    if config.endpoint.starts_with("mem") {
        let db = init_memory_db(&config.namespace, &config.database).await?;
        Ok(AppDb::Local(db))
    } else {
        let http_client = HttpDbClient::new(config);
        Ok(AppDb::Remote(http_client))
    }
}
