use ai::config::AppConfig;
use ai::db::init_db_from_config;
use ai::grpc::start_grpc_server;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let mut config = AppConfig::load().await?;
    let _vault_renewer = config.vault.start_token_renewer()?;
    if _vault_renewer.is_some() {
        println!("OpenBao/Vault automated token renewal: active.");
    }

    println!("=== Tonic gRPC SurrealDB Table Populator Server ===");
    println!("SurrealDB Target: {} (ns: {}, db: {})", config.db.endpoint, config.db.namespace, config.db.database);
    println!("Provider: {} (Gemini Model: {}, Qwen Model: {})", config.provider, config.model, config.qwen_model);
    println!("Listening on: {}:{}", config.grpc.host, config.grpc.port);

    let db = init_db_from_config(&config.db).await?;
    println!("Database authentication check: passed.");
    println!("Database connected successfully.");

    if let Some(report) = ai::grpc::check_oauth_at_startup(&mut config.grpc.auth).await? {
        println!(
            "OAuth authentication check: passed (provider: {}, client: {}).",
            report.issuer.as_deref().unwrap_or("configured"),
            report.client_id.as_deref().unwrap_or("none")
        );
    }

    start_grpc_server(Arc::new(config), Arc::new(db)).await?;

    Ok(())
}
