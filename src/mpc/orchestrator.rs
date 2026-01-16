use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use cggmp24::key_share::AuxInfo;
use cggmp24::security_level::{KeygenSecurityLevel, SecurityLevel128};
use cggmp24::{ExecutionId, KeyRefreshError, KeyShare};

/// Party index in the MPC protocol (0-indexed)
pub type PartyIndex = u16;

/// MPC Protocol Orchestrator
/// Coordinates the execution of Aux Generation, Keygen, and Signing phases
pub struct MPCOrchestrator {
    /// Current protocol state
    state: Arc<Mutex<ProtocolState>>,

    /// List of participating peers
    pub participants: Vec<PeerId>,

    /// Mapping from PeerId to PartyIndex (stable assignment)
    pub party_assignments: HashMap<PeerId, PartyIndex>,

    /// Timeout duration for each protocol phase
    timeout: Duration,

    /// Local peer ID
    local_peer_id: PeerId,
}

/// Protocol execution state machine
#[derive(Debug, Clone)]
pub enum ProtocolState {
    /// No protocol running
    Idle,

    /// Aux generation in progress
    AuxGeneration {
        /// Peer that initiated the protocol
        initiator: PeerId,
        /// When the protocol started
        started_at: Instant,
        /// ExecutionId for this run
        execution_id: Vec<u8>,
        /// Parties that have completed
        completed_parties: HashSet<PeerId>,
    },

    /// Aux generation completed
    AuxCompleted {
        /// Timestamp when completed
        completed_at: Instant,
        /// Number of successful parties
        success_count: usize,
    },

    /// Keygen in progress
    Keygen {
        /// Peer that initiated
        initiator: PeerId,
        /// When started
        started_at: Instant,
        /// ExecutionId for this run
        execution_id: Vec<u8>,
        /// Threshold parameter
        threshold: u16,
        /// Parties that have completed
        completed_parties: HashSet<PeerId>,
    },

    /// Keygen completed
    KeygenCompleted {
        /// Timestamp when completed
        completed_at: Instant,
        /// Shared public key (first party's contribution for verification)
        public_key: Vec<u8>,
    },

    /// Signing in progress
    Signing {
        /// Peer that initiated
        initiator: PeerId,
        /// When started
        started_at: Instant,
        /// ExecutionId for this run
        execution_id: Vec<u8>,
        /// Message to sign
        message: Vec<u8>,
        /// Subset of parties participating in signing
        signers: Vec<PartyIndex>,
        /// Parties that have completed
        completed_parties: HashSet<PeerId>,
    },

    /// Signing completed
    SigningCompleted {
        /// Timestamp when completed
        completed_at: Instant,
        /// The signature (first successful party's result)
        signature: Vec<u8>,
    },

    /// Protocol failed
    Failed {
        /// Which phase failed
        phase: String,
        /// Error reason
        reason: String,
        /// When it failed
        failed_at: Instant,
    },
}

/// Messages for coordinating protocol execution between peers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinationMessage {
    /// Initiate aux generation phase
    InitiateAux {
        /// Deterministic execution ID
        execution_id: Vec<u8>,
        /// All participating peers
        participants: Vec<PeerId>,
        /// Party index assignments
        party_assignments: HashMap<PeerId, PartyIndex>,
        /// Timestamp for synchronization
        timestamp: SystemTime,
    },

    /// Report aux generation completion
    AuxCompleted {
        /// Reporting party
        party_id: PeerId,
        /// Success or failure
        success: bool,
    },

    /// Initiate keygen phase
    InitiateKeygen {
        /// Deterministic execution ID
        execution_id: Vec<u8>,
        /// Threshold (t in t-of-n)
        threshold: u16,
        /// Total parties (n in t-of-n)
        total_parties: u16,
    },

    /// Report keygen completion
    KeygenCompleted {
        /// Reporting party
        party_id: PeerId,
        /// Shared public key (for verification)
        public_key: Vec<u8>,
        /// Success or failure
        success: bool,
    },

    /// Initiate signing phase
    InitiateSigning {
        /// Deterministic execution ID
        execution_id: Vec<u8>,
        /// Message to sign
        message: Vec<u8>,
        /// Subset of parties that will sign
        signers: Vec<PartyIndex>,
    },

    /// Report signing completion
    SigningCompleted {
        /// Reporting party
        party_id: PeerId,
        /// The signature
        signature: Vec<u8>,
        /// Success or failure
        success: bool,
    },

    /// Report protocol failure
    ProtocolFailed {
        /// Reporting party
        party_id: PeerId,
        /// Which phase failed
        phase: String,
        /// Error description
        error: String,
    },
}

