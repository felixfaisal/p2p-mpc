use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};

use cggmp24::KeyShare;
use cggmp24::key_share::AuxInfo;
use cggmp24::security_level::SecurityLevel128;
use cggmp24::supported_curves::Secp256k1;
use round_based::Incoming;

use crate::metrics;
use crate::mpc::AuxParty;
use crate::mpc::{AuxMsg, SecurityLevelTest};

/// Party index in the MPC protocol (0-indexed)
pub type PartyIndex = u16;

/// MPC Protocol Orchestrator
/// Coordinates the execution of Aux Generation, Keygen, and Signing phases
pub struct MPCOrchestrator {
    /// Current protocol state
    pub state: Arc<RwLock<ProtocolState>>,

    /// List of participating peers
    pub participants: Vec<PeerId>,

    /// Mapping from PeerId to PartyIndex (stable assignment)
    pub party_assignments: HashMap<PeerId, PartyIndex>,

    /// Timeout duration for each protocol phase
    #[allow(unused)]
    timeout: Duration,

    /// Local peer ID
    local_peer_id: PeerId,

    /// Channel to send network commands (including MPC messages)
    network_command_tx: mpsc::UnboundedSender<crate::network::NetworkCommand>,

    /// Channel to send incoming MPC aux messages from P2P network to aux party
    /// This will be set when spawning aux generation task
    pub aux_incoming_tx: Option<mpsc::UnboundedSender<Incoming<AuxMsg>>>,

    /// Channel to send incoming MPC keygen messages from P2P network to keygen party
    /// This will be set when spawning keygen task
    pub keygen_incoming_tx: Option<mpsc::UnboundedSender<Incoming<crate::mpc::ThresholdMessage>>>,

    /// Channel to send incoming MPC signing messages from P2P network to signing party
    /// This will be set when spawning signing task
    pub signing_incoming_tx: Option<mpsc::UnboundedSender<Incoming<crate::mpc::SigningMessage>>>,

    /// Storage for generated aux info
    aux_info: Arc<RwLock<Option<AuxInfo<SecurityLevel128>>>>,

    /// Storage for generated key share
    key_share: Arc<RwLock<Option<KeyShare<Secp256k1, SecurityLevelTest>>>>,
}

/// Protocol execution state machine
#[derive(Debug, Clone)]
pub enum ProtocolState {
    /// No protocol running
    Idle,

    /// Aux generation in progress
    #[allow(unused)]
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
    #[allow(unused)]
    AuxCompleted {
        /// Timestamp when completed
        completed_at: Instant,
        /// Number of successful parties
        success_count: usize,
    },

    /// Keygen in progress
    #[allow(unused)]
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
    #[allow(unused)]
    KeygenCompleted {
        /// Timestamp when completed
        completed_at: Instant,
        /// Shared public key (first party's contribution for verification)
        public_key: String,
    },

    /// Signing in progress
    #[allow(unused)]
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
    #[allow(unused)]
    SigningCompleted {
        /// Timestamp when completed
        completed_at: Instant,
        /// The signature (first successful party's result)
        signature: String,
    },

