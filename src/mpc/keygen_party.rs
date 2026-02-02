use std::pin::Pin;

use cggmp24::ExecutionId;
use cggmp24::key_share::{AuxInfo, KeyShare};
use cggmp24::supported_curves::Secp256k1;
use futures::{Sink, Stream};
use rand::rngs::OsRng;
use round_based::{Incoming, MpcParty, Outgoing};
use tokio::sync::mpsc;

// Import types from crypto module
use crate::mpc::{PartyId, SecurityLevelTest, ThresholdMessage};
use crate::network::NetworkCommand;

/// KeygenParty represents a single party in the keygen protocol
pub struct KeygenParty {
    id: PartyId,
    #[allow(unused)]
    index: u16,
    message_rx: Option<mpsc::UnboundedReceiver<Incoming<ThresholdMessage>>>,
    #[allow(unused)]
    execution_id: Vec<u8>,
    aux_info: AuxInfo<SecurityLevelTest>,
}

impl KeygenParty {
    /// Create a new KeygenParty with a message receiver channel and aux info
    pub fn new(
        id: PartyId,
        message_rx: mpsc::UnboundedReceiver<Incoming<ThresholdMessage>>,
        aux_info: AuxInfo<SecurityLevelTest>,
    ) -> Self {
        Self {
            id,
            index: id, // Using id as index for simplicity
            message_rx: Some(message_rx),
            execution_id: vec![],
            aux_info,
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
    /// This function is hooked to the P2P network worker.
    ///
    /// Returns: (incoming_stream, outgoing_sink) for MPC protocol
    pub fn create_mpc_channels(
        &mut self,
        outgoing_tx: mpsc::UnboundedSender<NetworkCommand>,
    ) -> (
        Pin<Box<dyn Stream<Item = Result<Incoming<ThresholdMessage>, std::io::Error>> + Send>>,
        Pin<Box<dyn Sink<Outgoing<ThresholdMessage>, Error = std::io::Error> + Send>>,
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
            move |tx: mpsc::UnboundedSender<NetworkCommand>,
                  outgoing_msg: Outgoing<ThresholdMessage>| async move {
                // Send the message through the channel
                // The network worker will handle routing to appropriate parties
                tx.send(NetworkCommand::SendKeygenMessage(outgoing_msg))
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))?;

                Ok::<_, std::io::Error>(tx)
            },
        ));

        (incoming_stream, outgoing_sink)
    }

    /// Generate key share for threshold signature scheme
    #[tracing::instrument(skip(self, outgoing_tx), fields(party_id = %self.id, n, threshold = t, timestamp))]
    pub async fn generate_key_share(
        &mut self,
        n: u16,
        t: u16,
        timestamp: u64,
        outgoing_tx: mpsc::UnboundedSender<NetworkCommand>,
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
        let (incoming, outgoing) = self.create_mpc_channels(outgoing_tx);
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
