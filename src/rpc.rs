use crate::network::{AssignmentMessage, NetworkCommand};
use jsonrpsee::{
    core::{RpcResult, async_trait},
    proc_macros::rpc,
    server::Server,
};
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

/// Session information for tracking peer assignments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub coordinator_id: String,
    pub assignments: HashMap<String, usize>,
    pub acceptances: HashMap<String, bool>,
    pub created_at: u64,
    pub total_nodes: usize,
}

impl SessionInfo {
    pub fn new(
        session_id: String,
        coordinator_id: String,
        assignments: HashMap<String, usize>,
    ) -> Self {
        let total_nodes = assignments.len();
        Self {
            session_id,
            coordinator_id,
            assignments,
            acceptances: HashMap::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            total_nodes,
        }
    }

    pub fn acceptance_count(&self) -> usize {
        self.acceptances.values().filter(|&&v| v).count()
    }

    pub fn all_accepted(&self) -> bool {
        self.acceptances.len() == self.total_nodes - 1 && // Exclude coordinator
        self.acceptances.values().all(|&v| v)
    }
}

/// Network information shared with RPC server
#[derive(Clone)]
pub struct NetworkInfo {
    pub peer_id: PeerId,
    pub listen_addresses: Arc<RwLock<Vec<Multiaddr>>>,
    pub peers: Arc<RwLock<Vec<PeerId>>>,
    pub topics: Arc<RwLock<Vec<String>>>,
    pub protocol_name: String,
    pub network_command_sender: mpsc::UnboundedSender<NetworkCommand>,
    pub peer_assignments: Arc<RwLock<HashMap<String, usize>>>,
    pub assignment_acceptances: Arc<RwLock<HashMap<String, bool>>>,
    // Session management
    pub sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
    pub current_session_id: Arc<RwLock<Option<String>>>,
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

/// Response structure for broadcast operation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BroadcastResponse {
    pub success: bool,
    pub message: String,
}

/// Response structure for peer assignment
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssignmentResponse {
    pub success: bool,
    pub session_id: String,
    pub assignments: HashMap<String, usize>,
    pub message: String,
}

