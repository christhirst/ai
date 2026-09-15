use crate::config::{AppConfig, ModelProvider};
use rig::prelude::*;
use rig::providers::{gemini, openai};

/// Executes the standard (untyped) text prompt variant.
pub async fn run(config: &AppConfig) -> Result<String, Box<dyn std::error::Error>> {
    match config.provider {
        ModelProvider::Gemini => {
            let client = gemini::Client::new(&config.gemini_api_key)?;
            let mut builder = client.agent(&config.model);

            if let Some(preamble) = &config.preamble {
                builder = builder.preamble(preamble);
            }

            if let Some(temperature) = config.temperature {
                builder = builder.temperature(temperature);
            }

            let agent = builder.build();

            println!("Provider: gemini | Model: {}", config.model);
            println!("Sending request to Gemini...");

            let response = agent.prompt(&config.prompt.query).await?;
            println!("\nAgent Response:\n{}", response);
            Ok(response)
        }
        ModelProvider::Qwen => {
            let model = if config.model.contains("gemini") {
                &config.qwen_model
            } else {
                &config.model
            };

            let client = openai::CompletionsClient::builder()
                .api_key(&config.qwen_api_key)
                .base_url(&config.qwen_base_url)
                .build()?;
            let mut builder = client.agent(model);

            if let Some(preamble) = &config.preamble {
                builder = builder.preamble(preamble);
            }

            if let Some(temperature) = config.temperature {
                builder = builder.temperature(temperature);
            }

            let agent = builder.build();

            println!(
                "Provider: qwen | Model: {} (base_url: {})",
                model, config.qwen_base_url
            );
            println!("Sending request to Qwen...");

            let response = agent.prompt(&config.prompt.query).await?;
            println!("\nAgent Response:\n{}", response);
            Ok(response)
        }
    }
}
