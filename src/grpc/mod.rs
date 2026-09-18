pub mod auth;
pub mod client;
pub mod extractor;
pub mod intervals;
pub mod service;

pub mod pb {
    tonic::include_proto!("table_populator");
}

pub use auth::{
    AuthIdentity, AuthValidator, GrpcAuthInterceptor, OAuthCheckReport, OidcDiscoveryDocument,
    check_oauth_at_startup, create_auth_layer, discover_oidc_endpoints,
};
pub use client::{AgentGrpcClient, ClientAuth, connect_client, connect_client_with_auth};
pub use extractor::{extract_table_data, parse_json_response};
pub use intervals::{
    DateIntervalStep, IntervalType, generate_interval_steps, inject_timeframe_into_prompt,
    parse_interval,
};
pub use pb::table_populator_service_server::{TablePopulatorService, TablePopulatorServiceServer};
pub use pb::*;
pub use service::{TablePopulatorServiceImpl, sanitize_and_map_records};

use crate::config::AppConfig;
use crate::db::AppDb;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;

pub const FILE_DESCRIPTOR_SET: &[u8] =
    tonic::include_file_descriptor_set!("table_populator_descriptor");

/// Starts the Tonic gRPC server on the configured address and port.
pub async fn start_grpc_server(
    config: Arc<AppConfig>,
    db: Arc<AppDb>,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr_str = format!("{}:{}", config.grpc.host, config.grpc.port);
    let addr: SocketAddr = addr_str.parse()?;

    let service = TablePopulatorServiceImpl::new(config.clone(), db);
    let svc = TablePopulatorServiceServer::new(service);
    let reflection_v1 = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()?;
    let reflection_v1alpha = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1alpha()?;

    println!("Starting Tonic gRPC TablePopulator server on {addr}...");
    tracing::info!(host = %config.grpc.host, port = %config.grpc.port, "Tonic gRPC server listening");

    if config.grpc.auth.is_active() {
        println!("gRPC Authentication: ACTIVE");
        if config.grpc.auth.admin_password.is_some() {
            println!(
                " - Basic Auth: ENABLED (admin user: '{}')",
                config.grpc.auth.admin_user
            );
        }
        if config.grpc.auth.oauth.is_configured() {
            println!(" - OAuth 2.0 Bearer: ENABLED");
        }
        let auth_layer = create_auth_layer(&config.grpc.auth)?;
        Server::builder()
            .layer(auth_layer)
            .add_service(reflection_v1)
            .add_service(reflection_v1alpha)
            .add_service(svc)
            .serve(addr)
            .await?;
    } else {
        println!("gRPC Authentication: DISABLED (public access)");
        Server::builder()
            .add_service(reflection_v1)
            .add_service(reflection_v1alpha)
            .add_service(svc)
            .serve(addr)
            .await?;
    }

    Ok(())
}
