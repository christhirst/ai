use crate::config::{AppConfig, ModelProvider};
use rig::prelude::*;
use rig::providers::{gemini, openai};
use serde_json::json;

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

            builder = builder.additional_params(json!({
                "enable_search": true,
                "search_options": {
                    "forced_search": true,
                    "search_strategy": "max",
                    "enable_source": true
                }
            }));

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
        ModelProvider::Local1 => {
            let local_cfg = config
                .local1
                .as_ref()
                .ok_or("Local1 configuration is missing in config.toml or environment variables")?;
            let model = local_cfg
                .model
                .as_deref()
                .unwrap_or(&config.model);
            let api_key = local_cfg
                .api_key
                .as_deref()
                .filter(|k| !k.trim().is_empty())
                .unwrap_or("none");
            let base_url = local_cfg
                .base_url
                .as_deref()
                .unwrap_or("http://localhost:8000/v1");

            let client = openai::CompletionsClient::builder()
                .api_key(api_key)
                .base_url(base_url)
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
                "Provider: local1 | Model: {} (base_url: {})",
                model, base_url
            );
            println!("Sending request to Local1...");

            let response = agent.prompt(&config.prompt.query).await?;
            println!("\nAgent Response:\n{}", response);
            Ok(response)
        }
        ModelProvider::Local2 => {
            let local_cfg = config
                .local2
                .as_ref()
                .ok_or("Local2 configuration is missing in config.toml or environment variables")?;
            let model = local_cfg
                .model
                .as_deref()
                .unwrap_or(&config.model);
            let api_key = local_cfg
                .api_key
                .as_deref()
                .filter(|k| !k.trim().is_empty())
                .unwrap_or("none");
            let base_url = local_cfg
                .base_url
                .as_deref()
                .unwrap_or("http://localhost:8000/v1");

            let client = openai::CompletionsClient::builder()
                .api_key(api_key)
                .base_url(base_url)
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
                "Provider: local2 | Model: {} (base_url: {})",
                model, base_url
            );
            println!("Sending request to Local2...");

            let response = agent.prompt(&config.prompt.query).await?;
            println!("\nAgent Response:\n{}", response);
            Ok(response)
        }
    }
}
