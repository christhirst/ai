use crate::grpc::pb::table_populator_service_client::TablePopulatorServiceClient;
use base64::Engine;
use tonic::metadata::MetadataValue;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Channel;
use tonic::{Request, Status};

#[derive(Debug, Clone)]
pub enum ClientAuth {
    Basic { user: String, pass: String },
    Bearer(String),
}

impl ClientAuth {
    pub fn to_header_value(&self) -> Result<MetadataValue<tonic::metadata::Ascii>, Box<dyn std::error::Error>> {
        match self {
            ClientAuth::Basic { user, pass } => {
                let raw = format!("{user}:{pass}");
                let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
                let val = format!("Basic {encoded}");
                Ok(val.parse()?)
            }
            ClientAuth::Bearer(token) => {
                let val = format!("Bearer {token}");
                Ok(val.parse()?)
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct ClientAuthInterceptor {
    header_val: Option<MetadataValue<tonic::metadata::Ascii>>,
}

impl tonic::service::Interceptor for ClientAuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if let Some(ref val) = self.header_val {
            request.metadata_mut().insert("authorization", val.clone());
        }
        Ok(request)
    }
}

pub type AgentGrpcClient = TablePopulatorServiceClient<InterceptedService<Channel, ClientAuthInterceptor>>;

/// Connects to a TablePopulator gRPC server without credentials.
pub async fn connect_client(addr: &str) -> Result<AgentGrpcClient, Box<dyn std::error::Error>> {
    connect_client_with_auth(addr, None).await
}

/// Connects to a TablePopulator gRPC server with optional Basic Auth or OAuth Bearer credentials.
pub async fn connect_client_with_auth(
    addr: &str,
    auth: Option<ClientAuth>,
) -> Result<AgentGrpcClient, Box<dyn std::error::Error>> {
    let endpoint = if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else {
        format!("http://{addr}")
    };
    let channel = Channel::from_shared(endpoint)?.connect().await?;
    let header_val = match auth {
        Some(a) => Some(a.to_header_value()?),
        None => None,
    };
    let interceptor = ClientAuthInterceptor { header_val };
    let client = TablePopulatorServiceClient::with_interceptor(channel, interceptor);
    Ok(client)
}
