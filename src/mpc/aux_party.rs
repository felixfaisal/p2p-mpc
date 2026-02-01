#[cfg(feature = "development")]
use std::path::PathBuf;
use std::pin::Pin;

use cggmp24::PregeneratedPrimes;
use cggmp24::key_share::AuxInfo;
use cggmp24::{ExecutionId, KeyRefreshError};
use futures::{Sink, Stream};
use rand::rngs::OsRng;
use round_based::{Incoming, MpcParty, Outgoing};
use tokio::sync::mpsc;

// Import types from crypto module
use crate::mpc::{AuxMsg, PartyId, SecurityLevelTest};
use crate::network::NetworkCommand;

/// AuxParty represents a single party in the aux generation protocol
pub struct AuxParty {
    id: PartyId,
    #[allow(unused)]
    index: u16,
    message_rx: Option<mpsc::UnboundedReceiver<Incoming<AuxMsg>>>,
    #[allow(unused)]
    execution_id: Vec<u8>,
}

impl AuxParty {
    /// Create a new AuxParty with a message receiver channel
    pub fn new(id: PartyId, message_rx: mpsc::UnboundedReceiver<Incoming<AuxMsg>>) -> Self {
        Self {
            id,
            index: id, // Using id as index for simplicity
            message_rx: Some(message_rx),
            execution_id: vec![],
        }
    }

    /// Get the path where Paillier primes are stored for this party
    #[cfg(feature = "development")]
    fn get_primes_path(party_id: PartyId) -> PathBuf {
        let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        path.push(".mpc_cache");
        std::fs::create_dir_all(&path).ok();
        path.push(format!("paillier_primes_party_{}.bin", party_id));
        path
    }

    /// Load Paillier primes from disk if they exist
    #[cfg(feature = "development")]
    fn load_primes_from_disk(party_id: PartyId) -> Option<PregeneratedPrimes<SecurityLevelTest>> {
        let primes_path = Self::get_primes_path(party_id);
        if !primes_path.exists() {
            return None;
        }

        match std::fs::read(&primes_path) {
            Ok(data) => match bincode::deserialize(&data) {
                Ok(primes) => {
                    tracing::info!("✓ Loaded cached Paillier primes from {:?}", primes_path);
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

    /// Save Paillier primes to disk for future reuse
    #[cfg(feature = "development")]
    fn save_primes_to_disk(party_id: PartyId, primes: &PregeneratedPrimes<SecurityLevelTest>) {
        let primes_path = Self::get_primes_path(party_id);

        match bincode::serialize(primes) {
            Ok(data) => match std::fs::write(&primes_path, &data) {
                Ok(_) => {
                    tracing::info!("✓ Saved Paillier primes to {:?}", primes_path);
                }
                Err(e) => {
                    tracing::warn!("Failed to write primes to {:?}: {}", primes_path, e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to serialize primes: {}", e);
            }
        }
    }

    /// Create execution ID that all parties will agree on
    pub fn create_shared_execution_id(
        protocol: &str,
        n: u16,
        timestamp: u64,
    ) -> ExecutionId<'static> {
        let mut id_data = Vec::new();
        id_data.extend_from_slice(protocol.as_bytes());
        id_data.extend_from_slice(&n.to_be_bytes());
        id_data.extend_from_slice(&timestamp.to_be_bytes());

        ExecutionId::new(Box::leak(id_data.into_boxed_slice()))
    }

    /// Create MPC channels for communication
    ///
    /// TODO: This function will be hooked to the P2P network worker in the future.
    /// Currently uses a local message passing mechanism.
    ///
    /// Returns: (incoming_stream, outgoing_sink) for MPC protocol
    pub fn create_mpc_channels(
        &mut self,
        outgoing_tx: mpsc::UnboundedSender<NetworkCommand>,
    ) -> (
        Pin<Box<dyn Stream<Item = Result<Incoming<AuxMsg>, std::io::Error>> + Send>>,
        Pin<Box<dyn Sink<Outgoing<AuxMsg>, Error = std::io::Error> + Send>>,
    ) {
        // Take ownership of message_rx
        let message_rx = self.message_rx.take().expect("message_rx already taken");

        // Incoming stream: Convert mpsc receiver to a Stream
        let incoming_stream = Box::pin(futures::stream::unfold(message_rx, |mut rx| async move {
            rx.recv().await.map(|msg| (Ok(msg), rx))
        }));

        // Outgoing sink: Convert to a Sink that sends through the channel
        let outgoing_sink = Box::pin(futures::sink::unfold(
            outgoing_tx,
            move |tx: mpsc::UnboundedSender<NetworkCommand>, outgoing_msg: Outgoing<AuxMsg>| async move {
                // Send the message through the channel
                // The coordinator will handle routing to appropriate parties
                tx.send(NetworkCommand::SendAuxMessage(outgoing_msg))
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))?;

                Ok::<_, std::io::Error>(tx)
            },
        ));

        (incoming_stream, outgoing_sink)
    }

    /// Generate aux info for threshold signature scheme
    pub async fn generate_aux_info(
        &mut self,
        n: u16,
        timestamp: u64,
        outgoing_tx: mpsc::UnboundedSender<NetworkCommand>,
    ) -> Result<AuxInfo<SecurityLevelTest>, KeyRefreshError> {
        tracing::info!("🔧 Party {} starting aux info generation", self.id);
        let mut rng = OsRng;
        let eid = Self::create_shared_execution_id("aux_gen", n, timestamp);

        // Load or generate Paillier primes
        #[cfg(feature = "development")]
        let primes = {
            tracing::warn!("⚠️  Development mode: Using cached Paillier primes");
            match Self::load_primes_from_disk(self.id) {
                Some(primes) => {
                    tracing::info!("✓ Party {} loaded cached Paillier primes", self.id);
                    primes
                }
                None => {
                    tracing::info!("⏳ Party {} generating new Paillier primes...", self.id);
                    let primes = PregeneratedPrimes::generate(&mut rng);
                    Self::save_primes_to_disk(self.id, &primes);
                    tracing::info!("✓ Party {} finished generating and cached primes", self.id);
                    primes
                }
            }
        };

        #[cfg(not(feature = "development"))]
        let primes = {
            tracing::info!("⏳ Party {} generating Paillier primes...", self.id);
            let primes = PregeneratedPrimes::generate(&mut rng);
            tracing::info!("✓ Party {} finished generating primes", self.id);
            primes
        };

        // Setup MPC party with channels
        let (incoming, outgoing) = self.create_mpc_channels(outgoing_tx);
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
        tracing::info!("   Party index: {}", self.id);
        tracing::info!("   Total parties: {}", n);

        Ok(aux_info)
    }
}
