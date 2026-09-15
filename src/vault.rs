use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use vaultrs::api::AuthInfo;
use vaultrs::api::token::responses::LookupTokenResponse;
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("OpenBao/Vault client error: {0}")]
    Client(#[from] vaultrs::error::ClientError),

    #[error("Failed to build OpenBao/Vault client settings: {0}")]
    ClientBuild(String),

    #[error(
        "OpenBao/Vault integration is enabled, but no authentication token was provided. Please set VAULT_TOKEN / OPENBAO_TOKEN or vault.token in config"
    )]
    MissingToken,

    #[error(
        "Failed to read secret from OpenBao/Vault (KV v{kv_version}) at mount '{mount}', path '{path}': {source}"
    )]
    SecretRead {
        mount: String,
        path: String,
        kv_version: u32,
        #[source]
        source: vaultrs::error::ClientError,
    },

    #[error("Secret payload at OpenBao/Vault '{mount}/{path}' is not a JSON object")]
    NotJsonObject { mount: String, path: String },

    #[error("OpenBao/Vault token lookup failed: {0}")]
    TokenLookup(String),

    #[error("OpenBao/Vault token renewal failed: {0}")]
    TokenRenewal(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VaultConfig {
    #[serde(default = "default_vault_enabled")]
    pub enabled: bool,
    #[serde(default = "default_vault_address")]
    pub address: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default = "default_vault_mount")]
    pub mount: String,
    #[serde(default = "default_vault_path")]
    pub path: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default = "default_vault_kv_version")]
    pub kv_version: u32,
    #[serde(default = "default_vault_auto_renew")]
    pub auto_renew: bool,
    #[serde(default)]
    pub renew_increment: Option<String>,
    #[serde(default)]
    pub keys: VaultKeysConfig,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            enabled: default_vault_enabled(),
            address: default_vault_address(),
            token: None,
            mount: default_vault_mount(),
            path: default_vault_path(),
            namespace: None,
            kv_version: default_vault_kv_version(),
            auto_renew: default_vault_auto_renew(),
            renew_increment: None,
            keys: VaultKeysConfig::default(),
        }
    }
}

/// Configurable mapping of secret key names expected from OpenBao.
/// Each entry can be either a single string or an array of strings in TOML.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct VaultKeysConfig {
    #[serde(
        default = "default_gemini_api_key_keys",
        alias = "api_key",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub gemini_api_key: Vec<String>,
    #[serde(
        default = "default_qwen_api_key_keys",
        alias = "qwen_key",
        alias = "dashscope_api_key",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub qwen_api_key: Vec<String>,
    #[serde(
        default = "default_qwen_base_url_keys",
        alias = "qwen_url",
        alias = "dashscope_base_url",
        alias = "dashscope_url",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub qwen_base_url: Vec<String>,
    #[serde(
        default = "default_db_password_keys",
        alias = "db_password",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub ai_db_password: Vec<String>,
    #[serde(
        default = "default_db_username_keys",
        alias = "db_username",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub ai_db_username: Vec<String>,
    #[serde(
        default = "default_db_endpoint_keys",
        alias = "db_endpoint",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub ai_db_endpoint: Vec<String>,
    #[serde(
        default = "default_db_namespace_keys",
        alias = "db_namespace",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub ai_db_namespace: Vec<String>,
    #[serde(
        default = "default_db_database_keys",
        alias = "db_database",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub ai_db_database: Vec<String>,
    #[serde(
        default = "default_grpc_admin_password_keys",
        alias = "grpc_admin_password",
        alias = "admin_password",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub ai_grpc_admin_password: Vec<String>,
    #[serde(
        default = "default_grpc_oauth_client_id_keys",
        alias = "grpc_oauth_client_id",
        alias = "oauth_client_id",
        alias = "client_id",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub ai_grpc_oauth_client_id: Vec<String>,
    #[serde(
        default = "default_grpc_oauth_secret_keys",
        alias = "grpc_oauth_secret",
        alias = "oauth_secret",
        alias = "oauth_client_secret",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub ai_grpc_oauth_secret: Vec<String>,
}

