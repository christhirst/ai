use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::fmt;

// TODO: Refactor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionVariant {
    #[serde(rename = "normal", alias = "prompt")]
    Normal,
    #[default]
    #[serde(rename = "typed", alias = "prompt_typed")]
    Typed,
    #[serde(rename = "all", alias = "both")]
    All,
}

impl fmt::Display for ExecutionVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionVariant::Normal => write!(f, "normal"),
            ExecutionVariant::Typed => write!(f, "typed"),
            ExecutionVariant::All => write!(f, "all"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PromptConfig {
    #[serde(default = "default_prompt_query")]
    pub query: String,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            query: default_prompt_query(),
        }
    }
}

fn default_prompt_query() -> String {
    "Hello! Tell me a one-sentence joke about programming.".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PromptTypedConfig {
    #[serde(default = "default_typed_query")]
    pub query: String,
}

impl Default for PromptTypedConfig {
    fn default() -> Self {
        Self {
            query: default_typed_query(),
        }
    }
}

fn default_typed_query() -> String {
    "Give me the GDP of Germany for each year 1990 to 2025".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_db_namespace")]
    pub namespace: String,
    #[serde(default = "default_db_database")]
    pub database: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            endpoint: default_db_endpoint(),
            namespace: default_db_namespace(),
            database: default_db_database(),
            username: None,
            password: None,
        }
    }
}

fn default_db_endpoint() -> String {
    "mem://".to_string()
}

fn default_db_namespace() -> String {
    "data".to_string()
}

fn default_db_database() -> String {
    "ai".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GrpcConfig {
    #[serde(default = "default_grpc_host")]
    pub host: String,
    #[serde(default = "default_grpc_port")]
    pub port: u16,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            host: default_grpc_host(),
            port: default_grpc_port(),
        }
    }
}

fn default_grpc_host() -> String {
    "127.0.0.1".to_string()
}

fn default_grpc_port() -> u16 {
    50051
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub gemini_api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: Option<f64>,
    #[serde(default = "default_preamble")]
    pub preamble: Option<String>,
    #[serde(default)]
    pub variant: ExecutionVariant,
    #[serde(default)]
    pub prompt: PromptConfig,
    #[serde(default)]
    pub prompt_typed: PromptTypedConfig,
    #[serde(default)]
    pub db: DatabaseConfig,
    #[serde(default)]
    pub grpc: GrpcConfig,
}

fn default_model() -> String {
    "gemini-3.5-flash-lite".to_string()
}

fn default_temperature() -> Option<f64> {
    Some(0.0)
}

