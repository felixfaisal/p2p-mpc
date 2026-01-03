use jsonrpsee::{
    core::{RpcResult, async_trait},
    proc_macros::rpc,
    server::Server,
};
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Network information shared with RPC server
#[derive(Clone)]
pub struct NetworkInfo {
    pub peer_id: PeerId,
    pub listen_addresses: Arc<RwLock<Vec<Multiaddr>>>,
    pub peers: Arc<RwLock<Vec<PeerId>>>,
    pub topics: Arc<RwLock<Vec<String>>>,
    pub protocol_name: String,
}

/// Response structure for peer list
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PeerListResponse {
    pub peers: Vec<String>,
    pub count: usize,
}

/// Response structure for topic list
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopicListResponse {
    pub topics: Vec<String>,
    pub count: usize,
}

/// Response structure for multiaddresses
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MultiaddressResponse {
    pub addresses: Vec<String>,
    pub count: usize,
}

/// RPC API definition
#[rpc(server)]
pub trait NetworkRpc {
    /// Get list of connected peers
    #[method(name = "network_getPeerList")]
    async fn get_peer_list(&self) -> RpcResult<PeerListResponse>;

    /// Get list of subscribed topics
    #[method(name = "network_getTopicList")]
    async fn get_topic_list(&self) -> RpcResult<TopicListResponse>;

    /// Get the protocol name
    #[method(name = "network_getProtocolName")]
    fn get_protocol_name(&self) -> RpcResult<String>;

    /// Get the local peer ID
    #[method(name = "network_getPeerId")]
    fn get_peer_id(&self) -> RpcResult<String>;

    /// Get multiaddresses the node is listening on
    #[method(name = "network_getMultiaddresses")]
    async fn get_multiaddresses(&self) -> RpcResult<MultiaddressResponse>;
}

/// RPC server implementation
pub struct NetworkRpcImpl {
    info: NetworkInfo,
}

impl NetworkRpcImpl {
    pub fn new(info: NetworkInfo) -> Self {
        Self { info }
    }
}

#[async_trait]
impl NetworkRpcServer for NetworkRpcImpl {
    async fn get_peer_list(&self) -> RpcResult<PeerListResponse> {
        let peers = self.info.peers.read().await;
        let peer_strings: Vec<String> = peers.iter().map(|p| p.to_string()).collect();
        let count = peer_strings.len();

        Ok(PeerListResponse {
            peers: peer_strings,
            count,
        })
    }

    async fn get_topic_list(&self) -> RpcResult<TopicListResponse> {
        let topics = self.info.topics.read().await;
        let topic_list = topics.clone();
        let count = topic_list.len();

        Ok(TopicListResponse {
            topics: topic_list,
            count,
        })
    }

    fn get_protocol_name(&self) -> RpcResult<String> {
        Ok(self.info.protocol_name.clone())
    }

    fn get_peer_id(&self) -> RpcResult<String> {
        Ok(self.info.peer_id.to_string())
    }

    async fn get_multiaddresses(&self) -> RpcResult<MultiaddressResponse> {
        let addrs = self.info.listen_addresses.read().await;
        let addr_strings: Vec<String> = addrs.iter().map(|a| a.to_string()).collect();
        let count = addr_strings.len();

        Ok(MultiaddressResponse {
            addresses: addr_strings,
            count,
        })
    }
}

/// Start the JSON-RPC server
pub async fn start_rpc_server(port: u16, network_info: NetworkInfo) -> anyhow::Result<()> {
    let server = Server::builder().build(format!("0.0.0.0:{}", port)).await?;

    let addr = server.local_addr()?;
    tracing::info!("JSON-RPC server listening on {}", addr);

    let rpc_impl = NetworkRpcImpl::new(network_info);
    let handle = server.start(rpc_impl.into_rpc());

    // Keep the server running
    handle.stopped().await;

    Ok(())
}
