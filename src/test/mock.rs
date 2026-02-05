use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use cggmp24::PregeneratedPrimes;
use futures::{Sink, Stream};
use round_based::{Incoming, MessageDestination, Outgoing};
use tokio::sync::{Mutex, mpsc};

use crate::mpc::{AuxMsg, PartyId, SecurityLevelTest, SigningMessage, ThresholdMessage};

/// Get the path to test fixtures for Paillier primes
pub fn get_fixtures_primes_path(party_id: PartyId) -> PathBuf {
    let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    path.push("fixtures");
    path.push("paillier_primes");
    path.push(format!("paillier_primes_party_{}.bin", party_id));
    path
}

/// Load Paillier primes from fixtures for testing
pub fn load_primes_from_fixtures(
    party_id: PartyId,
) -> Option<PregeneratedPrimes<SecurityLevelTest>> {
    let primes_path = get_fixtures_primes_path(party_id);
    if !primes_path.exists() {
        tracing::warn!("Fixtures primes not found at {:?}", primes_path);
        return None;
    }

    match std::fs::read(&primes_path) {
        Ok(data) => match bincode::deserialize(&data) {
            Ok(primes) => {
                tracing::info!("✓ Loaded Paillier primes from fixtures: {:?}", primes_path);
                Some(primes)
            }
            Err(e) => {
                tracing::warn!("Failed to deserialize primes from {:?}: {}", primes_path, e);
                None
            }
        },
        Err(e) => {
            tracing::warn!("Failed to read primes from {:?}: {}", primes_path, e);
            None
        }
    }
}

/// Mock network coordinator for Aux generation protocol
#[derive(Clone)]
pub struct NetworkCoordinator {
    channels: Arc<Mutex<HashMap<PartyId, mpsc::UnboundedSender<Incoming<AuxMsg>>>>>,
}

impl NetworkCoordinator {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register_party(
        &self,
        party_id: PartyId,
        sender: mpsc::UnboundedSender<Incoming<AuxMsg>>,
    ) {
        let mut channels = self.channels.lock().await;
        channels.insert(party_id, sender);
        tracing::info!("🔗 Party {} registered with network coordinator", party_id);
    }

    pub async fn broadcast(&self, from: PartyId, message: Incoming<AuxMsg>) {
        let channels = self.channels.lock().await;

        for (&party_id, sender) in channels.iter() {
            if party_id != from {
                if let Err(e) = sender.send(message.clone()) {
                    tracing::info!("Failed to send broadcast to party {}: {}", party_id, e);
                }
            }
        }
    }

    /// Send a message to a specific party
    pub async fn send_to_party(
        &self,
        to: PartyId,
        message: Incoming<AuxMsg>,
    ) -> Result<(), String> {
        let channels = self.channels.lock().await;

        if let Some(sender) = channels.get(&to) {
            sender
                .send(message)
                .map_err(|e| format!("Failed to send to party {}: {}", to, e))
        } else {
            Err(format!("Party {} not registered with coordinator", to))
        }
    }

    /// Create MPC channels for a party
    pub fn create_mpc_channels(
        &self,
        party_id: PartyId,
        message_rx: mpsc::UnboundedReceiver<Incoming<AuxMsg>>,
    ) -> (
        Pin<Box<dyn Stream<Item = Result<Incoming<AuxMsg>, std::io::Error>> + Send>>,
        Pin<Box<dyn Sink<Outgoing<AuxMsg>, Error = std::io::Error> + Send>>,
    ) {
        let coordinator = self.clone();

        // Incoming stream: Convert mpsc receiver to a Stream
        let incoming_stream = Box::pin(futures::stream::unfold(message_rx, |mut rx| async move {
            rx.recv().await.map(|msg| (Ok(msg), rx))
        }));

        // Outgoing sink: Convert mpsc sender to a Sink
        let outgoing_sink = Box::pin(futures::sink::unfold(
            coordinator,
            move |coordinator: NetworkCoordinator, outgoing_msg: Outgoing<AuxMsg>| async move {
                match outgoing_msg.recipient {
                    MessageDestination::AllParties => {
                        let network_msg = Incoming {
                            id: 0,
                            sender: party_id,
                            msg_type: round_based::MessageType::Broadcast,
                            msg: outgoing_msg.msg,
                        };
                        coordinator.broadcast(party_id, network_msg).await;
                    }
                    MessageDestination::OneParty(id) => {
                        let network_msg = Incoming {
                            id: 0,
                            sender: party_id,
                            msg_type: round_based::MessageType::P2P,
                            msg: outgoing_msg.msg,
                        };
                        coordinator.send_to_party(id, network_msg).await.unwrap();
                    }
                }

                Ok::<_, std::io::Error>(coordinator)
            },
        ));

        (incoming_stream, outgoing_sink)
    }
}

