use ai::config::{AppConfig, ExecutionVariant};
use ai::{prompt, prompt_typed};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let config = AppConfig::load()?;

    println!("Configuration loaded successfully.");
    println!("Selected Variant: {}\n", config.variant);

    match config.variant {
        ExecutionVariant::Normal => {
            prompt::run(&config).await?;
        }
        ExecutionVariant::Typed => {
            prompt_typed::run(&config).await?;
        }
        ExecutionVariant::All => {
            println!("=== Variant 1: Normal Prompt ===");
            prompt::run(&config).await?;
            println!("\n=== Variant 2: Typed Prompt ===");
            prompt_typed::run(&config).await?;
        }
    }

    Ok(())
}
