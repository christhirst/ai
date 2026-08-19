use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::fmt;

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
            .add_source(File::with_name("config/config").required(false))
            .add_source(File::with_name("config").required(false))
            .add_source(Environment::with_prefix("APP").separator("_"))
            .add_source(Environment::default());

        let config = builder.build()?;
        config.try_deserialize::<AppConfig>()
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
    }
}
