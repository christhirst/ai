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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelProvider {
    #[default]
    Gemini,
    Qwen,
}

impl fmt::Display for ModelProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelProvider::Gemini => write!(f, "gemini"),
            ModelProvider::Qwen => write!(f, "qwen"),
        }
    }
}

impl std::str::FromStr for ModelProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "gemini" | "google" => Ok(ModelProvider::Gemini),
            "qwen" | "dashscope" | "aliyun" | "alibaba" => Ok(ModelProvider::Qwen),
            other => Err(format!(
                "Unsupported provider '{other}': only 'gemini' and 'qwen' are supported."
            )),
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
    "".to_string()
}

fn default_db_namespace() -> String {
    "data".to_string()
}

fn default_db_database() -> String {
    "ai".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct GrpcOauthConfig {
    #[serde(default)]
    pub well_known_url: Option<String>,
    #[serde(default)]
    pub token_url: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub jwks_url: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub jwt_secret: Option<String>,
    #[serde(default)]
    pub jwt_public_key: Option<String>,
    #[serde(default)]
    pub static_tokens: Vec<String>,
    #[serde(default)]
    pub check_on_startup: Option<bool>,
}

impl GrpcOauthConfig {
    pub fn is_configured(&self) -> bool {
        self.well_known_url
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
            || self
                .jwks_url
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            || self
                .jwt_secret
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            || self
                .jwt_public_key
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            || self
                .client_id
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            || self
                .client_secret
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            || !self.static_tokens.is_empty()
    }
}

pub fn default_admin_user() -> String {
    "admin".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct GrpcAuthConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default = "default_admin_user")]
    pub admin_user: String,
    #[serde(default)]
    pub admin_password: Option<String>,
    #[serde(default)]
    pub oauth: GrpcOauthConfig,
}

impl GrpcAuthConfig {
    /// Returns true if authentication is explicitly enabled, or if unset and either
    /// admin_password or oauth is configured.
    pub fn is_active(&self) -> bool {
        match self.enabled {
            Some(explicit) => explicit,
            None => {
                let has_admin_pw = self
                    .admin_password
                    .as_deref()
                    .map(|p| !p.trim().is_empty())
                    .unwrap_or(false);
                has_admin_pw || self.oauth.is_configured()
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GrpcConfig {
    #[serde(default = "default_grpc_host")]
    pub host: String,
    #[serde(default = "default_grpc_port")]
    pub port: u16,
    #[serde(default)]
    pub auth: GrpcAuthConfig,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            host: default_grpc_host(),
            port: default_grpc_port(),
            auth: GrpcAuthConfig::default(),
        }
    }
}

fn default_grpc_host() -> String {
    "127.0.0.1".to_string()
}

fn default_grpc_port() -> u16 {
    50051
}

pub use crate::vault::{
    VaultConfig, VaultKeysConfig, default_vault_address, default_vault_auto_renew,
    default_vault_enabled, default_vault_kv_version, default_vault_mount, default_vault_path,
};

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct QwenConfig {
    #[serde(default, alias = "url")]
    pub base_url: Option<String>,
    #[serde(default, alias = "key")]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub provider: ModelProvider,
    #[serde(default)]
    pub gemini_api_key: String,
    #[serde(default, alias = "dashscope_api_key")]
    pub qwen_api_key: String,
    #[serde(
        default = "default_qwen_base_url",
        alias = "qwen_url",
        alias = "dashscope_base_url",
        alias = "dashscope_url"
    )]
    pub qwen_base_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_qwen_model", alias = "dashscope_model")]
    pub qwen_model: String,
    #[serde(default)]
    pub qwen: Option<QwenConfig>,
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
    #[serde(default)]
    pub vault: VaultConfig,
}

pub fn default_qwen_base_url() -> String {
    "https://dashscope-intl.aliyuncs.com/compatible-mode/v1".to_string()
}