impl Default for VaultKeysConfig {
    fn default() -> Self {
        Self {
            gemini_api_key: default_gemini_api_key_keys(),
            qwen_api_key: default_qwen_api_key_keys(),
            qwen_base_url: default_qwen_base_url_keys(),
            ai_db_password: default_db_password_keys(),
            ai_db_username: default_db_username_keys(),
            ai_db_endpoint: default_db_endpoint_keys(),
            ai_db_namespace: default_db_namespace_keys(),
            ai_db_database: default_db_database_keys(),
            ai_grpc_admin_password: default_grpc_admin_password_keys(),
            ai_grpc_oauth_client_id: default_grpc_oauth_client_id_keys(),
            ai_grpc_oauth_secret: default_grpc_oauth_secret_keys(),
        }
    }
}

pub fn default_gemini_api_key_keys() -> Vec<String> {
    vec!["gemini_api_key".to_string(), "api_key".to_string()]
}

pub fn default_qwen_api_key_keys() -> Vec<String> {
    vec![
        "qwen_api_key".to_string(),
        "dashscope_api_key".to_string(),
        "qwen".to_string(),
        "dashscope".to_string(),
        "DASHSCOPE_API_KEY".to_string(),
        "QWEN_API_KEY".to_string(),
        "api_key_qwen".to_string(),
    ]
}

pub fn default_qwen_base_url_keys() -> Vec<String> {
    vec![
        "qwen_base_url".to_string(),
        "qwen_url".to_string(),
        "dashscope_base_url".to_string(),
        "dashscope_url".to_string(),
        "QWEN_BASE_URL".to_string(),
        "DASHSCOPE_BASE_URL".to_string(),
    ]
}

pub fn default_db_password_keys() -> Vec<String> {
    vec![
        "db_password".to_string(),
        "password".to_string(),
        "SURREAL_PASS".to_string(),
    ]
}

pub fn default_db_username_keys() -> Vec<String> {
    vec![
        "db_username".to_string(),
        "username".to_string(),
        "SURREAL_USER".to_string(),
    ]
}

pub fn default_db_endpoint_keys() -> Vec<String> {
    vec![
        "db_endpoint".to_string(),
        "endpoint".to_string(),
        "SURREAL_URL".to_string(),
    ]
}

pub fn default_db_namespace_keys() -> Vec<String> {
    vec![
        "db_namespace".to_string(),
        "namespace".to_string(),
        "SURREAL_NS".to_string(),
    ]
}

pub fn default_db_database_keys() -> Vec<String> {
    vec![
        "db_database".to_string(),
        "database".to_string(),
        "SURREAL_DB".to_string(),
    ]
}

pub fn default_grpc_admin_password_keys() -> Vec<String> {
    vec![
        "grpc_admin_password".to_string(),
        "admin_password".to_string(),
    ]
}

pub fn default_grpc_oauth_client_id_keys() -> Vec<String> {
    vec![
        "oauth-clientname".to_string(),
        "oauth_clientname".to_string(),
        "oauth_client_id".to_string(),
        "client_id".to_string(),
        "clientname".to_string(),
    ]
}

pub fn default_grpc_oauth_secret_keys() -> Vec<String> {
    vec![
        "outh-secret".to_string(),
        "oauth-secret".to_string(),
        "oauth_secret".to_string(),
        "oauth_client_secret".to_string(),
        "client_secret".to_string(),
        "ai_grpc_oauth_secret".to_string(),
        "AI_GRPC_OAUTH_SECRET".to_string(),
        "oauth_jwt_secret".to_string(),
        "jwt_secret".to_string(),
    ]
}

/// Serde deserializer helper that accepts either a single string or a sequence of strings.
pub fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrVecVisitor;

    impl<'de> serde::de::Visitor<'de> for StringOrVecVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a list of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(vec![value.to_string()])
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(vec![value])
        }

        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
        where
            S: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(elem) = seq.next_element::<String>()? {
                vec.push(elem);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(StringOrVecVisitor)
}

