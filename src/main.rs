use config::Config;
use rig::prelude::*;
use rig::providers::gemini;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Config::builder()
        .add_source(config::File::with_name("config/config").required(false))
        .add_source(config::File::with_name("config").required(false))
        .add_source(config::Environment::default())
        .build()?;

    let api_key = settings.get_string("gemini_api_key")?;

    let client = gemini::Client::new(&api_key)?;

    /* let model_list = client.list_models().await?;
    println!("Model list: {:?}", model_list); */

    let agent = client
        .agent("gemini-3.5-flash-lite")
        .preamble("You are a helpful assistant.")
        .build();

    let response = agent.prompt("Hello! Tell me a one-sentence joke.").await?;
    println!("Agent response: {}", response);

    Ok(())
}