// ============================================================================
// Test Party Implementations
// ============================================================================

use cggmp24::key_share::AuxInfo;
use cggmp24::supported_curves::Secp256k1;
use cggmp24::{ExecutionId, KeyRefreshError, KeyShare};
use rand::rngs::OsRng;
use round_based::MpcParty;

/// Test AuxParty for generating auxiliary information
pub struct AuxParty {
    pub id: PartyId,
    coordinator: NetworkCoordinator,
    message_rx: Option<mpsc::UnboundedReceiver<Incoming<AuxMsg>>>,
}

impl AuxParty {
    pub async fn new(id: PartyId, coordinator: NetworkCoordinator) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        coordinator.register_party(id, tx).await;

        Self {
            id,
            coordinator,
            message_rx: Some(rx),
        }
    }

    /// Create execution ID that all parties will agree on
    fn create_shared_execution_id(protocol: &str, n: u16, timestamp: u64) -> ExecutionId<'static> {
        let mut id_data = Vec::new();
        id_data.extend_from_slice(protocol.as_bytes());
        id_data.extend_from_slice(&n.to_be_bytes());
        id_data.extend_from_slice(&timestamp.to_be_bytes());

        ExecutionId::new(Box::leak(id_data.into_boxed_slice()))
    }

    pub async fn generate_aux_info(
        &mut self,
        n: u16,
        timestamp: u64,
    ) -> Result<AuxInfo<SecurityLevelTest>, KeyRefreshError> {
        tracing::info!("🔧 Party {} starting aux info generation", self.id);
        let mut rng = OsRng;
        let eid = Self::create_shared_execution_id("aux_gen", n, timestamp);

        // Load primes from fixtures (for fast tests)
        let primes = load_primes_from_fixtures(self.id).expect(&format!(
            "Failed to load Paillier primes from fixtures for party {}",
            self.id
        ));
        tracing::info!("✓ Party {} loaded Paillier primes from fixtures", self.id);

        // Setup MPC party with channels
        let message_rx = self.message_rx.take().expect("message_rx already taken");
        let (incoming, outgoing) = self.coordinator.create_mpc_channels(self.id, message_rx);
        let mpc_party = MpcParty::connected((incoming, outgoing));

        // Run aux info generation protocol
        tracing::info!(
            "🔄 Party {} running aux info generation protocol...",
            self.id
        );
        let aux_info = cggmp24::aux_info_gen(eid, self.id, n, primes)
            .start(&mut rng, mpc_party)
            .await?;

        tracing::info!("✅ Party {} completed aux info generation", self.id);
        Ok(aux_info)
    }
}

/// Test KeygenParty for generating key shares
pub struct KeygenParty {
    pub id: PartyId,
    coordinator: KeygenNetworkCoordinator,
    message_rx: Option<mpsc::UnboundedReceiver<Incoming<ThresholdMessage>>>,
    aux_info: AuxInfo<SecurityLevelTest>,
}

impl KeygenParty {
    pub async fn new(
        id: PartyId,
        coordinator: KeygenNetworkCoordinator,
        aux_info: AuxInfo<SecurityLevelTest>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        coordinator.register_party(id, tx).await;

        Self {
            id,
            coordinator,
            message_rx: Some(rx),
            aux_info,
        }
    }

    /// Create execution ID that all parties will agree on
    fn create_shared_execution_id(protocol: &str, n: u16, timestamp: u64) -> ExecutionId<'static> {
        let mut id_data = Vec::new();
        id_data.extend_from_slice(protocol.as_bytes());
        id_data.extend_from_slice(&n.to_be_bytes());
        id_data.extend_from_slice(&timestamp.to_be_bytes());

        ExecutionId::new(Box::leak(id_data.into_boxed_slice()))
    }

    pub async fn generate_key_share(
        &mut self,
        n: u16,
        t: u16,
        timestamp: u64,
    ) -> Result<KeyShare<Secp256k1, SecurityLevelTest>, Box<dyn std::error::Error + Send + Sync>>
    {
        tracing::info!(
            "🔑 Party {} starting key generation (t={}, n={})",
            self.id,
            t,
            n
        );

        let mut rng = OsRng;
        let eid = Self::create_shared_execution_id("keygen", n, timestamp);

        // Setup MPC party with channels
        let message_rx = self.message_rx.take().expect("message_rx already taken");
        let (incoming, outgoing) = self.coordinator.create_mpc_channels(self.id, message_rx);
        let mpc_party = MpcParty::connected((incoming, outgoing));

        tracing::info!("🔄 Party {} running keygen protocol...", self.id);
        let incomplete_key_share = cggmp24::keygen::<Secp256k1>(eid, self.id, n)
            .set_security_level::<SecurityLevelTest>()
            .set_threshold(t)
            .start(&mut rng, mpc_party)
            .await?;

        let key_share = KeyShare::from_parts((incomplete_key_share, self.aux_info.clone()))?;

        tracing::info!("✅ Party {} completed key generation", self.id);
        tracing::info!(
            "   Shared public key: {}",
            hex::encode(key_share.shared_public_key.to_bytes(true))
        );

        Ok(key_share)
    }
}