pub fn default_vault_enabled() -> bool {
    false
}

pub fn default_vault_address() -> String {
    "http://127.0.0.1:8200".to_string()
}

pub fn default_vault_mount() -> String {
    "secret".to_string()
}

pub fn default_vault_path() -> String {
    "ai".to_string()
}

pub fn default_vault_kv_version() -> u32 {
    2
}

pub fn default_vault_auto_renew() -> bool {
    true
}

impl VaultConfig {
    /// Starts the automated background token renewer if `enabled` and `auto_renew` are both true.
    /// Returns `Ok(Some(RenewerHandle))` if spawned, or `Ok(None)` if disabled.
    pub fn start_token_renewer(&self) -> Result<Option<RenewerHandle>, VaultError> {
        if !self.enabled || !self.auto_renew {
            return Ok(None);
        }

        let client = create_vault_client(self)?;
        let options = TokenRenewalOptions {
            increment: self.renew_increment.clone(),
            ..Default::default()
        };

        Ok(Some(spawn_token_renewer(client, options)))
    }
}

/// Builds a `VaultClient` from the provided `VaultConfig`.
pub fn create_vault_client(config: &VaultConfig) -> Result<VaultClient, VaultError> {
    let token = config
        .token
        .as_ref()
        .filter(|t| !t.trim().is_empty())
        .ok_or(VaultError::MissingToken)?;

    let mut builder = VaultClientSettingsBuilder::default();
    builder.address(&config.address);
    builder.token(token);
    if let Some(ref ns) = config.namespace
        && !ns.trim().is_empty()
    {
        builder.namespace(Some(ns.clone()));
    }

    let settings = builder
        .build()
        .map_err(|e| VaultError::ClientBuild(e.to_string()))?;

    let client = VaultClient::new(settings).map_err(|e| VaultError::ClientBuild(e.to_string()))?;

    Ok(client)
}

/// Fetches secret data from OpenBao/Vault at the configured mount and path using KV v1 or KV v2.
pub async fn fetch_secrets(
    client: &VaultClient,
    mount: &str,
    path: &str,
    kv_version: u32,
) -> Result<serde_json::Map<String, serde_json::Value>, VaultError> {
    let secret_value: serde_json::Value = if kv_version == 1 {
        vaultrs::kv1::get(client, mount, path)
            .await
            .map_err(|e| VaultError::SecretRead {
                mount: mount.to_string(),
                path: path.to_string(),
                kv_version: 1,
                source: e,
            })?
    } else {
        vaultrs::kv2::read(client, mount, path)
            .await
            .map_err(|e| VaultError::SecretRead {
                mount: mount.to_string(),
                path: path.to_string(),
                kv_version: 2,
                source: e,
            })?
    };

    let secret_map =
        secret_value
            .as_object()
            .cloned()
            .ok_or_else(|| VaultError::NotJsonObject {
                mount: mount.to_string(),
                path: path.to_string(),
            })?;

    Ok(secret_map)
}

/// Helper to connect and retrieve secrets based directly on a `VaultConfig`.
pub async fn fetch_secrets_from_config(
    config: &VaultConfig,
) -> Result<serde_json::Map<String, serde_json::Value>, VaultError> {
    if !config.enabled {
        return Ok(serde_json::Map::new());
    }
    let client = create_vault_client(config)?;
    fetch_secrets(&client, &config.mount, &config.path, config.kv_version).await
}

/// Looks up the current token metadata from OpenBao/Vault (`/v1/auth/token/lookup-self`).
pub async fn lookup_token(client: &VaultClient) -> Result<LookupTokenResponse, VaultError> {
    vaultrs::token::lookup_self(client)
        .await
        .map_err(|e| VaultError::TokenLookup(e.to_string()))
}

