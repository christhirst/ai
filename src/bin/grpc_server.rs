use ai::config::AppConfig;
use ai::db::init_db_from_config;
use ai::grpc::start_grpc_server;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = AppConfig::load()?;
    println!("=== Tonic gRPC SurrealDB Table Populator Server ===");
    println!("SurrealDB Target: {} (ns: {}, db: {})", config.db.endpoint, config.db.namespace, config.db.database);
    println!("Gemini Model: {}", config.model);
    println!("Listening on: {}:{}", config.grpc.host, config.grpc.port);

    let db = init_db_from_config(&config.db).await?;
    println!("Database connected successfully.");

    start_grpc_server(Arc::new(config), Arc::new(db)).await?;

    Ok(())
}
