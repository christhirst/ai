use crate::config::AppConfig;
use rig::prelude::*;
use rig::providers::gemini;

/// Executes the standard (untyped) text prompt variant.
pub async fn run(config: &AppConfig) -> Result<String, Box<dyn std::error::Error>> {
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
    println!("Prompt: {}", config.prompt.query);
    println!("Sending request to Gemini...");

    let response = agent.prompt(&config.prompt.query).await?;

    println!("\nAgent Response:\n{}", response);
    Ok(response)
}