impl MPCOrchestrator {
    /// Create a new MPC orchestrator
    pub fn new(
        local_peer_id: PeerId,
        participants: Vec<PeerId>,
        party_assignments: HashMap<PeerId, PartyIndex>,
        timeout: Duration,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(ProtocolState::Idle)),
            participants,
            party_assignments,
            timeout,
            local_peer_id,
        }
    }

    /// Get current protocol state
    pub async fn get_state(&self) -> ProtocolState {
        self.state.lock().await.clone()
    }

    /// Get local peer ID
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Get party index for local peer
    pub fn local_party_index(&self) -> Option<PartyIndex> {
        self.party_assignments.get(&self.local_peer_id).copied()
    }

    // ========================================================================
    // COORDINATION MESSAGE HANDLERS
    // ========================================================================

    /// Handle incoming coordination message from another peer
    pub async fn handle_coordination_message(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        tracing::info!("Handling coordination message from {}: {:?}", from, message);

        match message {
            CoordinationMessage::InitiateAux { .. } => {
                self.handle_initiate_aux(from, message).await
            }
            CoordinationMessage::AuxCompleted { .. } => {
                self.handle_aux_completed(from, message).await
            }
            CoordinationMessage::InitiateKeygen { .. } => {
                self.handle_initiate_keygen(from, message).await
            }
            CoordinationMessage::KeygenCompleted { .. } => {
                self.handle_keygen_completed(from, message).await
            }
            CoordinationMessage::InitiateSigning { .. } => {
                self.handle_initiate_signing(from, message).await
            }
            CoordinationMessage::SigningCompleted { .. } => {
                self.handle_signing_completed(from, message).await
            }
            CoordinationMessage::ProtocolFailed { .. } => {
                self.handle_protocol_failed(from, message).await
            }
        }
    }

    /// Handle InitiateAux message
    async fn handle_initiate_aux(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // TODO: Extract message fields
        // TODO: Verify we're in Idle state
        // TODO: Validate party assignments include us
        // TODO: Update state to AuxGeneration
        // TODO: Spawn aux generation task (call separate function)

        tracing::warn!("TODO: Handle InitiateAux from {}", from);
        Ok(())
    }

    /// Handle AuxCompleted message
    async fn handle_aux_completed(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // TODO: Update completed_parties set
        // TODO: Check if all parties completed
        // TODO: If all completed, transition to AuxCompleted state
        // TODO: Optionally auto-trigger keygen

        tracing::info!("TODO: Handle AuxCompleted from {}", from);
        Ok(())
    }

    /// Handle InitiateKeygen message
    async fn handle_initiate_keygen(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // TODO: Extract message fields
        // TODO: Verify we're in AuxCompleted state
        // TODO: Update state to Keygen
        // TODO: Spawn keygen task

        tracing::info!("TODO: Handle InitiateKeygen from {}", from);
        Ok(())
    }

    /// Handle KeygenCompleted message
    async fn handle_keygen_completed(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // TODO: Update completed_parties set
        // TODO: Verify public keys match (all parties should generate same shared public key)
        // TODO: If all completed, transition to KeygenCompleted state

        tracing::info!("TODO: Handle KeygenCompleted from {}", from);
        Ok(())
    }

    /// Handle InitiateSigning message
    async fn handle_initiate_signing(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // TODO: Extract message fields
        // TODO: Verify we're in KeygenCompleted state
        // TODO: Check if we're in the signers list
        // TODO: Update state to Signing
        // TODO: Spawn signing task (if we're a signer)

        tracing::info!("TODO: Handle InitiateSigning from {}", from);
        Ok(())
    }

    /// Handle SigningCompleted message
    async fn handle_signing_completed(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // TODO: Update completed_parties set
        // TODO: Verify signatures match (all signers should produce same signature)
        // TODO: If all completed, transition to SigningCompleted state

        tracing::info!("TODO: Handle SigningCompleted from {}", from);
        Ok(())
    }

    /// Handle ProtocolFailed message
    async fn handle_protocol_failed(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // TODO: Log the failure
        // TODO: Optionally abort local protocol
        // TODO: Update state to Failed

        tracing::info!("TODO: Handle ProtocolFailed from {}", from);
        Ok(())
    }

    // ========================================================================
    // PROTOCOL INITIATION (RPC-triggered)
    // ========================================================================

    /// Initiate aux generation phase (called via RPC)
    pub async fn initiate_aux_generation(&mut self) -> Result<(), String> {
        // TODO: Check if in Idle state
        // TODO: Generate deterministic ExecutionId
        // TODO: Create InitiateAux message
        // TODO: Broadcast to all participants
        // TODO: Update local state
        // TODO: Start local aux generation

        tracing::info!("TODO: Initiate aux generation");
        Ok(())
    }

    /// Initiate keygen phase (called via RPC)
    pub async fn initiate_keygen(&mut self, threshold: u16) -> Result<(), String> {
        // TODO: Check if in AuxCompleted state
        // TODO: Generate deterministic ExecutionId
        // TODO: Create InitiateKeygen message
        // TODO: Broadcast to all participants
        // TODO: Update local state
        // TODO: Start local keygen

        tracing::info!("TODO: Initiate keygen with threshold={}", threshold);
        Ok(())
    }

    /// Initiate signing phase (called via RPC)
    pub async fn initiate_signing(
        &mut self,
        message: Vec<u8>,
        signers: Vec<PartyIndex>,
    ) -> Result<(), String> {
        // TODO: Check if in KeygenCompleted state
        // TODO: Validate signers list (indices exist, >= threshold)
        // TODO: Generate deterministic ExecutionId
        // TODO: Create InitiateSigning message
        // TODO: Broadcast to all participants
        // TODO: Update local state
        // TODO: Start local signing (if we're a signer)

        tracing::info!("TODO: Initiate signing with {} signers", signers.len());
        Ok(())
    }

    // ========================================================================
    // HELPER METHODS
    // ========================================================================

    /// Create a deterministic execution ID from phase and timestamp
    pub fn create_execution_id(&self, phase: &str, timestamp: SystemTime) -> Vec<u8> {
        // TODO: Hash(phase + timestamp + participants)
        // For now, just use phase name
        format!("{}-{:?}", phase, timestamp).into_bytes()
    }

    /// Broadcast coordination message to all participants
    async fn broadcast(&self, message: CoordinationMessage) -> Result<(), String> {
        // TODO: Serialize message
        // TODO: Send via P2P network to all participants
        // This will be filled in when integrating with network layer

        tracing::info!("TODO: Broadcast {:?}", message);
        Ok(())
    }

    /// Abort current protocol with reason
    pub async fn abort_protocol(&mut self, reason: &str) -> Result<(), String> {
        // TODO: Get current phase
        // TODO: Update state to Failed
        // TODO: Broadcast ProtocolFailed message
        // TODO: Clean up any running tasks

        tracing::error!("Aborting protocol: {}", reason);

        let current_phase = match &*self.state.lock().await {
            ProtocolState::AuxGeneration { .. } => "aux_generation",
            ProtocolState::Keygen { .. } => "keygen",
            ProtocolState::Signing { .. } => "signing",
            _ => "unknown",
        };

        *self.state.lock().await = ProtocolState::Failed {
            phase: current_phase.to_string(),
            reason: reason.to_string(),
            failed_at: Instant::now(),
        };

        Ok(())
    }

    /// Monitor for timeouts (should be spawned as background task)
    pub async fn monitor_timeouts(&self) {
        // TODO: Loop forever
        // TODO: Check current state
        // TODO: If in active protocol phase, check elapsed time
        // TODO: If timeout exceeded, call abort_protocol

        tracing::info!("TODO: Start timeout monitor");
    }
}
