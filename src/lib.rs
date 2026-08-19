pub mod config;
pub mod prompt;
pub mod prompt_typed;

pub use config::{AppConfig, ExecutionVariant, PromptConfig, PromptTypedConfig};
pub use prompt_typed::GdpRecord;
use rig::providers::gemini;

/// Creates a Gemini client configured with the given API key.
pub fn create_gemini_client(api_key: &str) -> Result<gemini::Client, Box<dyn std::error::Error>> {
    let client = gemini::Client::new(api_key)?;
    Ok(client)
}
