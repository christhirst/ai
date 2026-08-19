use ai::{config::AppConfig, prompt_typed};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let config = AppConfig::load()?;
    prompt_typed::run(&config).await?;
    Ok(())
}