pub fn default_qwen_model() -> String {
    "qwen-plus".to_string()
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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: ModelProvider::default(),
            gemini_api_key: "".to_string(),
            qwen_api_key: "".to_string(),
            qwen_base_url: default_qwen_base_url(),
            model: default_model(),
            qwen_model: default_qwen_model(),
            qwen: None,
            temperature: default_temperature(),
            preamble: default_preamble(),
            variant: ExecutionVariant::default(),
            prompt: PromptConfig::default(),
            prompt_typed: PromptTypedConfig::default(),
            db: DatabaseConfig::default(),
            grpc: GrpcConfig::default(),
            vault: VaultConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load_from_config() -> Result<Self, ConfigError> {
        let builder = Config::builder()
            .set_default("provider", "gemini")?
            .set_default("gemini_api_key", "")?
            .set_default("qwen_api_key", "")?
            .set_default("qwen_base_url", default_qwen_base_url())?
            .set_default("model", default_model())?
            .set_default("qwen_model", default_qwen_model())?
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
            .set_default("vault.enabled", default_vault_enabled())?
            .set_default("vault.address", default_vault_address())?
            .set_default("vault.mount", default_vault_mount())?
            .set_default("vault.path", default_vault_path())?
            .set_default("vault.kv_version", default_vault_kv_version() as i64)?
            .set_default("vault.auto_renew", default_vault_auto_renew())?
            .add_source(File::with_name("config/config").required(false))
            .add_source(File::with_name("config").required(false))
            .add_source(Environment::with_prefix("APP").separator("_"))
            .add_source(Environment::default());

        let mut config: AppConfig = builder.build()?.try_deserialize()?;

        // If [qwen] table was configured in TOML, apply non-empty values
        if let Some(ref qwen) = config.qwen {
            if let Some(ref url) = qwen.base_url
                && !url.trim().is_empty()
            {
                config.qwen_base_url = url.trim().to_string();
            }
            if let Some(ref key) = qwen.api_key
                && !key.trim().is_empty()
            {
                config.qwen_api_key = key.trim().to_string();
            }
            if let Some(ref qm) = qwen.model
                && !qm.trim().is_empty()
            {
                config.qwen_model = qm.trim().to_string();
            }
        }

        if let Ok(p) = std::env::var("PROVIDER")
            .or_else(|_| std::env::var("APP_PROVIDER"))
            .or_else(|_| std::env::var("AI_PROVIDER"))
            && let Ok(provider) = p.parse::<ModelProvider>()
        {
            config.provider = provider;
        }

        if let Ok(key) = std::env::var("GEMINI_API_KEY")
            && !key.trim().is_empty()
        {
            config.gemini_api_key = key;
        }

        if let Ok(key) = std::env::var("QWEN_API_KEY")
            .or_else(|_| std::env::var("DASHSCOPE_API_KEY"))
            .or_else(|_| std::env::var("APP_QWEN_API_KEY"))
            .or_else(|_| std::env::var("APP_DASHSCOPE_API_KEY"))
            && !key.trim().is_empty()
        {
            config.qwen_api_key = key.trim().to_string();
        }

        if let Ok(base_url) = std::env::var("QWEN_BASE_URL")
            .or_else(|_| std::env::var("DASHSCOPE_BASE_URL"))
            .or_else(|_| std::env::var("APP_QWEN_BASE_URL"))
            .or_else(|_| std::env::var("QWEN_URL"))
            .or_else(|_| std::env::var("DASHSCOPE_URL"))
            && !base_url.trim().is_empty()
        {
            config.qwen_base_url = base_url.trim().to_string();
        }

        // Normalize qwen_base_url by stripping any trailing slash
        config.qwen_base_url = config
            .qwen_base_url
            .trim()
            .trim_end_matches('/')
            .to_string();

        if let Ok(qm) = std::env::var("QWEN_MODEL").or_else(|_| std::env::var("APP_QWEN_MODEL"))
            && !qm.trim().is_empty()
        {
            config.qwen_model = qm.trim().to_string();
        }

        if let Ok(pass) = std::env::var("SURREAL_PASS")
            .or_else(|_| std::env::var("SURREALDB_PASS"))
            .or_else(|_| std::env::var("APP_SURREALDB_PASS"))
            .or_else(|_| std::env::var("APP_SURREAL_PASS"))
            .or_else(|_| std::env::var("DB_PASSWORD"))
            .or_else(|_| std::env::var("APP_DB_PASSWORD"))
            && !pass.trim().is_empty()
        {
            config.db.password = Some(pass);
        }

        if let Ok(user) = std::env::var("SURREAL_USER")
            .or_else(|_| std::env::var("SURREALDB_USER"))
            .or_else(|_| std::env::var("APP_SURREALDB_USER"))
            .or_else(|_| std::env::var("APP_SURREAL_USER"))
            .or_else(|_| std::env::var("DB_USERNAME"))
            .or_else(|_| std::env::var("DB_USER"))
            .or_else(|_| std::env::var("APP_DB_USER"))
            && !user.trim().is_empty()
        {
            config.db.username = Some(user);
        }

        if let Ok(endpoint) = std::env::var("SURREAL_URL")
            .or_else(|_| std::env::var("SURREALDB_URL"))
            .or_else(|_| std::env::var("DB_ENDPOINT"))
            .or_else(|_| std::env::var("APP_SURREAL_URL"))
            .or_else(|_| std::env::var("APP_SURREALDB_URL"))
            .or_else(|_| std::env::var("APP_DB_ENDPOINT"))
            && !endpoint.trim().is_empty()
        {
            config.db.endpoint = endpoint;
        }

        if let Ok(ns) = std::env::var("SURREAL_NS")
            .or_else(|_| std::env::var("SURREALDB_NS"))
            .or_else(|_| std::env::var("APP_SURREALDB_NS"))
            .or_else(|_| std::env::var("APP_SURREAL_NS"))
            .or_else(|_| std::env::var("DB_NAMESPACE"))
            .or_else(|_| std::env::var("APP_DB_NAMESPACE"))
            && !ns.trim().is_empty()
        {
            config.db.namespace = ns;
        }

        if let Ok(db) = std::env::var("SURREAL_DB")
            .or_else(|_| std::env::var("SURREALDB_DB"))
            .or_else(|_| std::env::var("APP_SURREALDB_DB"))
            .or_else(|_| std::env::var("APP_SURREAL_DB"))
            .or_else(|_| std::env::var("DB_DATABASE"))
            .or_else(|_| std::env::var("APP_DB_DATABASE"))
            && !db.trim().is_empty()
        {
            config.db.database = db;
        }

        if let Ok(host) = std::env::var("GRPC_HOST").or_else(|_| std::env::var("APP_GRPC_HOST"))
            && !host.trim().is_empty()
        {
            config.grpc.host = host;
        }

        if let Ok(port_str) = std::env::var("GRPC_PORT").or_else(|_| std::env::var("APP_GRPC_PORT"))
            && let Ok(port) = port_str.parse::<u16>()
        {
            config.grpc.port = port;
        }

        if let Ok(admin_pw) = std::env::var("ADMIN_PASSWORD")
            .or_else(|_| std::env::var("GRPC_ADMIN_PASSWORD"))
            .or_else(|_| std::env::var("APP_ADMIN_PASSWORD"))
            .or_else(|_| std::env::var("APP_GRPC_ADMIN_PASSWORD"))
            .or_else(|_| std::env::var("APP_GRPC_AUTH_ADMIN_PASSWORD"))
            && !admin_pw.trim().is_empty()
        {
            config.grpc.auth.admin_password = Some(admin_pw.trim().to_string());
        }

        if let Ok(admin_user) = std::env::var("ADMIN_USER")
            .or_else(|_| std::env::var("GRPC_ADMIN_USER"))
            .or_else(|_| std::env::var("APP_ADMIN_USER"))
            .or_else(|_| std::env::var("APP_GRPC_ADMIN_USER"))
            && !admin_user.trim().is_empty()
        {
            config.grpc.auth.admin_user = admin_user.trim().to_string();
        }

        if let Ok(auth_enabled) =
            std::env::var("GRPC_AUTH_ENABLED").or_else(|_| std::env::var("APP_GRPC_AUTH_ENABLED"))
        {
            let trimmed = auth_enabled.trim();
            config.grpc.auth.enabled = Some(trimmed.eq_ignore_ascii_case("true") || trimmed == "1");
        }

        if let Ok(jwks) = std::env::var("OAUTH_JWKS_URL")
            .or_else(|_| std::env::var("GRPC_OAUTH_JWKS_URL"))
            .or_else(|_| std::env::var("APP_OAUTH_JWKS_URL"))
            && !jwks.trim().is_empty()
        {
            config.grpc.auth.oauth.jwks_url = Some(jwks.trim().to_string());
        }

        if let Ok(iss) = std::env::var("OAUTH_ISSUER")
            .or_else(|_| std::env::var("GRPC_OAUTH_ISSUER"))
            .or_else(|_| std::env::var("APP_OAUTH_ISSUER"))
            && !iss.trim().is_empty()
        {
            config.grpc.auth.oauth.issuer = Some(iss.trim().to_string());
        }

        if let Ok(aud) = std::env::var("OAUTH_AUDIENCE")
            .or_else(|_| std::env::var("GRPC_OAUTH_AUDIENCE"))
            .or_else(|_| std::env::var("APP_OAUTH_AUDIENCE"))
            && !aud.trim().is_empty()
        {
            config.grpc.auth.oauth.audience = Some(aud.trim().to_string());
        }

        if let Ok(secret) = std::env::var("OAUTH_JWT_SECRET")
            .or_else(|_| std::env::var("GRPC_OAUTH_JWT_SECRET"))
            .or_else(|_| std::env::var("APP_OAUTH_JWT_SECRET"))
            .or_else(|_| std::env::var("AI_GRPC_OAUTH_SECRET"))
            .or_else(|_| std::env::var("GRPC_OAUTH_SECRET"))
            && !secret.trim().is_empty()
        {
            config.grpc.auth.oauth.jwt_secret = Some(secret.trim().to_string());
        }

        if let Ok(pub_key) = std::env::var("OAUTH_JWT_PUBLIC_KEY")
            .or_else(|_| std::env::var("GRPC_OAUTH_JWT_PUBLIC_KEY"))
            .or_else(|_| std::env::var("APP_OAUTH_JWT_PUBLIC_KEY"))
            && !pub_key.trim().is_empty()
        {
            config.grpc.auth.oauth.jwt_public_key = Some(pub_key.trim().to_string());
        }

        if let Ok(enabled_str) = std::env::var("VAULT_ENABLED")
            .or_else(|_| std::env::var("OPENBAO_ENABLED"))
            .or_else(|_| std::env::var("APP_VAULT_ENABLED"))
        {
            let trimmed = enabled_str.trim();
            config.vault.enabled = trimmed.eq_ignore_ascii_case("true") || trimmed == "1";
        }

        if let Ok(addr) = std::env::var("VAULT_ADDR")
            .or_else(|_| std::env::var("OPENBAO_ADDR"))
            .or_else(|_| std::env::var("APP_VAULT_ADDR"))
            .or_else(|_| std::env::var("APP_VAULT_ADDRESS"))
            && !addr.trim().is_empty()
        {
            config.vault.address = addr.trim().to_string();
        }

        if let Ok(token) = std::env::var("VAULT_TOKEN")
            .or_else(|_| std::env::var("OPENBAO_TOKEN"))
            .or_else(|_| std::env::var("APP_VAULT_TOKEN"))
            && !token.trim().is_empty()
        {
            config.vault.token = Some(token.trim().to_string());
        }

        if let Ok(mount) = std::env::var("VAULT_MOUNT")
            .or_else(|_| std::env::var("OPENBAO_MOUNT"))
            .or_else(|_| std::env::var("APP_VAULT_MOUNT"))
            && !mount.trim().is_empty()
        {
            config.vault.mount = mount.trim().to_string();
        }

        if let Ok(path) = std::env::var("VAULT_PATH")
            .or_else(|_| std::env::var("OPENBAO_PATH"))
            .or_else(|_| std::env::var("APP_VAULT_PATH"))
            && !path.trim().is_empty()
        {
            config.vault.path = path.trim().to_string();
        }

        if let Ok(ns) = std::env::var("VAULT_NAMESPACE")
            .or_else(|_| std::env::var("OPENBAO_NAMESPACE"))
            .or_else(|_| std::env::var("APP_VAULT_NAMESPACE"))
            && !ns.trim().is_empty()
        {
            config.vault.namespace = Some(ns.trim().to_string());
        }

        if let Ok(ver_str) = std::env::var("VAULT_KV_VERSION")
            .or_else(|_| std::env::var("OPENBAO_KV_VERSION"))
            .or_else(|_| std::env::var("APP_VAULT_KV_VERSION"))
            && let Ok(ver) = ver_str.trim().parse::<u32>()
        {
            config.vault.kv_version = ver;
        }

        if let Ok(auto_renew_str) = std::env::var("VAULT_AUTO_RENEW")
            .or_else(|_| std::env::var("OPENBAO_AUTO_RENEW"))
            .or_else(|_| std::env::var("APP_VAULT_AUTO_RENEW"))
        {
            let trimmed = auto_renew_str.trim();
            config.vault.auto_renew = trimmed.eq_ignore_ascii_case("true") || trimmed == "1";
        }

        if let Ok(inc) = std::env::var("VAULT_RENEW_INCREMENT")
            .or_else(|_| std::env::var("OPENBAO_RENEW_INCREMENT"))
            .or_else(|_| std::env::var("APP_VAULT_RENEW_INCREMENT"))
            && !inc.trim().is_empty()
        {
            config.vault.renew_increment = Some(inc.trim().to_string());
        }

        Ok(config)
    }

    /// Overwrites configuration values from a JSON map containing OpenBao/Vault secret fields.
    /// Returns the number of recognized fields overwritten.
    pub fn apply_secret_data(
        &mut self,
        secret_map: &serde_json::Map<String, serde_json::Value>,
    ) -> usize {
        let mut count = 0;

        let extract_str = |keys: &[String]| -> Option<String> {
            for key in keys {
                if let Some(val) = secret_map.get(key)
                    && let Some(s) = val.as_str()
                    && !s.is_empty()
                {
                    return Some(s.to_string());
                }
            }
            None
        };

        if let Some(key) = extract_str(&self.vault.keys.gemini_api_key) {
            self.gemini_api_key = key;
            count += 1;
        }

        if let Some(key) = extract_str(&self.vault.keys.qwen_api_key) {
            self.qwen_api_key = key;
            count += 1;
        }

        if let Some(url) = extract_str(&self.vault.keys.qwen_base_url) {
            self.qwen_base_url = url.trim().trim_end_matches('/').to_string();
            count += 1;
        }

        if let Some(pass) = extract_str(&self.vault.keys.ai_db_password) {
            self.db.password = Some(pass);
            count += 1;
        }

        if let Some(user) = extract_str(&self.vault.keys.ai_db_username) {
            self.db.username = Some(user);
            count += 1;
        }

        if let Some(endpoint) = extract_str(&self.vault.keys.ai_db_endpoint) {
            self.db.endpoint = endpoint;
            count += 1;
        }

        if let Some(ns) = extract_str(&self.vault.keys.ai_db_namespace) {
            self.db.namespace = ns;
            count += 1;
        }

        if let Some(database) = extract_str(&self.vault.keys.ai_db_database) {
            self.db.database = database;
            count += 1;
        }

        if let Some(db_val) = secret_map.get("db").and_then(|v| v.as_object()) {
            let extract_nested = |keys: &[String]| -> Option<String> {
                for key in keys {
                    let bare_key = key.strip_prefix("db_").unwrap_or(key.as_str());
                    if let Some(val) = db_val.get(key).or_else(|| db_val.get(bare_key))
                        && let Some(s) = val.as_str()
                        && !s.is_empty()
                    {
                        return Some(s.to_string());
                    }
                }
                None
            };

            if self.db.password.is_none()
                && let Some(pass) = extract_nested(&self.vault.keys.ai_db_password)
            {
                self.db.password = Some(pass);
                count += 1;
            }
            if self.db.username.is_none()
                && let Some(user) = extract_nested(&self.vault.keys.ai_db_username)
            {
                self.db.username = Some(user);
                count += 1;
            }
            if let Some(endpoint) = extract_nested(&self.vault.keys.ai_db_endpoint) {
                self.db.endpoint = endpoint;
                count += 1;
            }
            if let Some(ns) = extract_nested(&self.vault.keys.ai_db_namespace) {
                self.db.namespace = ns;
                count += 1;
            }
            if let Some(database) = extract_nested(&self.vault.keys.ai_db_database) {
                self.db.database = database;
                count += 1;
            }
        }

        if let Some(admin_pw) = extract_str(&self.vault.keys.ai_grpc_admin_password) {
            self.grpc.auth.admin_password = Some(admin_pw);
            count += 1;
        }

        if let Some(client_id) = extract_str(&self.vault.keys.ai_grpc_oauth_client_id) {
            self.grpc.auth.oauth.client_id = Some(client_id);
            count += 1;
        }

        if let Some(oauth_secret) = extract_str(&self.vault.keys.ai_grpc_oauth_secret) {
            self.grpc.auth.oauth.client_secret = Some(oauth_secret.clone());
            if self.grpc.auth.oauth.jwt_secret.is_none()
                && self.grpc.auth.oauth.well_known_url.is_none()
                && self.grpc.auth.oauth.jwks_url.is_none()
            {
                self.grpc.auth.oauth.jwt_secret = Some(oauth_secret);
            }
            count += 1;
        }

        if let Some(grpc_val) = secret_map.get("grpc").and_then(|v| v.as_object()) {
            let auth_val = grpc_val
                .get("auth")
                .and_then(|v| v.as_object())
                .unwrap_or(grpc_val);

            if self.grpc.auth.admin_password.is_none() {
                for key in &self.vault.keys.ai_grpc_admin_password {
                    let bare_key = key.strip_prefix("grpc_").unwrap_or(key.as_str());
                    if let Some(p) = auth_val
                        .get(key)
                        .or_else(|| auth_val.get(bare_key))
                        .and_then(|v| v.as_str())
                        && !p.is_empty()
                    {
                        self.grpc.auth.admin_password = Some(p.to_string());
                        count += 1;
                        break;
                    }
                }
            }

            if self.grpc.auth.oauth.client_id.is_none() {
                for key in &self.vault.keys.ai_grpc_oauth_client_id {
                    let bare_key = key
                        .strip_prefix("grpc_")
                        .or_else(|| key.strip_prefix("ai_grpc_"))
                        .or_else(|| key.strip_prefix("oauth_"))
                        .unwrap_or(key.as_str());
                    if let Some(s) = auth_val
                        .get(key)
                        .or_else(|| auth_val.get(bare_key))
                        .and_then(|v| v.as_str())
                        && !s.is_empty()
                    {
                        self.grpc.auth.oauth.client_id = Some(s.to_string());
                        count += 1;
                        break;
                    }
                }
            }

            if self.grpc.auth.oauth.client_secret.is_none() {
                for key in &self.vault.keys.ai_grpc_oauth_secret {
                    let bare_key = key
                        .strip_prefix("grpc_")
                        .or_else(|| key.strip_prefix("ai_grpc_"))
                        .or_else(|| key.strip_prefix("oauth_"))
                        .unwrap_or(key.as_str());
                    if let Some(s) = auth_val
                        .get(key)
                        .or_else(|| auth_val.get(bare_key))
                        .and_then(|v| v.as_str())
                        && !s.is_empty()
                    {
                        self.grpc.auth.oauth.client_secret = Some(s.to_string());
                        if self.grpc.auth.oauth.jwt_secret.is_none()
                            && self.grpc.auth.oauth.well_known_url.is_none()
                            && self.grpc.auth.oauth.jwks_url.is_none()
                        {
                            self.grpc.auth.oauth.jwt_secret = Some(s.to_string());
                        }
                        count += 1;
                        break;
                    }
                }
            }
        }

        if let Some(qwen_val) = secret_map
            .get("qwen")
            .or_else(|| secret_map.get("dashscope"))
            .and_then(|v| v.as_object())
        {
            let extract_nested = |keys: &[String]| -> Option<String> {
                for key in keys {
                    let bare_key = key
                        .strip_prefix("qwen_")
                        .or_else(|| key.strip_prefix("dashscope_"))
                        .unwrap_or(key.as_str());
                    if let Some(val) = qwen_val
                        .get(key)
                        .or_else(|| qwen_val.get(bare_key))
                        .or_else(|| bare_key.strip_prefix("api_").and_then(|k| qwen_val.get(k)))
                        .or_else(|| bare_key.strip_prefix("base_").and_then(|k| qwen_val.get(k)))
                        && let Some(s) = val.as_str()
                        && !s.is_empty()
                    {
                        return Some(s.to_string());
                    }
                }
                None
            };

            if self.qwen_api_key.is_empty()
                && let Some(key) = extract_nested(&self.vault.keys.qwen_api_key)
            {
                self.qwen_api_key = key;
                count += 1;
            }
            if let Some(url) = extract_nested(&self.vault.keys.qwen_base_url) {
                self.qwen_base_url = url.trim().trim_end_matches('/').to_string();
                count += 1;
            }
        }

        count
    }

    /// When `vault.enabled` is true, connects to OpenBao/Vault using `vaultrs` and overwrites
    /// present secrets in `AppConfig`. If connection fails or no secret entries are found,
    /// returns an error so the server/application will not start.
    pub async fn apply_vault_secrets(&mut self) -> Result<(), ConfigError> {
        if !self.vault.enabled {
            return Ok(());
        }

        let secret_map = crate::vault::fetch_secrets_from_config(&self.vault)
            .await
            .map_err(|e| ConfigError::Message(e.to_string()))?;

        let overwritten = self.apply_secret_data(&secret_map);
        if overwritten == 0 {
            return Err(ConfigError::Message(format!(
                "OpenBao/Vault is enabled, but no recognized secret entries (e.g. gemini_api_key, qwen_api_key, db_password, etc.) were found at '{}/{}'",
                self.vault.mount, self.vault.path
            )));
        }

        tracing::info!(
            "Successfully overwritten {} secret(s) from OpenBao/Vault ({}/{})",
            overwritten,
            self.vault.mount,
            self.vault.path
        );

        Ok(())
    }

    /// Loads application configuration from config-rs and, if Vault is enabled,
    /// overwrites secrets from OpenBao/Vault using vaultrs.
    pub async fn load() -> Result<Self, ConfigError> {
        let mut config = Self::load_from_config()?;
        config.apply_vault_secrets().await?;
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
        assert_eq!(config.db.endpoint, "");
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

    #[test]
    fn test_deserialize_vault_config_from_toml() {
        let toml_str = r#"
gemini_api_key = "test_key_123"

[vault]
enabled = true
address = "http://openbao.internal:8200"
token = "test_token_root"
mount = "ai_secrets"
path = "service_config"
namespace = "corp_ns"
kv_version = 2
"#;

        let c = Config::builder()
            .add_source(File::from_str(toml_str, FileFormat::Toml))
            .build()
            .unwrap();
        let config: AppConfig = c.try_deserialize().unwrap();

        assert!(config.vault.enabled);
        assert_eq!(config.vault.address, "http://openbao.internal:8200");
        assert_eq!(config.vault.token.as_deref(), Some("test_token_root"));
        assert_eq!(config.vault.mount, "ai_secrets");
        assert_eq!(config.vault.path, "service_config");
        assert_eq!(config.vault.namespace.as_deref(), Some("corp_ns"));
        assert_eq!(config.vault.kv_version, 2);
    }

    #[test]
    fn test_deserialize_config_without_gemini_api_key() {
        let toml_str = r#"
[vault]
enabled = true
address = "http://127.0.0.1:8200"
"#;
        let c = Config::builder()
            .add_source(File::from_str(toml_str, FileFormat::Toml))
            .build()
            .unwrap();
        let config: AppConfig = c.try_deserialize().unwrap();
        assert_eq!(config.gemini_api_key, "");
        assert!(config.vault.enabled);
    }

    #[test]
    fn test_apply_secret_data_flat_and_selective_overwrite() {
        let mut config = AppConfig {
            gemini_api_key: "original_gemini_key".to_string(),
            model: "gemini-3.5-flash-lite".to_string(),
            db: DatabaseConfig {
                endpoint: "ws://localhost:8000".to_string(),
                namespace: "original_ns".to_string(),
                database: "original_db".to_string(),
                username: Some("orig_user".to_string()),
                password: Some("orig_pass".to_string()),
            },
            ..Default::default()
        };

        let secret_json: serde_json::Value = serde_json::json!({
            "gemini_api_key": "overwritten_gemini_key",
            "db_password": "overwritten_password"
            // Note: db_username, db_endpoint, namespace, database are absent!
        });

        let map = secret_json.as_object().unwrap();
        let overwritten = config.apply_secret_data(map);

        assert_eq!(overwritten, 2);
        // Overwritten fields
        assert_eq!(config.gemini_api_key, "overwritten_gemini_key");
        assert_eq!(config.db.password.as_deref(), Some("overwritten_password"));
        // Preserved fields (not overwritten because absent from OpenBao)
        assert_eq!(config.db.username.as_deref(), Some("orig_user"));
        assert_eq!(config.db.endpoint, "ws://localhost:8000");
        assert_eq!(config.db.namespace, "original_ns");
        assert_eq!(config.db.database, "original_db");
    }

    #[test]
    fn test_apply_secret_data_nested_db() {
        let mut config = AppConfig {
            gemini_api_key: "original_key".to_string(),
            model: "gemini-3.5-flash-lite".to_string(),
            ..Default::default()
        };

        let secret_json: serde_json::Value = serde_json::json!({
            "api_key": "vault_api_key_123",
            "db": {
                "password": "vault_db_password_nested",
                "username": "vault_db_user_nested",
                "endpoint": "wss://vault-db.ux-ti.com/rpc"
            }
        });

        let map = secret_json.as_object().unwrap();
        let overwritten = config.apply_secret_data(map);

        assert_eq!(overwritten, 4);
        assert_eq!(config.gemini_api_key, "vault_api_key_123");
        assert_eq!(
            config.db.password.as_deref(),
            Some("vault_db_password_nested")
        );
        assert_eq!(config.db.username.as_deref(), Some("vault_db_user_nested"));
        assert_eq!(config.db.endpoint, "wss://vault-db.ux-ti.com/rpc");
    }

    #[test]
    fn test_apply_secret_data_returns_zero_when_no_known_secrets() {
        let mut config = AppConfig {
            gemini_api_key: "orig_key".to_string(),
            model: "gemini-3.5-flash-lite".to_string(),
            ..Default::default()
        };

        let secret_json: serde_json::Value = serde_json::json!({
            "unrelated_field": "some_value",
            "another_key": 12345
        });

        let map = secret_json.as_object().unwrap();
        let overwritten = config.apply_secret_data(map);

        assert_eq!(overwritten, 0);
        assert_eq!(config.gemini_api_key, "orig_key");
    }

    #[tokio::test]
    async fn test_apply_vault_secrets_fails_when_token_missing() {
        let mut config = AppConfig {
            gemini_api_key: "orig_key".to_string(),
            vault: VaultConfig {
                enabled: true,
                address: "http://127.0.0.1:8200".to_string(),
                token: None, // No token!
                mount: "secret".to_string(),
                path: "ai".to_string(),
                namespace: None,
                kv_version: 2,
                ..Default::default()
            },
            ..Default::default()
        };

        let res = config.apply_vault_secrets().await;
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("no authentication token was provided"));
    }

    #[tokio::test]
    async fn test_apply_vault_secrets_fails_when_unreachable() {
        let mut config = AppConfig {
            gemini_api_key: "orig_key".to_string(),
            vault: VaultConfig {
                enabled: true,
                address: "http://127.0.0.1:59999".to_string(), // unreachable port
                token: Some("dummy_token".to_string()),
                mount: "secret".to_string(),
                path: "ai".to_string(),
                namespace: None,
                kv_version: 2,
                ..Default::default()
            },
            ..Default::default()
        };

        let res = config.apply_vault_secrets().await;
        assert!(res.is_err(), "Expected connection to fail when unreachable");
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to read secret from OpenBao/Vault"));
    }

    #[test]
    fn test_deserialize_grpc_auth_config() {
        let toml_str = r#"
[grpc]
host = "0.0.0.0"
port = 50051

[grpc.auth]
enabled = true
admin_user = "superuser"
admin_password = "secret_admin_pass"

[grpc.auth.oauth]
jwks_url = "https://auth.example.com/.well-known/jwks.json"
issuer = "https://auth.example.com"
audience = "test-service"
jwt_secret = "hmac_secret_key"
jwt_public_key = "-----BEGIN PUBLIC KEY-----\nMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE\n-----END PUBLIC KEY-----"
static_tokens = ["token1", "token2"]
"#;
        let c = Config::builder()
            .add_source(File::from_str(toml_str, FileFormat::Toml))
            .build()
            .unwrap();
        let config: AppConfig = c.try_deserialize().unwrap();

        assert_eq!(config.grpc.host, "0.0.0.0");
        assert_eq!(config.grpc.port, 50051);
        assert_eq!(config.grpc.auth.enabled, Some(true));
        assert_eq!(config.grpc.auth.admin_user, "superuser");
        assert_eq!(
            config.grpc.auth.admin_password.as_deref(),
            Some("secret_admin_pass")
        );
        assert_eq!(
            config.grpc.auth.oauth.jwks_url.as_deref(),
            Some("https://auth.example.com/.well-known/jwks.json")
        );
        assert_eq!(
            config.grpc.auth.oauth.issuer.as_deref(),
            Some("https://auth.example.com")
        );
        assert_eq!(
            config.grpc.auth.oauth.audience.as_deref(),
            Some("test-service")
        );
        assert_eq!(
            config.grpc.auth.oauth.jwt_secret.as_deref(),
            Some("hmac_secret_key")
        );
        assert_eq!(
            config.grpc.auth.oauth.static_tokens,
            vec!["token1", "token2"]
        );
        assert!(config.grpc.auth.is_active());
    }

    #[test]
    fn test_grpc_auth_is_active_auto_detection() {
        let mut auth = GrpcAuthConfig::default();
        assert!(!auth.is_active());

        // When admin_password is set, is_active() is true automatically
        auth.admin_password = Some("adminpass".to_string());
        assert!(auth.is_active());

        // When explicit enabled = false, is_active() is false even with password
        auth.enabled = Some(false);
        assert!(!auth.is_active());

        // When explicit enabled = true, is_active() is true
        auth.enabled = Some(true);
        assert!(auth.is_active());

        // When oauth is configured, is_active() is true
        let mut oauth_auth = GrpcAuthConfig::default();
        oauth_auth.oauth.jwt_secret = Some("secret".to_string());
        assert!(oauth_auth.is_active());
    }

    #[test]
    fn test_apply_secret_data_overwrites_grpc_auth() {
        let mut config = AppConfig::default();

        let secret_json = serde_json::json!({
            "admin_password": "overwritten_admin_password",
            "oauth_jwt_secret": "overwritten_jwt_secret"
        });

        let count = config.apply_secret_data(secret_json.as_object().unwrap());
        assert_eq!(count, 2);
        assert_eq!(
            config.grpc.auth.admin_password.as_deref(),
            Some("overwritten_admin_password")
        );
        assert_eq!(
            config.grpc.auth.oauth.jwt_secret.as_deref(),
            Some("overwritten_jwt_secret")
        );
        assert!(config.grpc.auth.is_active());
    }

    #[test]
    fn test_apply_secret_data_recognizes_ai_grpc_oauth_secret() {
        let mut config = AppConfig::default();

        let secret_json = serde_json::json!({
            "ai_grpc_oauth_secret": "my_super_secure_client_secret_from_openbao"
        });

        let count = config.apply_secret_data(secret_json.as_object().unwrap());
        assert_eq!(count, 1);
        assert_eq!(
            config.grpc.auth.oauth.jwt_secret.as_deref(),
            Some("my_super_secure_client_secret_from_openbao")
        );
        assert!(config.grpc.auth.oauth.is_configured());
        assert!(config.grpc.auth.is_active());
    }

    #[test]
    fn test_apply_secret_data_with_custom_configured_vault_keys() {
        let mut config = AppConfig {
            vault: VaultConfig {
                keys: VaultKeysConfig {
                    ai_grpc_oauth_secret: vec!["MY_CUSTOM_OAUTH_TOKEN_KEY".to_string()],
                    gemini_api_key: vec!["CUSTOM_AI_KEY".to_string()],
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let secret_json = serde_json::json!({
            "MY_CUSTOM_OAUTH_TOKEN_KEY": "custom_jwt_token_123",
            "CUSTOM_AI_KEY": "custom_gemini_key_456"
        });

        let count = config.apply_secret_data(secret_json.as_object().unwrap());
        assert_eq!(count, 2);
        assert_eq!(
            config.grpc.auth.oauth.jwt_secret.as_deref(),
            Some("custom_jwt_token_123")
        );
        assert_eq!(config.gemini_api_key, "custom_gemini_key_456");
    }

    #[test]
    fn test_apply_secret_data_extracts_oauth_client_id_and_client_secret() {
        let mut config = AppConfig::default();

        // Uses the exact keys present in OpenBao
        let secret_json = serde_json::json!({
            "oauth-clientname": "ai_agent",
            "outh-secret": "gYOtfRgz4heSn9Ez"
        });

        let count = config.apply_secret_data(secret_json.as_object().unwrap());
        assert_eq!(count, 2);
        assert_eq!(
            config.grpc.auth.oauth.client_id.as_deref(),
            Some("ai_agent")
        );
        assert_eq!(
            config.grpc.auth.oauth.client_secret.as_deref(),
            Some("gYOtfRgz4heSn9Ez")
        );
        assert_eq!(
            config.grpc.auth.oauth.jwt_secret.as_deref(),
            Some("gYOtfRgz4heSn9Ez")
        );
        assert!(config.grpc.auth.oauth.is_configured());
        assert!(config.grpc.auth.is_active());
    }

    #[test]
    fn test_model_provider_parsing_and_display() {
        assert_eq!(
            "gemini".parse::<ModelProvider>().unwrap(),
            ModelProvider::Gemini
        );
        assert_eq!(
            "google".parse::<ModelProvider>().unwrap(),
            ModelProvider::Gemini
        );
        assert_eq!(
            "GEMINI".parse::<ModelProvider>().unwrap(),
            ModelProvider::Gemini
        );
        assert_eq!(
            "qwen".parse::<ModelProvider>().unwrap(),
            ModelProvider::Qwen
        );
        assert_eq!(
            "dashscope".parse::<ModelProvider>().unwrap(),
            ModelProvider::Qwen
        );
        assert_eq!(
            "aliyun".parse::<ModelProvider>().unwrap(),
            ModelProvider::Qwen
        );
        assert_eq!(
            "alibaba".parse::<ModelProvider>().unwrap(),
            ModelProvider::Qwen
        );
        assert_eq!(
            "QWEN".parse::<ModelProvider>().unwrap(),
            ModelProvider::Qwen
        );

        assert!("invalid_provider".parse::<ModelProvider>().is_err());

        assert_eq!(ModelProvider::Gemini.to_string(), "gemini");
        assert_eq!(ModelProvider::Qwen.to_string(), "qwen");
    }

    #[test]
    fn test_apply_secret_data_extracts_qwen_api_key_from_vault() {
        let mut config = AppConfig::default();

        let secret_json = serde_json::json!({
            "qwen_api_key": "sk-qwen-openbao-secret-key-12345",
            "db_password": "surreal_pass"
        });

        let count = config.apply_secret_data(secret_json.as_object().unwrap());
        assert_eq!(count, 2);
        assert_eq!(config.qwen_api_key, "sk-qwen-openbao-secret-key-12345");
        assert_eq!(config.db.password.as_deref(), Some("surreal_pass"));
    }

    #[test]
    fn test_apply_secret_data_extracts_dashscope_api_key_alias() {
        let mut config = AppConfig::default();

        let secret_json = serde_json::json!({
            "dashscope_api_key": "sk-dashscope-secret-999"
        });

        let count = config.apply_secret_data(secret_json.as_object().unwrap());
        assert_eq!(count, 1);
        assert_eq!(config.qwen_api_key, "sk-dashscope-secret-999");
    }

    #[test]
    fn test_deserialize_qwen_config_from_toml() {
        let toml_str = r#"
provider = "qwen"
qwen_api_key = "test_qwen_key_abc"
qwen_base_url = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
qwen_model = "qwen-max"
model = "qwen-plus"
"#;

        let c = Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .unwrap();
        let config: AppConfig = c.try_deserialize().unwrap();

        assert_eq!(config.provider, ModelProvider::Qwen);
        assert_eq!(config.qwen_api_key, "test_qwen_key_abc");
        assert_eq!(
            config.qwen_base_url,
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(config.qwen_model, "qwen-max");
        assert_eq!(config.model, "qwen-plus");
    }

    #[test]
    fn test_deserialize_qwen_config_with_coding_plan_url_and_table_syntax() {
        let toml_str = r#"
provider = "qwen"

[qwen]
base_url = "https://coding.dashscope.aliyuncs.com/v1/"
model = "qwen-turbo"
api_key = "sk-coding-plan-key"
"#;

        let c = Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .unwrap();
        let mut config: AppConfig = c.try_deserialize().unwrap();

        // Apply table overrides like load_from_config() does
        if let Some(ref qwen) = config.qwen {
            if let Some(ref url) = qwen.base_url {
                config.qwen_base_url = url.trim().trim_end_matches('/').to_string();
            }
            if let Some(ref key) = qwen.api_key {
                config.qwen_api_key = key.trim().to_string();
            }
            if let Some(ref m) = qwen.model {
                config.qwen_model = m.trim().to_string();
            }
        }

        assert_eq!(config.provider, ModelProvider::Qwen);
        assert_eq!(
            config.qwen_base_url,
            "https://coding.dashscope.aliyuncs.com/v1"
        );
        assert_eq!(config.qwen_model, "qwen-turbo");
        assert_eq!(config.qwen_api_key, "sk-coding-plan-key");
    }

    #[test]
    fn test_deserialize_qwen_url_aliases() {
        let toml_str = r#"
provider = "qwen"
qwen_url = "https://coding.dashscope.aliyuncs.com/v1"
"#;

        let c = Config::builder()
            .add_source(config::File::from_str(toml_str, config::FileFormat::Toml))
            .build()
            .unwrap();
        let config: AppConfig = c.try_deserialize().unwrap();
        assert_eq!(
            config.qwen_base_url,
            "https://coding.dashscope.aliyuncs.com/v1"
        );
    }

    #[test]
    fn test_apply_secret_data_extracts_qwen_base_url_and_nested() {
        let mut config = AppConfig::default();

        let secret_json = serde_json::json!({
            "qwen_api_key": "sk-flat-key",
            "dashscope_base_url": "https://coding.dashscope.aliyuncs.com/v1/"
        });

        let count = config.apply_secret_data(secret_json.as_object().unwrap());
        assert_eq!(count, 2);
        assert_eq!(config.qwen_api_key, "sk-flat-key");
        assert_eq!(
            config.qwen_base_url,
            "https://coding.dashscope.aliyuncs.com/v1"
        );

        // Nested qwen object
        let mut config2 = AppConfig::default();
        let nested_json = serde_json::json!({
            "qwen": {
                "key": "sk-nested-key",
                "base_url": "https://coding.dashscope.aliyuncs.com/v1"
            }
        });

        let count2 = config2.apply_secret_data(nested_json.as_object().unwrap());
        assert_eq!(count2, 2);
        assert_eq!(config2.qwen_api_key, "sk-nested-key");
        assert_eq!(
            config2.qwen_base_url,
            "https://coding.dashscope.aliyuncs.com/v1"
        );
    }
}