/// Response structure for session status query
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionStatusResponse {
    pub success: bool,
    pub session_id: String,
    pub coordinator_id: String,
    pub assignments: HashMap<String, usize>,
    pub acceptances: HashMap<String, bool>,
    pub acceptance_count: usize,
    pub total_nodes: usize,
    pub all_accepted: bool,
    pub created_at: u64,
    pub message: String,
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

    /// Broadcast a message to a gossipsub topic
    #[method(name = "network_broadcast")]
    async fn broadcast(&self, topic: String, message: String) -> RpcResult<BroadcastResponse>;

    /// Assign indices to all connected peers
    #[method(name = "network_assignPeerIndices")]
    async fn assign_peer_indices(&self) -> RpcResult<AssignmentResponse>;

    /// Get status of a specific session or current session
    #[method(name = "network_getSessionStatus")]
    async fn get_session_status(
        &self,
        session_id: Option<String>,
    ) -> RpcResult<SessionStatusResponse>;
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

    async fn broadcast(&self, topic: String, message: String) -> RpcResult<BroadcastResponse> {
        let data = message.into_bytes();

        match self
            .info
            .network_command_sender
            .send(NetworkCommand::Broadcast {
                topic: topic.clone(),
                data,
            }) {
            Ok(_) => {
                tracing::info!(target: "RpcServer", "Broadcast request sent for topic: {}", topic);
                Ok(BroadcastResponse {
                    success: true,
                    message: format!("Message broadcast to topic '{}' initiated", topic),
                })
            }
            Err(e) => {
                tracing::error!(target: "RpcServer", "Failed to send broadcast command: {}", e);
                Ok(BroadcastResponse {
                    success: false,
                    message: format!("Failed to broadcast: {}", e),
                })
            }
        }
    }

    async fn assign_peer_indices(&self) -> RpcResult<AssignmentResponse> {
        // Get list of connected peers
        let peers = self.info.peers.read().await;
        let peer_list: Vec<String> = peers.iter().map(|p| p.to_string()).collect();

        // Create assignments: peer_id -> index
        // Start with coordinator (ourselves) at index 0
        let mut assignments = HashMap::new();
        let coordinator_id = self.info.peer_id.to_string();
        assignments.insert(coordinator_id.clone(), 0);

        // Assign indices to connected peers starting from 1
        for (offset, peer_id) in peer_list.iter().enumerate() {
            assignments.insert(peer_id.clone(), offset + 1);
        }

        // Generate unique session ID
        let session_id = format!(
            "{}-{}",
            self.info.peer_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );

        // Create and store session info
        let session_info = SessionInfo::new(
            session_id.clone(),
            self.info.peer_id.to_string(),
            assignments.clone(),
        );

        {
            let mut sessions = self.info.sessions.write().await;
            sessions.insert(session_id.clone(), session_info.clone());
        }

        // Update current session ID
        {
            let mut current = self.info.current_session_id.write().await;
            *current = Some(session_id.clone());
        }

        // Store assignments locally (for backwards compatibility)
        {
            let mut local_assignments = self.info.peer_assignments.write().await;
            *local_assignments = assignments.clone();
        }

        // Clear previous acceptances (for backwards compatibility)
        {
            let mut acceptances = self.info.assignment_acceptances.write().await;
            acceptances.clear();
        }

        // Create assignment message
        let assignment_msg = AssignmentMessage::Request {
            coordinator_id: self.info.peer_id.to_string(),
            assignments: assignments.clone(),
            session_id: session_id.clone(),
        };

        // Serialize and broadcast
        match serde_json::to_string(&assignment_msg) {
            Ok(json) => {
                let data = json.into_bytes();
                match self
                    .info
                    .network_command_sender
                    .send(NetworkCommand::Broadcast {
                        topic: "mpc-assignment".to_string(),
                        data,
                    }) {
                    Ok(_) => {
                        tracing::info!(
                            target: "RpcServer",
                            "Sent peer assignment for session {} to {} peers (including coordinator)",
                            session_id,
                            assignments.len()
                        );
                        Ok(AssignmentResponse {
                            success: true,
                            session_id,
                            assignments: assignments.clone(),
                            message: format!(
                                "Assignment created for {} total nodes: coordinator (index 0) + {} peers",
                                assignments.len(),
                                peer_list.len()
                            ),
                        })
                    }
                    Err(e) => {
                        tracing::error!(target: "RpcServer", "Failed to broadcast assignment: {}", e);
                        Ok(AssignmentResponse {
                            success: false,
                            session_id,
                            assignments: HashMap::new(),
                            message: format!("Failed to broadcast assignment: {}", e),
                        })
                    }
                }
            }
            Err(e) => {
                tracing::error!(target: "RpcServer", "Failed to serialize assignment: {}", e);
                Ok(AssignmentResponse {
                    success: false,
                    session_id,
                    assignments: HashMap::new(),
                    message: format!("Failed to serialize assignment: {}", e),
                })
            }
        }
    }

    async fn get_session_status(
        &self,
        session_id: Option<String>,
    ) -> RpcResult<SessionStatusResponse> {
        // Determine which session to query
        let target_session_id = match session_id {
            Some(id) => id,
            None => {
                // Get current session ID
                let current = self.info.current_session_id.read().await;
                match current.as_ref() {
                    Some(id) => id.clone(),
                    None => {
                        return Ok(SessionStatusResponse {
                            success: false,
                            session_id: String::new(),
                            coordinator_id: String::new(),
                            assignments: HashMap::new(),
                            acceptances: HashMap::new(),
                            acceptance_count: 0,
                            total_nodes: 0,
                            all_accepted: false,
                            created_at: 0,
                            message: "No active session found".to_string(),
                        });
                    }
                }
            }
        };

        // Look up session info
        let sessions = self.info.sessions.read().await;
        match sessions.get(&target_session_id) {
            Some(session) => {
                let acceptance_count = session.acceptance_count();
                let all_accepted = session.all_accepted();

                Ok(SessionStatusResponse {
                    success: true,
                    session_id: session.session_id.clone(),
                    coordinator_id: session.coordinator_id.clone(),
                    assignments: session.assignments.clone(),
                    acceptances: session.acceptances.clone(),
                    acceptance_count,
                    total_nodes: session.total_nodes,
                    all_accepted,
                    created_at: session.created_at,
                    message: if all_accepted {
                        format!(
                            "All {} peers have accepted their assignments",
                            acceptance_count
                        )
                    } else {
                        format!(
                            "Waiting for acceptances: {}/{} received",
                            acceptance_count,
                            session.total_nodes - 1 // Exclude coordinator
                        )
                    },
                })
            }
            None => Ok(SessionStatusResponse {
                success: false,
                session_id: target_session_id.clone(),
                coordinator_id: String::new(),
                assignments: HashMap::new(),
                acceptances: HashMap::new(),
                acceptance_count: 0,
                total_nodes: 0,
                all_accepted: false,
                created_at: 0,
                message: format!("Session '{}' not found", target_session_id),
            }),
        }
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