/// Renews the current token lease on OpenBao/Vault (`/v1/auth/token/renew-self`).
pub async fn renew_token(
    client: &VaultClient,
    increment: Option<&str>,
) -> Result<AuthInfo, VaultError> {
    vaultrs::token::renew_self(client, increment)
        .await
        .map_err(|e| VaultError::TokenRenewal(e.to_string()))
}

/// Configuration options for the automated token renewal background task.
#[derive(Debug, Clone)]
pub struct TokenRenewalOptions {
    /// Fraction of remaining lease TTL to wait before attempting renewal (e.g. 0.66 = 2/3 of lease duration).
    /// Defaults to 0.66.
    pub renewal_fraction: f64,
    /// Minimum sleep duration between renewal attempts to avoid rapid polling.
    /// Defaults to 5 seconds.
    pub min_interval: Duration,
    /// Retry interval after a failed renewal call.
    /// Defaults to 10 seconds.
    pub retry_interval: Duration,
    /// Max retries after consecutive failures before giving up.
    /// Defaults to 5.
    pub max_retries: u32,
    /// Optional renewal increment string (e.g. "1h", "24h") passed to OpenBao.
    pub increment: Option<String>,
}

impl Default for TokenRenewalOptions {
    fn default() -> Self {
        Self {
            renewal_fraction: 0.66,
            min_interval: Duration::from_secs(5),
            retry_interval: Duration::from_secs(10),
            max_retries: 5,
            increment: None,
        }
    }
}

/// Computes the duration to sleep before the next token renewal attempt.
pub fn calculate_sleep_duration(ttl_secs: u64, options: &TokenRenewalOptions) -> Duration {
    if ttl_secs <= 1 {
        return Duration::from_secs(1);
    }
    let fraction = if options.renewal_fraction > 0.0 && options.renewal_fraction < 1.0 {
        options.renewal_fraction
    } else {
        0.66
    };

    let target_secs = (ttl_secs as f64 * fraction).round() as u64;
    let max_safe_secs = ttl_secs.saturating_sub(1).max(1);
    let clamped_secs = target_secs
        .max(options.min_interval.as_secs())
        .min(max_safe_secs);
    Duration::from_secs(clamped_secs)
}

/// Handle to the background automated token renewal task.
/// Supports RAII cleanup on drop, manual graceful stop, and detaching.
pub struct RenewerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<JoinHandle<()>>,
}

impl RenewerHandle {
    /// Signals the background renewal task to cleanly terminate.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Aborts the underlying tokio task immediately.
    pub fn abort(&self) {
        if let Some(ref handle) = self.join_handle {
            handle.abort();
        }
    }

    /// Checks if the renewal task has finished.
    pub fn is_finished(&self) -> bool {
        self.join_handle
            .as_ref()
            .map(|h| h.is_finished())
            .unwrap_or(true)
    }

    /// Detaches the renewal task, allowing it to run in the background indefinitely
    /// without stopping when this handle is dropped.
    pub fn detach(mut self) -> Option<JoinHandle<()>> {
        self.shutdown_tx.take();
        self.join_handle.take()
    }

    /// Awaits completion of the background task.
    pub async fn wait_for_completion(mut self) -> Result<(), tokio::task::JoinError> {
        if let Some(handle) = self.join_handle.take() {
            handle.await
        } else {
            Ok(())
        }
    }
}

