use crate::grpc::pb::table_populator_service_client::TablePopulatorServiceClient;
use tonic::transport::Channel;

pub type AgentGrpcClient = TablePopulatorServiceClient<Channel>;

/// Connects to a TablePopulator gRPC server at the given address (e.g. "127.0.0.1:50051" or "http://127.0.0.1:50051").
pub async fn connect_client(addr: &str) -> Result<AgentGrpcClient, Box<dyn std::error::Error>> {
    let endpoint = if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else {
        format!("http://{addr}")
    };
    let client = TablePopulatorServiceClient::connect(endpoint).await?;
    Ok(client)
}
