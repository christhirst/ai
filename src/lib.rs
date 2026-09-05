pub mod config;
pub mod db;
pub mod grpc;
pub mod prompt;
pub mod prompt_typed;

pub use config::{AppConfig, DatabaseConfig, ExecutionVariant, GrpcConfig, PromptConfig, PromptTypedConfig};
pub use db::{AppDb, LocalDb};
pub use grpc::{
    connect_client, start_grpc_server, AgentGrpcClient, PopulateTableRequest, PopulateTableResponse,
    TablePopulatorService, TablePopulatorServiceServer, TablePopulatorServiceImpl,
};
pub use prompt_typed::{GdpRecord, Homecides};
use rig::providers::gemini;

/// Creates a Gemini client configured with the given API key.
pub fn create_gemini_client(api_key: &str) -> Result<gemini::Client, Box<dyn std::error::Error>> {
    let client = gemini::Client::new(api_key)?;
    Ok(client)
}
