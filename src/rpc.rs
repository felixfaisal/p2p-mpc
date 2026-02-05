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
    // MPC Orchestrator
    pub mpc_orchestrator: Arc<RwLock<Option<crate::mpc::MPCOrchestrator>>>,
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

/// Response structure for MPC aux generation initiation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InitiateAuxGenResponse {
    pub success: bool,
    pub message: String,
    pub execution_id: Option<String>,
}

/// Response structure for MPC keygen initiation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InitiateKeygenResponse {
    pub success: bool,
    pub message: String,
    pub execution_id: Option<String>,
    pub threshold: Option<u16>,
    pub total_parties: Option<u16>,
}

/// Response structure for MPC signing initiation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InitiateSigningResponse {
    pub success: bool,
    pub message: String,
    pub execution_id: Option<String>,
    pub signers: Option<Vec<u16>>,
    pub signature: Option<String>,
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

    /// Initiate MPC aux generation phase
    #[method(name = "mpc_initiateAuxGen")]
    async fn initiate_aux_gen(&self) -> RpcResult<InitiateAuxGenResponse>;

    /// Initiate MPC keygen phase
    #[method(name = "mpc_initiateKeygen")]
    async fn initiate_keygen(&self, threshold: u16) -> RpcResult<InitiateKeygenResponse>;

    /// Initiate MPC signing phase
    #[method(name = "mpc_initiateSigning")]
    async fn initiate_signing(
        &self,
        message: String,
        signers: Vec<u16>,
    ) -> RpcResult<InitiateSigningResponse>;
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
    #[tracing::instrument(skip(self), fields(peer_id = %self.info.peer_id))]
    async fn get_peer_list(&self) -> RpcResult<PeerListResponse> {
        let peers = self.info.peers.read().await;
        let peer_strings: Vec<String> = peers.iter().map(|p| p.to_string()).collect();
        let count = peer_strings.len();

        Ok(PeerListResponse {
            peers: peer_strings,
            count,
        })
    }

    #[tracing::instrument(skip(self), fields(peer_id = %self.info.peer_id))]
    async fn get_topic_list(&self) -> RpcResult<TopicListResponse> {
        let topics = self.info.topics.read().await;
        let topic_list = topics.clone();
        let count = topic_list.len();

        Ok(TopicListResponse {
            topics: topic_list,
            count,
        })
    }

    #[tracing::instrument(skip(self), fields(peer_id = %self.info.peer_id))]
    fn get_protocol_name(&self) -> RpcResult<String> {
        Ok(self.info.protocol_name.clone())
    }

    #[tracing::instrument(skip(self), fields(peer_id = %self.info.peer_id))]
    fn get_peer_id(&self) -> RpcResult<String> {
        Ok(self.info.peer_id.to_string())
    }

    #[tracing::instrument(skip(self), fields(peer_id = %self.info.peer_id))]
    async fn get_multiaddresses(&self) -> RpcResult<MultiaddressResponse> {
        let addrs = self.info.listen_addresses.read().await;
        let addr_strings: Vec<String> = addrs.iter().map(|a| a.to_string()).collect();
        let count = addr_strings.len();

        Ok(MultiaddressResponse {
            addresses: addr_strings,
            count,
        })
    }

    #[tracing::instrument(skip(self, message), fields(peer_id = %self.info.peer_id, topic = %topic, message_len = message.len()))]
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

    #[tracing::instrument(skip(self), fields(peer_id = %self.info.peer_id))]
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

    #[tracing::instrument(skip(self), fields(peer_id = %self.info.peer_id, session_id = ?session_id))]
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

    #[tracing::instrument(skip(self), fields(peer_id = %self.info.peer_id))]
    async fn initiate_aux_gen(&self) -> RpcResult<InitiateAuxGenResponse> {
        use std::time::SystemTime;

        // Check if orchestrator is initialized
        let mut orchestrator_lock = self.info.mpc_orchestrator.write().await;
        let orchestrator = match orchestrator_lock.as_mut() {
            Some(orch) => orch,
            None => {
                return Ok(InitiateAuxGenResponse {
                    success: false,
                    message: "MPC orchestrator not initialized. Please assign peer indices first."
                        .to_string(),
                    execution_id: None,
                });
            }
        };

        // Get orchestrator state
        let state = orchestrator.get_state().await;

        // Verify we're in Idle state
        match state {
            crate::mpc::ProtocolState::Idle => {
                // Good to proceed
            }
            _ => {
                return Ok(InitiateAuxGenResponse {
                    success: false,
                    message: format!("Cannot initiate aux generation. Current state: {:?}", state),
                    execution_id: None,
                });
            }
        }

        // Create execution ID
        let timestamp = SystemTime::now();
        let execution_id = orchestrator.create_execution_id("aux", timestamp);
        let execution_id_hex = hex::encode(&execution_id);

        // Get participants and party assignments from orchestrator
        let participants = orchestrator.participants.clone();
        let party_assignments = orchestrator.party_assignments.clone();

        // Create InitiateAux coordination message
        let coord_message = crate::mpc::CoordinationMessage::InitiateAux {
            execution_id: execution_id.clone(),
            participants: participants.clone(),
            party_assignments: party_assignments.clone(),
            timestamp,
        };

        // Serialize and broadcast via P2P
        match serde_json::to_string(&coord_message) {
            Ok(json) => {
                let data = json.into_bytes();
                match self.info.network_command_sender.send(
                    crate::network::NetworkCommand::Broadcast {
                        topic: crate::network::MPC_COORDINATION_TOPIC.to_string(),
                        data,
                    },
                ) {
                    Ok(_) => {
                        tracing::info!(
                            "📡 Broadcasted InitiateAux message (execution_id: {})",
                            execution_id_hex
                        );

                        // Handle the message locally as well (self-message)
                        if let Err(e) = orchestrator
                            .handle_coordination_message(self.info.peer_id, coord_message)
                            .await
                        {
                            tracing::error!("Failed to handle local aux initiation: {:?}", e);
                            return Ok(InitiateAuxGenResponse {
                                success: false,
                                message: format!("Failed to process locally: {}", e),
                                execution_id: Some(execution_id_hex),
                            });
                        }

                        Ok(InitiateAuxGenResponse {
                            success: true,
                            message: format!(
                                "Aux generation initiated with {} participants",
                                participants.len()
                            ),
                            execution_id: Some(execution_id_hex),
                        })
                    }
                    Err(e) => Ok(InitiateAuxGenResponse {
                        success: false,
                        message: format!("Failed to broadcast message: {}", e),
                        execution_id: Some(execution_id_hex),
                    }),
                }
            }
            Err(e) => Ok(InitiateAuxGenResponse {
                success: false,
                message: format!("Failed to serialize coordination message: {}", e),
                execution_id: None,
            }),
        }
    }

    #[tracing::instrument(skip(self), fields(peer_id = %self.info.peer_id, threshold))]
    async fn initiate_keygen(&self, threshold: u16) -> RpcResult<InitiateKeygenResponse> {
        use std::time::SystemTime;

        // Check if orchestrator is initialized
        let mut orchestrator_lock = self.info.mpc_orchestrator.write().await;
        let orchestrator = match orchestrator_lock.as_mut() {
            Some(orch) => orch,
            None => {
                return Ok(InitiateKeygenResponse {
                    success: false,
                    message: "MPC orchestrator not initialized. Please assign peer indices first."
                        .to_string(),
                    execution_id: None,
                    threshold: None,
                    total_parties: None,
                });
            }
        };

        // Get total number of parties
        let total_parties = orchestrator.participants.len() as u16;

        // Validate threshold
        if threshold == 0 {
            return Ok(InitiateKeygenResponse {
                success: false,
                message: "Threshold must be at least 1".to_string(),
                execution_id: None,
                threshold: Some(threshold),
                total_parties: Some(total_parties),
            });
        }

        if threshold > total_parties {
            return Ok(InitiateKeygenResponse {
                success: false,
                message: format!(
                    "Threshold ({}) cannot exceed total parties ({})",
                    threshold, total_parties
                ),
                execution_id: None,
                threshold: Some(threshold),
                total_parties: Some(total_parties),
            });
        }

        // Get orchestrator state
        let state = orchestrator.get_state().await;

        // Verify we're in AuxCompleted state (or allow Idle for testing)
        match state {
            crate::mpc::ProtocolState::AuxCompleted { .. } | crate::mpc::ProtocolState::Idle => {
                // Good to proceed
            }
            _ => {
                return Ok(InitiateKeygenResponse {
                    success: false,
                    message: format!(
                        "Cannot initiate keygen. Must complete aux generation first. Current state: {:?}",
                        state
                    ),
                    execution_id: None,
                    threshold: Some(threshold),
                    total_parties: Some(total_parties),
                });
            }
        }

        // Create execution ID
        let timestamp = SystemTime::now();
        let execution_id = orchestrator.create_execution_id("keygen", timestamp);
        let execution_id_hex = hex::encode(&execution_id);

        // Create InitiateKeygen coordination message
        let coord_message = crate::mpc::CoordinationMessage::InitiateKeygen {
            execution_id: execution_id.clone(),
            threshold,
            total_parties,
            timestamp,
        };

        // Serialize and broadcast via P2P
        match serde_json::to_string(&coord_message) {
            Ok(json) => {
                let data = json.into_bytes();
                match self.info.network_command_sender.send(
                    crate::network::NetworkCommand::Broadcast {
                        topic: crate::network::MPC_COORDINATION_TOPIC.to_string(),
                        data,
                    },
                ) {
                    Ok(_) => {
                        tracing::info!(
                            "📡 Broadcasted InitiateKeygen message (execution_id: {}, t={}, n={})",
                            execution_id_hex,
                            threshold,
                            total_parties
                        );

                        // Handle the message locally as well (self-message)
                        if let Err(e) = orchestrator
                            .handle_coordination_message(self.info.peer_id, coord_message)
                            .await
                        {
                            tracing::error!("Failed to handle local keygen initiation: {:?}", e);
                            return Ok(InitiateKeygenResponse {
                                success: false,
                                message: format!("Failed to process locally: {}", e),
                                execution_id: Some(execution_id_hex),
                                threshold: Some(threshold),
                                total_parties: Some(total_parties),
                            });
                        }

                        Ok(InitiateKeygenResponse {
                            success: true,
                            message: format!(
                                "Keygen initiated with {} participants (threshold: {})",
                                total_parties, threshold
                            ),
                            execution_id: Some(execution_id_hex),
                            threshold: Some(threshold),
                            total_parties: Some(total_parties),
                        })
                    }
                    Err(e) => Ok(InitiateKeygenResponse {
                        success: false,
                        message: format!("Failed to broadcast message: {}", e),
                        execution_id: Some(execution_id_hex),
                        threshold: Some(threshold),
                        total_parties: Some(total_parties),
                    }),
                }
            }
            Err(e) => Ok(InitiateKeygenResponse {
                success: false,
                message: format!("Failed to serialize coordination message: {}", e),
                execution_id: None,
                threshold: Some(threshold),
                total_parties: Some(total_parties),
            }),
        }
    }

    #[tracing::instrument(skip(self, message), fields(peer_id = %self.info.peer_id, message_len = message.len(), signers = ?signers))]
    async fn initiate_signing(
        &self,
        message: String,
        signers: Vec<u16>,
    ) -> RpcResult<InitiateSigningResponse> {
        use std::time::SystemTime;

        // Check if orchestrator is initialized
        let mut orchestrator_lock = self.info.mpc_orchestrator.write().await;
        let orchestrator = match orchestrator_lock.as_mut() {
            Some(orch) => orch,
            None => {
                return Ok(InitiateSigningResponse {
                    success: false,
                    message: "MPC orchestrator not initialized. Please assign peer indices first."
                        .to_string(),
                    execution_id: None,
                    signers: None,
                    signature: None,
                });
            }
        };

        // Validate signers list
        if signers.is_empty() {
            return Ok(InitiateSigningResponse {
                success: false,
                message: "Signers list cannot be empty".to_string(),
                execution_id: None,
                signers: Some(signers),
                signature: None,
            });
        }

        let total_parties = orchestrator.participants.len() as u16;

        // Validate all signers are valid party indices
        for &signer in &signers {
            if signer >= total_parties {
                return Ok(InitiateSigningResponse {
                    success: false,
                    message: format!(
                        "Invalid signer index: {} (total parties: {})",
                        signer, total_parties
                    ),
                    execution_id: None,
                    signers: Some(signers),
                    signature: None,
                });
            }
        }

        // Get orchestrator state
        let state = orchestrator.get_state().await;

        // Verify we're in KeygenCompleted state (or allow Idle for testing)
        match state {
            crate::mpc::ProtocolState::KeygenCompleted { .. } | crate::mpc::ProtocolState::Idle => {
                // Good to proceed
            }
            _ => {
                return Ok(InitiateSigningResponse {
                    success: false,
                    message: format!(
                        "Cannot initiate signing. Must complete keygen first. Current state: {:?}",
                        state
                    ),
                    execution_id: None,
                    signers: Some(signers),
                    signature: None,
                });
            }
        }

        // Create execution ID
        let timestamp = SystemTime::now();
        let execution_id = orchestrator.create_execution_id("signing", timestamp);
        let execution_id_hex = hex::encode(&execution_id);

        // Create InitiateSigning coordination message
        let coord_message = crate::mpc::CoordinationMessage::InitiateSigning {
            execution_id: execution_id.clone(),
            message: message.clone().into_bytes(),
            signers: signers.clone(),
            timestamp,
        };

        // Serialize and broadcast via P2P
        match serde_json::to_string(&coord_message) {
            Ok(json) => {
                let data = json.into_bytes();
                match self.info.network_command_sender.send(
                    crate::network::NetworkCommand::Broadcast {
                        topic: crate::network::MPC_COORDINATION_TOPIC.to_string(),
                        data,
                    },
                ) {
                    Ok(_) => {
                        tracing::info!(
                            "📡 Broadcasted InitiateSigning message (execution_id: {}, signers: {:?}, message: {})",
                            execution_id_hex,
                            signers,
                            message
                        );

                        // Handle the message locally as well (self-message)
                        if let Err(e) = orchestrator
                            .handle_coordination_message(self.info.peer_id, coord_message)
                            .await
                        {
                            tracing::error!("Failed to handle local signing initiation: {:?}", e);
                            return Ok(InitiateSigningResponse {
                                success: false,
                                message: format!("Failed to process locally: {}", e),
                                execution_id: Some(execution_id_hex),
                                signers: Some(signers),
                                signature: None,
                            });
                        }

                        // Wait for the signature to be generated (with timeout)
                        drop(orchestrator_lock); // Release the write lock

                        tracing::info!("⏳ Waiting for signature generation to complete...");
                        let timeout = std::time::Duration::from_secs(60); // 60 second timeout

                        let orch_read = self.info.mpc_orchestrator.read().await;
                        let signature = if let Some(orch) = orch_read.as_ref() {
                            orch.wait_for_signature(timeout).await
                        } else {
                            None
                        };

                        match signature {
                            Some(sig) => {
                                tracing::info!("✅ Signature generated successfully");
                                Ok(InitiateSigningResponse {
                                    success: true,
                                    message: format!(
                                        "Signing completed with {} signers for message: {}",
                                        signers.len(),
                                        message
                                    ),
                                    execution_id: Some(execution_id_hex),
                                    signers: Some(signers),
                                    signature: Some(sig),
                                })
                            }
                            None => {
                                tracing::error!(
                                    "❌ Failed to generate signature (timeout or failure)"
                                );
                                Ok(InitiateSigningResponse {
                                    success: false,
                                    message: "Signing initiated but failed to complete (timeout or error)".to_string(),
                                    execution_id: Some(execution_id_hex),
                                    signers: Some(signers),
                                    signature: None,
                                })
                            }
                        }
                    }
                    Err(e) => Ok(InitiateSigningResponse {
                        success: false,
                        message: format!("Failed to broadcast message: {}", e),
                        execution_id: Some(execution_id_hex),
                        signers: Some(signers),
                        signature: None,
                    }),
                }
            }
            Err(e) => Ok(InitiateSigningResponse {
                success: false,
                message: format!("Failed to serialize coordination message: {}", e),
                execution_id: None,
                signers: Some(signers),
                signature: None,
            }),
        }
    }
}

/// Start the JSON-RPC server
#[tracing::instrument(skip(network_info), fields(port, peer_id = %network_info.peer_id))]
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
