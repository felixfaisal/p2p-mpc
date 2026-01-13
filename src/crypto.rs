use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use cggmp24::key_refresh::Msg;
use cggmp24::key_share::AuxInfo;
use cggmp24::security_level::SecurityLevel128;
use cggmp24::supported_curves::Secp256k1;
use cggmp24::{DataToSign, ExecutionId};
use futures::{Sink, SinkExt, Stream};
use rand::rngs::OsRng;
use round_based::{Incoming, MessageDestination, MessageType, MpcParty, Outgoing};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

// You need to specify the concrete types for D and L
type AuxMsg = Msg<Sha256, SecurityLevel128>;

type PartyId = u16;

/// Creates MPC communication channels (Stream for incoming, Sink for outgoing)
///
/// # Type Parameters
/// - `M`: The message type for this MPC protocol (must be Serialize + DeserializeOwned)
///
/// # Arguments
/// - `incoming_rx`: Channel receiver for incoming messages from other parties
/// - `outgoing_tx`: Channel sender for outgoing messages to other parties
/// - `party_id`: This party's ID
///
/// # Returns
/// A tuple of (incoming_stream, outgoing_sink) that can be used with MpcParty
fn create_mpc_channels<M>(
    incoming_rx: mpsc::UnboundedReceiver<Incoming<M>>,
    outgoing_tx: mpsc::UnboundedSender<Outgoing<M>>,
    party_id: PartyId,
) -> (
    Pin<Box<dyn Stream<Item = Result<Incoming<M>, std::io::Error>> + Send>>,
    Pin<Box<dyn Sink<Outgoing<M>, Error = std::io::Error> + Send>>,
)
where
    M: Send + 'static,
{
    // Incoming stream: Convert mpsc receiver to a Stream
    let incoming_stream = Box::pin(futures::stream::unfold(incoming_rx, |mut rx| async move {
        rx.recv().await.map(|msg| (Ok(msg), rx))
    }));

    // Outgoing sink: Convert mpsc sender to a Sink
    let outgoing_sink = Box::pin(futures::sink::unfold(
        outgoing_tx,
        |tx, msg: Outgoing<M>| async move {
            tx.send(msg).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Channel closed")
            })?;
            Ok::<_, std::io::Error>(tx)
        },
    ));

    (incoming_stream, outgoing_sink)
}

/// Creates an MPC party with communication channels
///
/// # Type Parameters
/// - `M`: The message type for this MPC protocol
///
/// # Arguments
/// - `incoming_rx`: Channel receiver for incoming messages
/// - `outgoing_tx`: Channel sender for outgoing messages
/// - `party_id`: This party's ID
///
/// # Returns
/// An MpcParty ready to run protocols
fn setup_mpc_party<M>(
    incoming_rx: mpsc::UnboundedReceiver<Incoming<M>>,
    outgoing_tx: mpsc::UnboundedSender<Outgoing<M>>,
    party_id: PartyId,
) -> MpcParty<
    M,
    (
        Pin<Box<dyn Stream<Item = Result<Incoming<M>, std::io::Error>> + Send>>,
        Pin<Box<dyn Sink<Outgoing<M>, Error = std::io::Error> + Send>>,
    ),
>
where
    M: Send + 'static,
{
    let (incoming, outgoing) = create_mpc_channels(incoming_rx, outgoing_tx, party_id);
    MpcParty::connected((incoming, outgoing))
}

/// Generate auxiliary information for CGGMP24 protocol
///
/// # Arguments
/// - `party_id`: This party's unique identifier
/// - `party_index`: This party's index (0 to n-1)
/// - `n`: Total number of parties
/// - `execution_id`: Unique execution ID for this protocol run
/// - `incoming_rx`: Channel to receive messages from other parties
/// - `outgoing_tx`: Channel to send messages to other parties
///
/// # Returns
/// The generated AuxInfo on success
pub async fn generate_aux_info(
    party_id: PartyId,
    party_index: u16,
    n: u16,
    execution_id: ExecutionId<'_>,
    incoming_rx: mpsc::UnboundedReceiver<Incoming<AuxMsg>>,
    outgoing_tx: mpsc::UnboundedSender<Outgoing<AuxMsg>>,
) {
    println!(
        "🔧 Party {} (index {}) starting aux info generation",
        party_id, party_index
    );

    let mut rng = OsRng;

    // Generate Paillier primes (this is expensive and takes time)
    println!(
        "⏳ Party {} generating Paillier primes (this may take a while)...",
        party_id
    );
    let primes = cggmp24::PregeneratedPrimes::generate(&mut rng);
    println!("✓ Party {} finished generating primes", party_id);

    // Setup MPC party with channels
    let mpc_party = setup_mpc_party(incoming_rx, outgoing_tx, party_id);

    // Run aux info generation protocol
    println!(
        "🔄 Party {} running aux info generation protocol...",
        party_id
    );
    let aux_info = cggmp24::aux_info_gen(execution_id, party_index, n, primes)
        .start(&mut rng, mpc_party)
        .await
        .unwrap();

    println!("✅ Party {} completed aux info generation", party_id);
    println!("   Party index: {}", party_index);
    println!("   Total parties: {}", n);

    // Ok(aux_info)
}
