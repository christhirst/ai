use ai::{config::AppConfig, prompt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let config = AppConfig::load()?;
    prompt::run(&config).await?;
    Ok(())
}
