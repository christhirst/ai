use crate::config::AppConfig;
use rig::prelude::*;
use rig::providers::gemini;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use surrealdb_types::SurrealValue;

/// Strongly-typed structured record for GDP data analysis.
#[derive(Deserialize, Serialize, JsonSchema, SurrealValue, Debug, Clone, PartialEq)]
pub struct GdpRecord {
    /// The GDP value (in trillions USD)
    pub gdp: f64,
    /// The four-digit calendar year (e.g., "1990")
    pub year: String,
}

#[derive(Deserialize, Serialize, JsonSchema, SurrealValue, Debug, Clone, PartialEq)]
pub struct Homecides {
    /// The GDP value (in trillions USD)
    pub amount: i32,
    /// The four-digit calendar year (e.g., "1990")
    pub source: String,
}

/// Executes the typed structured output prompt variant.
pub async fn run(config: &AppConfig) -> Result<Vec<Homecides>, Box<dyn std::error::Error>> {
    // Define the native grounding config as a JSON value
    let _grounding_config = json!({
        "tools": [
            {
                "google_search": {}
            }
        ]
    });

    let client = gemini::Client::new(&config.gemini_api_key)?;

    let mut builder = client.agent(&config.model);

    if let Some(preamble) = &config.preamble {
        builder = builder.preamble(preamble);
        //.additional_params(grounding_config);
    }

    if let Some(temperature) = config.temperature {
        builder = builder.temperature(temperature);
    }

    let agent = builder.build();

    println!("Model: {}", config.model);
    println!("Prompt: {}", config.prompt_typed.query);
    println!("Requesting structured data from Gemini...");

    let response: Vec<Homecides> = agent.prompt_typed(&config.prompt_typed.query).await?;

    println!(
        "\nTyped Agent Response ({} records received):",
        response.len()
    );
    // for item in &response {
    //     println!("  • Year {}: {} Trillion USD", item.year, item.gdp);
    // }
    for item in &response {
        println!("  • Year {}: {} Trillion USD", item.amount, item.source);
    }
    Ok(response)
}
