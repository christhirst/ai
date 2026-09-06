pub mod client;
pub mod extractor;
pub mod intervals;
pub mod service;

pub mod pb {
    tonic::include_proto!("table_populator");
}

pub use client::{connect_client, AgentGrpcClient};
pub use extractor::{extract_table_data, parse_json_response};
pub use intervals::{
    generate_interval_steps, inject_timeframe_into_prompt, parse_interval, DateIntervalStep,
    IntervalType,
};
pub use pb::table_populator_service_server::{TablePopulatorService, TablePopulatorServiceServer};
pub use pb::*;
pub use service::{sanitize_and_map_records, TablePopulatorServiceImpl};

use crate::config::AppConfig;
use crate::db::AppDb;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;

pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("table_populator_descriptor");

/// Starts the Tonic gRPC server on the configured address and port.
pub async fn start_grpc_server(
    config: Arc<AppConfig>,
    db: Arc<AppDb>,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr_str = format!("{}:{}", config.grpc.host, config.grpc.port);
    let addr: SocketAddr = addr_str.parse()?;

    let service = TablePopulatorServiceImpl::new(config.clone(), db);
    let svc = TablePopulatorServiceServer::new(service);
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()?;

    println!("Starting Tonic gRPC TablePopulator server on {addr}...");
    tracing::info!(host = %config.grpc.host, port = %config.grpc.port, "Tonic gRPC server listening");

    Server::builder()
        .add_service(reflection)
        .add_service(svc)
        .serve(addr)
        .await?;

    Ok(())
}