/// Test SigningParty for generating signatures
pub struct SigningParty {
    pub id: PartyId,
    coordinator: SigningNetworkCoordinator,
    message_rx: Option<mpsc::UnboundedReceiver<Incoming<SigningMessage>>>,
    key_share: KeyShare<Secp256k1, SecurityLevelTest>,
}

impl SigningParty {
    pub async fn new(
        id: PartyId,
        coordinator: SigningNetworkCoordinator,
        key_share: KeyShare<Secp256k1, SecurityLevelTest>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        coordinator.register_party(id, tx).await;

        Self {
            id,
            coordinator,
            message_rx: Some(rx),
            key_share,
        }
    }

    /// Create execution ID that all parties will agree on
    fn create_shared_execution_id(protocol: &str, n: u16, timestamp: u64) -> ExecutionId<'static> {
        let mut id_data = Vec::new();
        id_data.extend_from_slice(protocol.as_bytes());
        id_data.extend_from_slice(&n.to_be_bytes());
        id_data.extend_from_slice(&timestamp.to_be_bytes());

        ExecutionId::new(Box::leak(id_data.into_boxed_slice()))
    }

    pub async fn sign_message(
        &mut self,
        signers: &[u16],
        message: &[u8],
        timestamp: u64,
    ) -> Result<cggmp24::Signature<Secp256k1>, Box<dyn std::error::Error + Send + Sync>> {
        use sha2::Sha256;

        tracing::info!("✍️  Party {} starting signing process", self.id);
        tracing::info!("   Message: {:?}", String::from_utf8_lossy(message));

        let mut rng = OsRng;
        let eid = Self::create_shared_execution_id("signing", signers.len() as u16, timestamp);
        let data_to_sign = cggmp24::DataToSign::<Secp256k1>::digest::<Sha256>(message);

        // Setup MPC party with channels
        let message_rx = self.message_rx.take().expect("message_rx already taken");
        let (incoming, outgoing) = self.coordinator.create_mpc_channels(self.id, message_rx);
        let mpc_party = MpcParty::connected((incoming, outgoing));

        tracing::info!("🔄 Party {} running signing protocol...", self.id);
        let signature = cggmp24::signing::<Secp256k1, SecurityLevelTest>(
            eid,
            self.id,
            signers,
            &self.key_share,
        )
        .sign(&mut rng, mpc_party, &data_to_sign)
        .await?;

        tracing::info!("✅ Party {} completed signing", self.id);

        Ok(signature)
    }
}

/// Mock network coordinator for Keygen protocol
#[derive(Clone)]
pub struct KeygenNetworkCoordinator {
    channels: Arc<Mutex<HashMap<PartyId, mpsc::UnboundedSender<Incoming<ThresholdMessage>>>>>,
}

impl KeygenNetworkCoordinator {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register_party(
        &self,
        party_id: PartyId,
        sender: mpsc::UnboundedSender<Incoming<ThresholdMessage>>,
    ) {
        let mut channels = self.channels.lock().await;
        channels.insert(party_id, sender);
        tracing::info!("🔗 Party {} registered with network coordinator", party_id);
    }

    pub async fn broadcast(&self, from: PartyId, message: Incoming<ThresholdMessage>) {
        let channels = self.channels.lock().await;

        for (&party_id, sender) in channels.iter() {
            if party_id != from {
                if let Err(e) = sender.send(message.clone()) {
                    tracing::info!("Failed to send broadcast to party {}: {}", party_id, e);
                }
            }
        }
    }

    /// Send a message to a specific party
    pub async fn send_to_party(
        &self,
        to: PartyId,
        message: Incoming<ThresholdMessage>,
    ) -> Result<(), String> {
        let channels = self.channels.lock().await;

        if let Some(sender) = channels.get(&to) {
            sender
                .send(message)
                .map_err(|e| format!("Failed to send to party {}: {}", to, e))
        } else {
            Err(format!("Party {} not registered with coordinator", to))
        }
    }

