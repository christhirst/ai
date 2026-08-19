use crate::config::AppConfig;
use rig::prelude::*;
use rig::providers::gemini;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Strongly-typed structured record for GDP data analysis.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct GdpRecord {
    /// The GDP value (in trillions USD)
    pub gdp: f64,
    /// The four-digit calendar year (e.g., "1990")
    pub year: String,
}

/// Executes the typed structured output prompt variant.
pub async fn run(config: &AppConfig) -> Result<Vec<GdpRecord>, Box<dyn std::error::Error>> {
    let client = gemini::Client::new(&config.gemini_api_key)?;

    let mut builder = client.agent(&config.model);

    if let Some(preamble) = &config.preamble {
        builder = builder.preamble(preamble);
    }

    if let Some(temperature) = config.temperature {
        builder = builder.temperature(temperature);
    }

    let agent = builder.build();

    println!("Model: {}", config.model);
    println!("Prompt: {}", config.prompt_typed.query);
    println!("Requesting structured data from Gemini...");

    let response: Vec<GdpRecord> = agent.prompt_typed(&config.prompt_typed.query).await?;

    println!(
        "\nTyped Agent Response ({} records received):",
        response.len()
    );
    for item in &response {
        println!("  • Year {}: {} Trillion USD", item.year, item.gdp);
    }

    Ok(response)
}
