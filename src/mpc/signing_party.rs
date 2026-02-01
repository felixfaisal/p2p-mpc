use std::pin::Pin;

use cggmp24::ExecutionId;
use cggmp24::key_share::{AuxInfo, KeyShare};
use cggmp24::supported_curves::Secp256k1;
use futures::{Sink, Stream};
use rand::rngs::OsRng;
use round_based::{Incoming, MpcParty, Outgoing};
use sha2::Sha256;
use tokio::sync::mpsc;

// Import types from crypto module
use crate::mpc::{PartyId, SecurityLevelTest, SigningMessage};
use crate::network::NetworkCommand;

/// SigningParty represents a single party in the signing protocol
pub struct SigningParty {
    id: PartyId,
    #[allow(unused)]
    index: u16,
    message_rx: Option<mpsc::UnboundedReceiver<Incoming<SigningMessage>>>,
    #[allow(unused)]
    execution_id: Vec<u8>,
    key_share: KeyShare<Secp256k1, SecurityLevelTest>,
    #[allow(unused)]
    aux_info: AuxInfo<SecurityLevelTest>,
}

impl SigningParty {
    /// Create a new SigningParty with a message receiver channel, key share, and aux info
    pub fn new(
        id: PartyId,
        message_rx: mpsc::UnboundedReceiver<Incoming<SigningMessage>>,
        key_share: KeyShare<Secp256k1, SecurityLevelTest>,
        aux_info: AuxInfo<SecurityLevelTest>,
    ) -> Self {
        Self {
            id,
            index: id, // Using id as index for simplicity
            message_rx: Some(message_rx),
            execution_id: vec![],
            key_share,
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
        Pin<Box<dyn Stream<Item = Result<Incoming<SigningMessage>, std::io::Error>> + Send>>,
        Pin<Box<dyn Sink<Outgoing<SigningMessage>, Error = std::io::Error> + Send>>,
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
                  outgoing_msg: Outgoing<SigningMessage>| async move {
                // Send the message through the channel
                // The network worker will handle routing to appropriate parties
                tx.send(NetworkCommand::SendSigningMessage(outgoing_msg))
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))?;

                Ok::<_, std::io::Error>(tx)
            },
        ));

        (incoming_stream, outgoing_sink)
    }

    /// Sign a message using threshold signature scheme
    pub async fn sign_message(
        &mut self,
        signers: &[u16],
        message: &[u8],
        timestamp: u64,
        outgoing_tx: mpsc::UnboundedSender<NetworkCommand>,
    ) -> Result<cggmp24::Signature<Secp256k1>, Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!("✍️  Party {} starting signing process", self.id);
        tracing::info!("   Message: {:?}", String::from_utf8_lossy(message));

        let mut rng = OsRng;
        let eid = Self::create_shared_execution_id("signing", signers.len() as u16, timestamp);
        let data_to_sign = cggmp24::DataToSign::<Secp256k1>::digest::<Sha256>(message);

        // Setup MPC party with channels
        let (incoming, outgoing) = self.create_mpc_channels(outgoing_tx);
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