    /// Protocol failed
    #[allow(unused)]
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
        /// Timestamp for synchronization
        timestamp: SystemTime,
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
        /// Timestamp for synchronization
        timestamp: SystemTime,
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
        network_command_tx: mpsc::UnboundedSender<crate::network::NetworkCommand>,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(ProtocolState::Idle)),
            participants,
            party_assignments,
            timeout,
            local_peer_id,
            network_command_tx,
            aux_incoming_tx: None,
            keygen_incoming_tx: None,
            signing_incoming_tx: None,
            aux_info: Arc::new(RwLock::new(None)),
            key_share: Arc::new(RwLock::new(None)),
        }
    }

    /// Get current protocol state
    pub async fn get_state(&self) -> ProtocolState {
        self.state.read().await.clone()
    }

    /// Update protocol state metrics
    fn update_state_metrics(state: &ProtocolState) {
        // Reset all state gauges
        metrics::MPC_PROTOCOL_STATE
            .with_label_values(&["idle"])
            .set(0.0);
        metrics::MPC_PROTOCOL_STATE
            .with_label_values(&["aux_generation"])
            .set(0.0);
        metrics::MPC_PROTOCOL_STATE
            .with_label_values(&["aux_completed"])
            .set(0.0);
        metrics::MPC_PROTOCOL_STATE
            .with_label_values(&["keygen"])
            .set(0.0);
        metrics::MPC_PROTOCOL_STATE
            .with_label_values(&["keygen_completed"])
            .set(0.0);
        metrics::MPC_PROTOCOL_STATE
            .with_label_values(&["signing"])
            .set(0.0);
        metrics::MPC_PROTOCOL_STATE
            .with_label_values(&["signing_completed"])
            .set(0.0);
        metrics::MPC_PROTOCOL_STATE
            .with_label_values(&["failed"])
            .set(0.0);

        // Set current state to 1
        match state {
            ProtocolState::Idle => {
                metrics::MPC_PROTOCOL_STATE
                    .with_label_values(&["idle"])
                    .set(1.0);
            }
            ProtocolState::AuxGeneration { .. } => {
                metrics::MPC_PROTOCOL_STATE
                    .with_label_values(&["aux_generation"])
                    .set(1.0);
            }
            ProtocolState::AuxCompleted { .. } => {
                metrics::MPC_PROTOCOL_STATE
                    .with_label_values(&["aux_completed"])
                    .set(1.0);
            }
            ProtocolState::Keygen { threshold, .. } => {
                metrics::MPC_PROTOCOL_STATE
                    .with_label_values(&["keygen"])
                    .set(1.0);
                metrics::THRESHOLD_VALUE.set(*threshold as f64);
            }
            ProtocolState::KeygenCompleted { .. } => {
                metrics::MPC_PROTOCOL_STATE
                    .with_label_values(&["keygen_completed"])
                    .set(1.0);
            }
            ProtocolState::Signing { signers, .. } => {
                metrics::MPC_PROTOCOL_STATE
                    .with_label_values(&["signing"])
                    .set(1.0);
                metrics::ACTIVE_PARTIES.set(signers.len() as f64);
            }
            ProtocolState::SigningCompleted { .. } => {
                metrics::MPC_PROTOCOL_STATE
                    .with_label_values(&["signing_completed"])
                    .set(1.0);
            }
            ProtocolState::Failed { .. } => {
                metrics::MPC_PROTOCOL_STATE
                    .with_label_values(&["failed"])
                    .set(1.0);
            }
        }
    }

    /// Get local peer ID
    #[allow(unused)]
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Get party index for local peer
    #[allow(unused)]
    pub fn local_party_index(&self) -> Option<PartyIndex> {
        self.party_assignments.get(&self.local_peer_id).copied()
    }

    // ========================================================================
    // COORDINATION MESSAGE HANDLERS
    // ========================================================================

    /// Handle incoming coordination message from another peer
    #[tracing::instrument(skip(self, message), fields(from_peer = %from, message_type = ?message))]
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
    #[tracing::instrument(skip(self, message), fields(from_peer = %from))]
    async fn handle_initiate_aux(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // Step 1: Extract message fields
        let (execution_id, participants, party_assignments, timestamp) = match message {
            CoordinationMessage::InitiateAux {
                execution_id,
                participants,
                party_assignments,
                timestamp,
            } => (execution_id, participants, party_assignments, timestamp),
            _ => {
                return Err("Expected InitiateAux message".to_string());
            }
        };

        // Convert timestamp to u64 (seconds since UNIX_EPOCH)
        let timestamp_secs = timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Invalid timestamp: {}", e))?
            .as_secs();

        tracing::info!(
            "📥 Received InitiateAux from {} (execution_id: {}, timestamp: {}, participants: {})",
            from,
            hex::encode(&execution_id),
            timestamp_secs,
            participants.len()
        );

        // Step 2: Verify we're in Idle state
        let mut state = self.state.write().await;
        match *state {
            ProtocolState::Idle => {
                // Good to proceed
                tracing::debug!("State check passed: Currently in Idle state");
            }
            _ => {
                return Err(format!(
                    "Cannot initiate aux generation. Current state: {:?}",
                    *state
                ));
            }
        }

        // Step 3: Validate party assignments include us and get our index
        let our_party_index = party_assignments.get(&self.local_peer_id).ok_or_else(|| {
            format!(
                "Party assignments do not include our peer ID: {}",
                self.local_peer_id
            )
        })?;

        tracing::info!(
            "✅ Party assignment validated: Our party index is {}",
            our_party_index
        );

        // Step 4: Update state to AuxGeneration
        let start_time = Instant::now();
        *state = ProtocolState::AuxGeneration {
            initiator: from,
            started_at: start_time,
            execution_id: execution_id.clone(),
            completed_parties: HashSet::new(),
        };

        // Update metrics
        Self::update_state_metrics(&*state);
        metrics::AUX_GENERATION_INITIATED.inc();
        metrics::ACTIVE_PARTIES.set(self.participants.len() as f64);

        tracing::info!(
            "🚀 Transitioned to AuxGeneration state (initiator: {}, our_index: {})",
            from,
            our_party_index
        );

        // Setup Aux Party
        let (tx, rx) = mpsc::unbounded_channel();
        self.aux_incoming_tx = Some(tx);
        let mut party = AuxParty::new(our_party_index.to_owned(), rx);
        let outgoing_sender = self.network_command_tx.clone();
        // For now we're hardcoding total parties to be 3
        let n = 3;
        let aux_info_store = self.aux_info.clone();
        let protocol_state = self.state.clone();

        tokio::task::spawn(async move {
            let aux_result = party
                .generate_aux_info(n, timestamp_secs, outgoing_sender)
                .await;

            match aux_result {
                Ok(aux_info) => {
                    let duration = start_time.elapsed().as_secs_f64();
                    tracing::info!("Aux info generation completed in {:.2}s", duration);

                    // Track success metrics
                    metrics::AUX_GENERATION_COMPLETED.inc();
                    metrics::AUX_GENERATION_DURATION.observe(duration);

                    // Store aux info using write lock
                    let mut aux_info_store = aux_info_store.write().await;
                    *aux_info_store = Some(aux_info);

                    // Update state to Aux info completed
                    let mut state = protocol_state.write().await;
                    *state = ProtocolState::AuxCompleted {
                        completed_at: Instant::now(),
                        success_count: 1,
                    };
                    Self::update_state_metrics(&*state);
                }
                Err(e) => {
                    tracing::error!("Aux info generation failed: {:?}", e);
                    metrics::AUX_GENERATION_FAILED.inc();

                    let mut state = protocol_state.write().await;
                    *state = ProtocolState::Failed {
                        phase: "aux_generation".to_string(),
                        reason: format!("{:?}", e),
                        failed_at: Instant::now(),
                    };
                    Self::update_state_metrics(&*state);
                }
            }
        });

        // TODO: Spawn aux generation task (call separate function)
        tracing::info!(
            "Spawned aux generation task for party index {}",
            our_party_index
        );

        Ok(())
    }

    /// Handle AuxCompleted message
    async fn handle_aux_completed(
        &mut self,
        from: PeerId,
        _message: CoordinationMessage,
    ) -> Result<(), String> {
        // TODO: Update completed_parties set
        // TODO: Check if all parties completed
        // TODO: If all completed, transition to AuxCompleted state
        // TODO: Optionally auto-trigger keygen

        tracing::info!("TODO: Handle AuxCompleted from {}", from);
        Ok(())
    }

    /// Handle InitiateKeygen message
    #[tracing::instrument(skip(self, message), fields(from_peer = %from))]
    async fn handle_initiate_keygen(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // Step 1: Extract message fields
        let (execution_id, threshold, total_parties, timestamp) = match message {
            CoordinationMessage::InitiateKeygen {
                execution_id,
                threshold,
                total_parties,
                timestamp,
            } => (execution_id, threshold, total_parties, timestamp),
            _ => {
                return Err("Expected InitiateKeygen message".to_string());
            }
        };

        // Convert timestamp to u64 (seconds since UNIX_EPOCH)
        let timestamp_secs = timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Invalid timestamp: {}", e))?
            .as_secs();

        tracing::info!(
            "📥 Received InitiateKeygen from {} (execution_id: {}, threshold: {}, total_parties: {}, timestamp: {})",
            from,
            hex::encode(&execution_id),
            threshold,
            total_parties,
            timestamp_secs
        );

        // Step 2: Verify we're in AuxCompleted state (or Idle for testing)
        let mut state = self.state.write().await;
        match *state {
            ProtocolState::AuxCompleted { .. } | ProtocolState::Idle => {
                // Good to proceed
                tracing::debug!("State check passed: Ready for keygen");
            }
            _ => {
                return Err(format!(
                    "Cannot initiate keygen. Must complete aux generation first. Current state: {:?}",
                    *state
                ));
            }
        }

        // Step 3: Validate threshold
        if threshold == 0 {
            return Err("Threshold must be at least 1".to_string());
        }

        if threshold > total_parties {
            return Err(format!(
                "Threshold ({}) cannot exceed total parties ({})",
                threshold, total_parties
            ));
        }

        // Step 4: Validate our party index exists
        let our_party_index = self
            .party_assignments
            .get(&self.local_peer_id)
            .ok_or_else(|| {
                format!(
                    "Party assignments do not include our peer ID: {}",
                    self.local_peer_id
                )
            })?;

        tracing::info!(
            "✅ Keygen parameters validated: threshold={}, total_parties={}, our_index={}",
            threshold,
            total_parties,
            our_party_index
        );

        // Step 5: Update state to Keygen
        let start_time = Instant::now();
        *state = ProtocolState::Keygen {
            initiator: from,
            started_at: start_time,
            execution_id: execution_id.clone(),
            threshold,
            completed_parties: std::collections::HashSet::new(),
        };

        // Update metrics
        Self::update_state_metrics(&*state);
        metrics::KEYGEN_INITIATED.inc();
        metrics::ACTIVE_PARTIES.set(total_parties as f64);
        metrics::THRESHOLD_VALUE.set(threshold as f64);

        tracing::info!(
            "🚀 Transitioned to Keygen state (initiator: {}, threshold: {}/{}, our_index: {})",
            from,
            threshold,
            total_parties,
            our_party_index
        );

        // Setup keygen party
        let aux_info_lock = self.aux_info.read().await;
        let aux_info = aux_info_lock.as_ref().ok_or_else(|| {
            "Aux info not available. Must complete aux generation first.".to_string()
        })?;

        let (tx, rx) = mpsc::unbounded_channel();
        self.keygen_incoming_tx = Some(tx);

        let mut party = crate::mpc::KeygenParty::new(*our_party_index, rx, aux_info.clone());
        let outgoing_sender = self.network_command_tx.clone();
        let protocol_state = self.state.clone();
        let key_share_store = self.key_share.clone();

        // Spawn keygen task
        tokio::spawn(async move {
            match party
                .generate_key_share(total_parties, threshold, timestamp_secs, outgoing_sender)
                .await
            {
                Ok(key_share) => {
                    let duration = start_time.elapsed().as_secs_f64();
                    tracing::info!(
                        "Keygen completed in {:.2}s. Shared public key: {}",
                        duration,
                        hex::encode(key_share.shared_public_key.to_bytes(true))
                    );

                    // Track success metrics
                    metrics::KEYGEN_COMPLETED.inc();
                    metrics::KEYGEN_DURATION.observe(duration);

                    let mut state = protocol_state.write().await;
                    *state = ProtocolState::KeygenCompleted {
                        completed_at: Instant::now(),
                        public_key: hex::encode(key_share.shared_public_key.to_bytes(true)),
                    };
                    Self::update_state_metrics(&*state);
                    drop(state);

                    *key_share_store.write().await = Some(key_share);
                    // TODO: Broadcast KeygenCompleted message
                }
                Err(e) => {
                    tracing::error!("Keygen failed: {:?}", e);
                    metrics::KEYGEN_FAILED.inc();

                    let mut state = protocol_state.write().await;
                    *state = ProtocolState::Failed {
                        phase: "keygen".to_string(),
                        reason: format!("{:?}", e),
                        failed_at: Instant::now(),
                    };
                    Self::update_state_metrics(&*state);
                    // TODO: Broadcast ProtocolFailed message
                }
            }
        });

        Ok(())
    }

    /// Handle KeygenCompleted message
    async fn handle_keygen_completed(
        &mut self,
        from: PeerId,
        _message: CoordinationMessage,
    ) -> Result<(), String> {
        // TODO: Update completed_parties set
        // TODO: Verify public keys match (all parties should generate same shared public key)
        // TODO: If all completed, transition to KeygenCompleted state

        tracing::info!("TODO: Handle KeygenCompleted from {}", from);
        Ok(())
    }

    /// Handle InitiateSigning message
    #[tracing::instrument(skip(self, message), fields(from_peer = %from))]
    async fn handle_initiate_signing(
        &mut self,
        from: PeerId,
        message: CoordinationMessage,
    ) -> Result<(), String> {
        // Step 1: Extract message fields
        let (execution_id, message_to_sign, signers, timestamp) = match message {
            CoordinationMessage::InitiateSigning {
                execution_id,
                message,
                signers,
                timestamp,
            } => (execution_id, message, signers, timestamp),
            _ => {
                return Err("Expected InitiateSigning message".to_string());
            }
        };

        // Convert timestamp to u64 (seconds since UNIX_EPOCH)
        let timestamp_secs = timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Invalid timestamp: {}", e))?
            .as_secs();

        tracing::info!(
            "📥 Received InitiateSigning from {} (execution_id: {}, signers: {:?}, timestamp: {}, message: {})",
            from,
            hex::encode(&execution_id),
            signers,
            timestamp_secs,
            String::from_utf8_lossy(&message_to_sign)
        );

        // Step 2: Verify we're in KeygenCompleted state (or allow Idle for testing)
        let mut state = self.state.write().await;
        match *state {
            ProtocolState::KeygenCompleted { .. } | ProtocolState::Idle => {
                // Good to proceed
                tracing::debug!("State check passed: Ready for signing");
            }
            _ => {
                return Err(format!(
                    "Cannot initiate signing. Must complete keygen first. Current state: {:?}",
                    *state
                ));
            }
        }

        // Step 3: Check if we're in the signers list
        let our_party_idx = self.party_assignments.get(&self.local_peer_id).copied();
        let our_party_idx = our_party_idx.ok_or_else(|| {
            format!(
                "Our peer ID not found in party assignments: {}",
                self.local_peer_id
            )
        })?;

        let we_are_signer = signers.contains(&our_party_idx);

        tracing::info!(
            "✅ Signing request validated: we_are_signer={}, our_index={}, signers={:?}",
            we_are_signer,
            our_party_idx,
            signers
        );

        // Step 4: Update state to Signing
        let start_time = Instant::now();
        *state = ProtocolState::Signing {
            initiator: from,
            started_at: start_time,
            execution_id: execution_id.clone(),
            message: message_to_sign.clone(),
            signers: signers.iter().copied().collect(),
            completed_parties: std::collections::HashSet::new(),
        };

        // Update metrics
        Self::update_state_metrics(&*state);
        metrics::SIGNING_INITIATED.inc();
        metrics::ACTIVE_PARTIES.set(signers.len() as f64);

        tracing::info!(
            "🚀 Transitioned to Signing state (initiator: {}, signers: {:?})",
            from,
            signers
        );

        // Step 5: If we're a signer, we'll spawn the signing task
        // For now, just log that we should do it
        if we_are_signer {
            tracing::info!("📝 We are a signer - TODO: spawn signing task");
            // Setup signer party
            let aux_info_lock = self.aux_info.read().await;
            let aux_info = aux_info_lock.as_ref().ok_or_else(|| {
                "Aux info not available. Must complete aux generation first.".to_string()
            })?;

            let key_share_lock = self.key_share.read().await;
            let key_share = key_share_lock.as_ref().ok_or_else(|| {
                "Key share info not available. Must complete key generation first.".to_string()
            })?;

            let (tx, rx) = mpsc::unbounded_channel();
            self.signing_incoming_tx = Some(tx);

            let mut party = crate::mpc::SigningParty::new(
                our_party_idx,
                rx,
                key_share.clone(),
                aux_info.clone(),
            );
            let outgoing_sender = self.network_command_tx.clone();
            let protocol_state = self.state.clone();

            // Spawn signing task
            tokio::spawn(async move {
                match party
                    .sign_message(
                        signers.as_ref(),
                        message_to_sign.as_ref(),
                        timestamp_secs,
                        outgoing_sender,
                    )
                    .await
                {
                    Ok(signature) => {
                        let duration = start_time.elapsed().as_secs_f64();
                        tracing::info!("Signing completed in {:.2}s: {:?}", duration, signature);

                        // Track success metrics
                        metrics::SIGNING_COMPLETED.inc();
                        metrics::SIGNING_DURATION.observe(duration);

                        let mut state = protocol_state.write().await;
                        *state = ProtocolState::SigningCompleted {
                            completed_at: Instant::now(),
                            signature: format!("{:?}", signature),
                        };
                        Self::update_state_metrics(&*state);
                        // TODO: Broadcast SigningCompleted message
                    }
                    Err(e) => {
                        tracing::error!("Signing failed: {:?}", e);
                        metrics::SIGNING_FAILED.inc();

                        let mut state = protocol_state.write().await;
                        *state = ProtocolState::Failed {
                            phase: "signing".to_string(),
                            reason: format!("{:?}", e),
                            failed_at: Instant::now(),
                        };
                        Self::update_state_metrics(&*state);
                        // TODO: Broadcast ProtocolFailed message
                    }
                }
            });
        } else {
            tracing::info!("👁️  We are NOT a signer - observing only");
        }

        Ok(())
    }

    /// Handle SigningCompleted message
    async fn handle_signing_completed(
        &mut self,
        from: PeerId,
        _message: CoordinationMessage,
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
        _message: CoordinationMessage,
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
    #[allow(unused)]
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
    #[allow(unused)]
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
    #[allow(unused)]
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
    #[allow(unused)]
    async fn broadcast(&self, message: CoordinationMessage) -> Result<(), String> {
        // TODO: Serialize message
        // TODO: Send via P2P network to all participants
        // This will be filled in when integrating with network layer

        tracing::info!("TODO: Broadcast {:?}", message);
        Ok(())
    }

    /// Abort current protocol with reason
    #[allow(unused)]
    pub async fn abort_protocol(&mut self, reason: &str) -> Result<(), String> {
        // TODO: Get current phase
        // TODO: Update state to Failed
        // TODO: Broadcast ProtocolFailed message
        // TODO: Clean up any running tasks

        tracing::error!("Aborting protocol: {}", reason);

        let current_phase = match &*self.state.read().await {
            ProtocolState::AuxGeneration { .. } => "aux_generation",
            ProtocolState::Keygen { .. } => "keygen",
            ProtocolState::Signing { .. } => "signing",
            _ => "unknown",
        };

        *self.state.write().await = ProtocolState::Failed {
            phase: current_phase.to_string(),
            reason: reason.to_string(),
            failed_at: Instant::now(),
        };

        Ok(())
    }

    /// Monitor for timeouts (should be spawned as background task)
    #[allow(unused)]
    pub async fn monitor_timeouts(&self) {
        // TODO: Loop forever
        // TODO: Check current state
        // TODO: If in active protocol phase, check elapsed time
        // TODO: If timeout exceeded, call abort_protocol

        tracing::info!("TODO: Start timeout monitor");
    }
}