impl Drop for RenewerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Spawns an asynchronous background task that monitors and renews the OpenBao token.
pub fn spawn_token_renewer(client: VaultClient, options: TokenRenewalOptions) -> RenewerHandle {
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let join_handle = tokio::spawn(async move {
        tracing::info!("Starting OpenBao/Vault automated token renewal task");

        // Step 1: Initial token lookup
        let initial_ttl = match lookup_token(&client).await {
            Ok(lookup) => {
                let renewable = lookup.renewable.unwrap_or(true);
                if !renewable {
                    tracing::warn!(
                        "OpenBao/Vault token is not renewable (renewable=false). Automated renewal will not run."
                    );
                    return;
                }
                if lookup.ttl == 0 {
                    tracing::info!(
                        "OpenBao/Vault token has no expiration (ttl=0). Automated renewal will not run."
                    );
                    return;
                }
                tracing::info!(
                    "OpenBao/Vault initial token lookup succeeded: TTL is {}s (creation_ttl: {}s)",
                    lookup.ttl,
                    lookup.creation_ttl
                );
                lookup.ttl
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to perform initial OpenBao/Vault token lookup: {err}. Attempting renewal with default intervals."
                );
                3600
            }
        };

        let mut next_sleep = calculate_sleep_duration(initial_ttl, &options);
        let mut consecutive_failures = 0;

        loop {
            tracing::debug!(
                "OpenBao/Vault token renewer sleeping for {}s until next renewal attempt",
                next_sleep.as_secs()
            );

            tokio::select! {
                _ = &mut shutdown_rx => {
                    tracing::info!("OpenBao/Vault token renewer task received shutdown signal; stopping.");
                    break;
                }
                _ = tokio::time::sleep(next_sleep) => {
                    tracing::debug!("Initiating OpenBao/Vault token renewal...");
                    match renew_token(&client, options.increment.as_deref()).await {
                        Ok(auth_info) => {
                            consecutive_failures = 0;
                            let lease = auth_info.lease_duration;
                            tracing::info!(
                                "Successfully renewed OpenBao/Vault token! New lease duration: {}s (renewable: {})",
                                lease,
                                auth_info.renewable
                            );

                            if !auth_info.renewable {
                                tracing::warn!(
                                    "OpenBao/Vault token indicates it is no longer renewable; stopping renewer."
                                );
                                break;
                            }

                            next_sleep = calculate_sleep_duration(lease, &options);
                        }
                        Err(err) => {
                            consecutive_failures += 1;
                            tracing::error!(
                                "Failed to renew OpenBao/Vault token (failure {}/{}): {}",
                                consecutive_failures,
                                options.max_retries,
                                err
                            );

                            if consecutive_failures >= options.max_retries {
                                tracing::error!(
                                    "OpenBao/Vault token renewal exceeded max retries ({}); stopping renewer task.",
                                    options.max_retries
                                );
                                break;
                            }

                            next_sleep = options.retry_interval;
                        }
                    }
                }
            }
        }
    });

    RenewerHandle {
        shutdown_tx: Some(shutdown_tx),
        join_handle: Some(join_handle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_vault_config() {
        let config = VaultConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.address, "http://127.0.0.1:8200");
        assert_eq!(config.mount, "secret");
        assert_eq!(config.path, "ai");
        assert_eq!(config.kv_version, 2);
        assert!(config.auto_renew);
        assert_eq!(config.renew_increment, None);
    }

    #[test]
    fn test_calculate_sleep_duration_standard() {
        let options = TokenRenewalOptions {
            renewal_fraction: 0.66,
            min_interval: Duration::from_secs(5),
            retry_interval: Duration::from_secs(10),
            max_retries: 5,
            increment: None,
        };

        // 8 hours = 28800s -> ~19008s
        let sleep_8h = calculate_sleep_duration(28800, &options);
        assert_eq!(sleep_8h.as_secs(), 19008);

        // 1 hour = 3600s -> ~2376s
        let sleep_1h = calculate_sleep_duration(3600, &options);
        assert_eq!(sleep_1h.as_secs(), 2376);

        // 60s -> ~40s
        let sleep_60s = calculate_sleep_duration(60, &options);
        assert_eq!(sleep_60s.as_secs(), 40);
    }

    #[test]
    fn test_calculate_sleep_duration_clamps_and_bounds() {
        let options = TokenRenewalOptions {
            renewal_fraction: 0.5,
            min_interval: Duration::from_secs(10),
            retry_interval: Duration::from_secs(5),
            max_retries: 3,
            increment: None,
        };

        // Small TTL = 12s -> fraction gives 6s, clamped by min_interval (10s) -> 10s, max safe is 11s -> 10s
        assert_eq!(calculate_sleep_duration(12, &options).as_secs(), 10);

        // Tiny TTL = 4s -> max safe is 3s -> clamped to 3s (never exceeding ttl-1)
        assert_eq!(calculate_sleep_duration(4, &options).as_secs(), 3);

        // Edge case: TTL <= 1s
        assert_eq!(calculate_sleep_duration(1, &options).as_secs(), 1);
        assert_eq!(calculate_sleep_duration(0, &options).as_secs(), 1);
    }

    #[test]
    fn test_create_vault_client_missing_token() {
        let config = VaultConfig {
            enabled: true,
            address: "http://127.0.0.1:8200".to_string(),
            token: None,
            ..Default::default()
        };
        let res = create_vault_client(&config);
        assert!(matches!(res, Err(VaultError::MissingToken)));
    }

    #[test]
    fn test_create_vault_client_with_valid_settings() {
        let config = VaultConfig {
            enabled: true,
            address: "http://127.0.0.1:8200".to_string(),
            token: Some("dummy_token".to_string()),
            namespace: Some("test_ns".to_string()),
            ..Default::default()
        };
        let res = create_vault_client(&config);
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_renewer_handle_lifecycle() {
        let (tx, rx) = oneshot::channel();
        let join_handle = tokio::spawn(async move {
            let _ = rx.await;
        });

        let mut handle = RenewerHandle {
            shutdown_tx: Some(tx),
            join_handle: Some(join_handle),
        };

        assert!(!handle.is_finished());
        handle.stop();
        // Await completion
        let res = handle.wait_for_completion().await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_renewer_handle_drop_cleans_up() {
        let (tx, rx) = oneshot::channel();
        let join_handle = tokio::spawn(async move {
            let _ = rx.await;
        });

        {
            let _handle = RenewerHandle {
                shutdown_tx: Some(tx),
                join_handle: Some(join_handle),
            };
            // _handle dropped here, sends stop signal
        }
    }

    #[test]
    fn test_deserialize_vault_keys_config_single_string_and_array() {
        let json_str = r#"{
            "gemini_api_key": "MY_GEMINI_KEY",
            "grpc_oauth_client_id": "oauth-clientname",
            "grpc_oauth_secret": ["AI_GRPC_OAUTH_SECRET", "oauth_jwt_secret"],
            "db_password": "PROD_SURREAL_PASSWORD"
        }"#;
        let keys: VaultKeysConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(keys.gemini_api_key, vec!["MY_GEMINI_KEY".to_string()]);
        assert_eq!(
            keys.ai_grpc_oauth_client_id,
            vec!["oauth-clientname".to_string()]
        );
        assert_eq!(
            keys.ai_grpc_oauth_secret,
            vec![
                "AI_GRPC_OAUTH_SECRET".to_string(),
                "oauth_jwt_secret".to_string()
            ]
        );
        assert_eq!(
            keys.ai_db_password,
            vec!["PROD_SURREAL_PASSWORD".to_string()]
        );
        // Default fields should be populated
        assert_eq!(keys.qwen_api_key, default_qwen_api_key_keys());
        assert_eq!(keys.qwen_base_url, default_qwen_base_url_keys());
        assert_eq!(keys.ai_db_username, default_db_username_keys());
    }

    #[test]
    fn test_deserialize_vault_config_with_keys_section() {
        let json_str = r#"{
            "enabled": true,
            "address": "https://secrets.example.com",
            "keys": {
                "grpc_oauth_client_id": "oauth-clientname",
                "grpc_oauth_secret": "AI_GRPC_OAUTH_SECRET"
            }
        }"#;
        let vault: VaultConfig = serde_json::from_str(json_str).unwrap();
        assert!(vault.enabled);
        assert_eq!(
            vault.keys.ai_grpc_oauth_client_id,
            vec!["oauth-clientname".to_string()]
        );
        assert_eq!(
            vault.keys.ai_grpc_oauth_secret,
            vec!["AI_GRPC_OAUTH_SECRET".to_string()]
        );
    }
}