    /// Create MPC channels for a party
    pub fn create_mpc_channels(
        &self,
        party_id: PartyId,
        message_rx: mpsc::UnboundedReceiver<Incoming<ThresholdMessage>>,
    ) -> (
        Pin<Box<dyn Stream<Item = Result<Incoming<ThresholdMessage>, std::io::Error>> + Send>>,
        Pin<Box<dyn Sink<Outgoing<ThresholdMessage>, Error = std::io::Error> + Send>>,
    ) {
        let coordinator = self.clone();

        // Incoming stream: Convert mpsc receiver to a Stream
        let incoming_stream = Box::pin(futures::stream::unfold(message_rx, |mut rx| async move {
            rx.recv().await.map(|msg| (Ok(msg), rx))
        }));

        // Outgoing sink: Convert mpsc sender to a Sink
        let outgoing_sink = Box::pin(futures::sink::unfold(
            coordinator,
            move |coordinator: KeygenNetworkCoordinator,
                  outgoing_msg: Outgoing<ThresholdMessage>| async move {
                match outgoing_msg.recipient {
                    MessageDestination::AllParties => {
                        let network_msg = Incoming {
                            id: 0,
                            sender: party_id,
                            msg_type: round_based::MessageType::Broadcast,
                            msg: outgoing_msg.msg,
                        };
                        coordinator.broadcast(party_id, network_msg).await;
                    }
                    MessageDestination::OneParty(id) => {
                        let network_msg = Incoming {
                            id: 0,
                            sender: party_id,
                            msg_type: round_based::MessageType::P2P,
                            msg: outgoing_msg.msg,
                        };
                        coordinator.send_to_party(id, network_msg).await.unwrap();
                    }
                }

                Ok::<_, std::io::Error>(coordinator)
            },
        ));

        (incoming_stream, outgoing_sink)
    }
}

/// Mock network coordinator for Signing protocol
#[derive(Clone)]
pub struct SigningNetworkCoordinator {
    channels: Arc<Mutex<HashMap<PartyId, mpsc::UnboundedSender<Incoming<SigningMessage>>>>>,
}

impl SigningNetworkCoordinator {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register_party(
        &self,
        party_id: PartyId,
        sender: mpsc::UnboundedSender<Incoming<SigningMessage>>,
    ) {
        let mut channels = self.channels.lock().await;
        channels.insert(party_id, sender);
        tracing::info!("🔗 Party {} registered with network coordinator", party_id);
    }

    pub async fn broadcast(&self, from: PartyId, message: Incoming<SigningMessage>) {
        let channels = self.channels.lock().await;

        for (&party_id, sender) in channels.iter() {
            if party_id != from {
                if let Err(e) = sender.send(message.clone()) {
                    tracing::info!("Failed to send broadcast to party {}: {}", party_id, e);
                }
            }
        }
    }

    /// Send a message to a specific party
    pub async fn send_to_party(
        &self,
        to: PartyId,
        message: Incoming<SigningMessage>,
    ) -> Result<(), String> {
        let channels = self.channels.lock().await;

        if let Some(sender) = channels.get(&to) {
            sender
                .send(message)
                .map_err(|e| format!("Failed to send to party {}: {}", to, e))
        } else {
            Err(format!("Party {} not registered with coordinator", to))
        }
    }

    /// Create MPC channels for a party
    pub fn create_mpc_channels(
        &self,
        party_id: PartyId,
        message_rx: mpsc::UnboundedReceiver<Incoming<SigningMessage>>,
    ) -> (
        Pin<Box<dyn Stream<Item = Result<Incoming<SigningMessage>, std::io::Error>> + Send>>,
        Pin<Box<dyn Sink<Outgoing<SigningMessage>, Error = std::io::Error> + Send>>,
    ) {
        let coordinator = self.clone();

        // Incoming stream: Convert mpsc receiver to a Stream
        let incoming_stream = Box::pin(futures::stream::unfold(message_rx, |mut rx| async move {
            rx.recv().await.map(|msg| (Ok(msg), rx))
        }));

        // Outgoing sink: Convert mpsc sender to a Sink
        let outgoing_sink = Box::pin(futures::sink::unfold(
            coordinator,
            move |coordinator: SigningNetworkCoordinator,
                  outgoing_msg: Outgoing<SigningMessage>| async move {
                match outgoing_msg.recipient {
                    MessageDestination::AllParties => {
                        let network_msg = Incoming {
                            id: 0,
                            sender: party_id,
                            msg_type: round_based::MessageType::Broadcast,
                            msg: outgoing_msg.msg,
                        };
                        coordinator.broadcast(party_id, network_msg).await;
                    }
                    MessageDestination::OneParty(id) => {
                        let network_msg = Incoming {
                            id: 0,
                            sender: party_id,
                            msg_type: round_based::MessageType::P2P,
                            msg: outgoing_msg.msg,
                        };
                        coordinator.send_to_party(id, network_msg).await.unwrap();
                    }
                }

                Ok::<_, std::io::Error>(coordinator)
            },
        ));

        (incoming_stream, outgoing_sink)
    }
}