fn default_preamble() -> Option<String> {
    Some("You are a helpful assistant.".to_string())
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let builder = Config::builder()
            .set_default("model", default_model())?
            .set_default("temperature", 0.0)?
            .set_default("preamble", "You are a helpful assistant.")?
            .set_default("variant", "typed")?
            .set_default("prompt.query", default_prompt_query())?
            .set_default("prompt_typed.query", default_typed_query())?
            .set_default("db.endpoint", default_db_endpoint())?
            .set_default("db.namespace", default_db_namespace())?
            .set_default("db.database", default_db_database())?
            .set_default("grpc.host", default_grpc_host())?
            .set_default("grpc.port", default_grpc_port() as i64)?
            .add_source(File::with_name("config/config").required(false))
            .add_source(File::with_name("config").required(false))
            .add_source(Environment::with_prefix("APP").separator("_"))
            .add_source(Environment::default());

        let mut config: AppConfig = builder.build()?.try_deserialize()?;

        if let Ok(key) = std::env::var("GEMINI_API_KEY") {
            if !key.trim().is_empty() {
                config.gemini_api_key = key;
            }
        }

        if let Ok(pass) = std::env::var("SURREAL_PASS")
            .or_else(|_| std::env::var("SURREALDB_PASS"))
            .or_else(|_| std::env::var("DB_PASSWORD"))
        {
            if !pass.trim().is_empty() {
                config.db.password = Some(pass);
            }
        }

        if let Ok(user) = std::env::var("SURREAL_USER")
            .or_else(|_| std::env::var("SURREALDB_USER"))
            .or_else(|_| std::env::var("DB_USERNAME"))
            .or_else(|_| std::env::var("DB_USER"))
        {
            if !user.trim().is_empty() {
                config.db.username = Some(user);
            }
        }

        if let Ok(endpoint) = std::env::var("SURREAL_URL")
            .or_else(|_| std::env::var("SURREALDB_URL"))
            .or_else(|_| std::env::var("DB_ENDPOINT"))
        {
            if !endpoint.trim().is_empty() {
                config.db.endpoint = endpoint;
            }
        }

        if let Ok(ns) = std::env::var("SURREAL_NS")
            .or_else(|_| std::env::var("SURREALDB_NS"))
            .or_else(|_| std::env::var("DB_NAMESPACE"))
        {
            if !ns.trim().is_empty() {
                config.db.namespace = ns;
            }
        }

        if let Ok(db) = std::env::var("SURREAL_DB")
            .or_else(|_| std::env::var("SURREALDB_DB"))
            .or_else(|_| std::env::var("DB_DATABASE"))
        {
            if !db.trim().is_empty() {
                config.db.database = db;
            }
        }

        if let Ok(host) = std::env::var("GRPC_HOST").or_else(|_| std::env::var("APP_GRPC_HOST")) {
            if !host.trim().is_empty() {
                config.grpc.host = host;
            }
        }

        if let Ok(port_str) = std::env::var("GRPC_PORT").or_else(|_| std::env::var("APP_GRPC_PORT")) {
            if let Ok(port) = port_str.parse::<u16>() {
                config.grpc.port = port;
            }
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::FileFormat;

    #[test]
    fn test_deserialize_config_from_toml() {
        let toml_str = r#"
gemini_api_key = "test_key_123"
model = "gemini-3.5-flash-lite"
temperature = 0.5
preamble = "Custom preamble"
variant = "normal"

[prompt]
query = "Say hi"

[prompt_typed]
query = "Give GDP data"

[grpc]
host = "0.0.0.0"
port = 50052
"#;

        let c = Config::builder()
            .add_source(File::from_str(toml_str, FileFormat::Toml))
            .build()
            .unwrap();
        let config: AppConfig = c.try_deserialize().unwrap();

        assert_eq!(config.gemini_api_key, "test_key_123");
        assert_eq!(config.model, "gemini-3.5-flash-lite");
        assert_eq!(config.temperature, Some(0.5));
        assert_eq!(config.preamble.as_deref(), Some("Custom preamble"));
        assert_eq!(config.variant, ExecutionVariant::Normal);
        assert_eq!(config.prompt.query, "Say hi");
        assert_eq!(config.prompt_typed.query, "Give GDP data");
        assert_eq!(config.db.endpoint, "mem://");
        assert_eq!(config.db.namespace, "data");
        assert_eq!(config.db.database, "ai");
        assert_eq!(config.grpc.host, "0.0.0.0");
        assert_eq!(config.grpc.port, 50052);
    }

    #[test]
    fn test_deserialize_config_with_custom_db() {
        let toml_str = r#"
gemini_api_key = "test_key_123"

[db]
endpoint = "ws://localhost:8000"
namespace = "production"
database = "analytics"
username = "root"
password = "secretpassword"
"#;

        let c = Config::builder()
            .add_source(File::from_str(toml_str, FileFormat::Toml))
            .build()
            .unwrap();
        let config: AppConfig = c.try_deserialize().unwrap();

        assert_eq!(config.db.endpoint, "ws://localhost:8000");
        assert_eq!(config.db.namespace, "production");
        assert_eq!(config.db.database, "analytics");
        assert_eq!(config.db.username.as_deref(), Some("root"));
        assert_eq!(config.db.password.as_deref(), Some("secretpassword"));
        assert_eq!(config.grpc.host, "127.0.0.1");
        assert_eq!(config.grpc.port, 50051);
    }
}
