// use std::collections::HashMap;
// use std::pin::Pin;
// use std::sync::Arc;
// use std::time::{SystemTime, UNIX_EPOCH};

// use cggmp24::ExecutionId;
// use cggmp24::KeyRefreshError;
// use cggmp24::KeyShare;
// use cggmp24::key_share::AuxInfo;
// use cggmp24::security_level::{KeygenSecurityLevel, SecurityLevel128};
// use cggmp24::supported_curves::Secp256k1;
// use futures::{Sink, Stream};
// use rand::rngs::OsRng;
// use round_based::{Incoming, MessageDestination, MpcParty, Outgoing};
// use sha2::Sha256;
// use tokio::sync::{Mutex, mpsc};
// use tokio::time;

// // Define security level for aux_info (using main cggmp24)
// // #[derive(Clone)]
// // pub struct SecurityLevelTest;

// // define_security_level!(SecurityLevelTest {
// //     kappa_bits: 80,         // Minimal for testing
// //     rsa_prime_bitlen: 1024, // RSA-2048 for testing (still weak, but more stable)
// //     rsa_pubkey_bitlen: 2047,
// //     epsilon: 256,
// //     ell: 128,
// //     ell_prime: 640,
// //     m: 128,
// // });
// pub type SecurityLevelTest = SecurityLevel128;

// // Define security level for keygen (using cggmp24_keygen)
// #[derive(Clone)]
// pub struct KeygenSecurityLevelTest;

// cggmp24_keygen::security_level::define_security_level!(KeygenSecurityLevelTest {
//     kappa_bits: 128, // Minimal for testing (matches SecurityLevelTest above)
// });

// // Type aliases for different protocol messages
// pub type AuxMsg = cggmp24::key_refresh::Msg<Sha256, SecurityLevelTest>;
// pub type ThresholdMessage =
//     cggmp24::keygen::msg::threshold::Msg<Secp256k1, SecurityLevelTest, Sha256>;
// pub type SigningMessage = cggmp24::signing::msg::Msg<Secp256k1, Sha256>;

// pub type PartyId = u16;

// #[derive(Clone)]
// struct NetworkCoordinator {
//     channels: Arc<Mutex<HashMap<PartyId, mpsc::UnboundedSender<Incoming<AuxMsg>>>>>,
// }

// impl NetworkCoordinator {
//     fn new() -> Self {
//         Self {
//             channels: Arc::new(Mutex::new(HashMap::new())),
//         }
//     }

//     async fn register_party(
//         &self,
//         party_id: PartyId,
//         sender: mpsc::UnboundedSender<Incoming<AuxMsg>>,
//     ) {
//         let mut channels = self.channels.lock().await;
//         channels.insert(party_id, sender);
//         tracing::info!("🔗 Party {} registered with network coordinator", party_id);
//     }

//     async fn broadcast(&self, from: PartyId, message: Incoming<AuxMsg>) {
//         let channels = self.channels.lock().await;

//         for (&party_id, sender) in channels.iter() {
//             if party_id != from {
//                 if let Err(e) = sender.send(message.clone()) {
//                     tracing::info!("Failed to send broadcast to party {}: {}", party_id, e);
//                 }
//             }
//         }
//     }

//     /// Send a message to a specific party
//     async fn send_to_party(&self, to: PartyId, message: Incoming<AuxMsg>) -> Result<(), String> {
//         let channels = self.channels.lock().await;

//         if let Some(sender) = channels.get(&to) {
//             sender
//                 .send(message)
//                 .map_err(|e| format!("Failed to send to party {}: {}", to, e))
//         } else {
//             Err(format!("Party {} not registered with coordinator", to))
//         }
//     }
// }

// #[derive(Clone)]
// struct KeygenNetworkCoordinator {
//     channels: Arc<Mutex<HashMap<PartyId, mpsc::UnboundedSender<Incoming<ThresholdMessage>>>>>,
// }

// impl KeygenNetworkCoordinator {
//     fn new() -> Self {
//         Self {
//             channels: Arc::new(Mutex::new(HashMap::new())),
//         }
//     }

//     async fn register_party(
//         &self,
//         party_id: PartyId,
//         sender: mpsc::UnboundedSender<Incoming<ThresholdMessage>>,
//     ) {
//         let mut channels = self.channels.lock().await;
//         channels.insert(party_id, sender);
//         tracing::info!("🔗 Party {} registered with network coordinator", party_id);
//     }

//     async fn broadcast(&self, from: PartyId, message: Incoming<ThresholdMessage>) {
//         let channels = self.channels.lock().await;

//         for (&party_id, sender) in channels.iter() {
//             if party_id != from {
//                 if let Err(e) = sender.send(message.clone()) {
//                     tracing::info!("Failed to send broadcast to party {}: {}", party_id, e);
//                 }
//             }
//         }
//     }

//     /// Send a message to a specific party
//     async fn send_to_party(
//         &self,
//         to: PartyId,
//         message: Incoming<ThresholdMessage>,
//     ) -> Result<(), String> {
//         let channels = self.channels.lock().await;

//         if let Some(sender) = channels.get(&to) {
//             sender
//                 .send(message)
//                 .map_err(|e| format!("Failed to send to party {}: {}", to, e))
//         } else {
//             Err(format!("Party {} not registered with coordinator", to))
//         }
//     }
// }

// #[derive(Clone)]
// struct SigningNetworkCoordinator {
//     channels: Arc<Mutex<HashMap<PartyId, mpsc::UnboundedSender<Incoming<SigningMessage>>>>>,
// }

// impl SigningNetworkCoordinator {
//     fn new() -> Self {
//         Self {
//             channels: Arc::new(Mutex::new(HashMap::new())),
//         }
//     }

//     async fn register_party(
//         &self,
//         party_id: PartyId,
//         sender: mpsc::UnboundedSender<Incoming<SigningMessage>>,
//     ) {
//         let mut channels = self.channels.lock().await;
//         channels.insert(party_id, sender);
//         tracing::info!("🔗 Party {} registered with network coordinator", party_id);
//     }

//     async fn broadcast(&self, from: PartyId, message: Incoming<SigningMessage>) {
//         let channels = self.channels.lock().await;

//         for (&party_id, sender) in channels.iter() {
//             if party_id != from {
//                 if let Err(e) = sender.send(message.clone()) {
//                     tracing::info!("Failed to send broadcast to party {}: {}", party_id, e);
//                 }
//             }
//         }
//     }

//     /// Send a message to a specific party
//     async fn send_to_party(
//         &self,
//         to: PartyId,
//         message: Incoming<SigningMessage>,
//     ) -> Result<(), String> {
//         let channels = self.channels.lock().await;

//         if let Some(sender) = channels.get(&to) {
//             sender
//                 .send(message)
//                 .map_err(|e| format!("Failed to send to party {}: {}", to, e))
//         } else {
//             Err(format!("Party {} not registered with coordinator", to))
//         }
//     }
// }

// struct AuxParty {
//     id: PartyId,
//     index: u16,
//     coordinator: NetworkCoordinator,
//     message_rx: Option<mpsc::UnboundedReceiver<Incoming<AuxMsg>>>,
//     execution_id: Vec<u8>,
// }

// impl AuxParty {
//     async fn new(id: PartyId, coordinator: NetworkCoordinator) -> Self {
//         let (tx, rx) = mpsc::unbounded_channel();
//         coordinator.register_party(id, tx).await;

//         Self {
//             id,
//             index: id, // Using id as index for simplicity
//             coordinator,
//             message_rx: Some(rx),
//             execution_id: vec![],
//         }
//     }

//     /// Create execution ID that all parties will agree on
//     fn create_shared_execution_id(
//         protocol: &str, // No self needed at all!
//         n: u16,
//         timestamp: u64,
//     ) -> ExecutionId<'static> {
//         let mut id_data = Vec::new();
//         id_data.extend_from_slice(protocol.as_bytes());
//         id_data.extend_from_slice(&n.to_be_bytes());
//         id_data.extend_from_slice(&timestamp.to_be_bytes());

//         ExecutionId::new(Box::leak(id_data.into_boxed_slice()))
//     }

//     fn create_mpc_channels(
//         &mut self,
//     ) -> (
//         Pin<Box<dyn Stream<Item = Result<Incoming<AuxMsg>, std::io::Error>> + Send>>,
//         Pin<Box<dyn Sink<Outgoing<AuxMsg>, Error = std::io::Error> + Send>>,
//     ) {
//         let coordinator = self.coordinator.clone();
//         let party_id = self.id;

//         // Take ownership of message_rx
//         let message_rx = self.message_rx.take().expect("message_rx already taken");

//         // Incoming stream: Convert mpsc receiver to a Stream
//         let incoming_stream = Box::pin(futures::stream::unfold(message_rx, |mut rx| async move {
//             rx.recv().await.map(|msg| (Ok(msg), rx))
//         }));

//         // Outgoing sink: Convert mpsc sender to a Sink
//         let outgoing_sink = Box::pin(futures::sink::unfold(
//             coordinator,
//             move |coordinator: NetworkCoordinator, outgoing_msg: Outgoing<AuxMsg>| async move {
//                 match outgoing_msg.recipient {
//                     MessageDestination::AllParties => {
//                         let network_msg = Incoming {
//                             id: 0,
//                             sender: party_id,
//                             msg_type: round_based::MessageType::Broadcast,
//                             msg: outgoing_msg.msg,
//                         };
//                         coordinator.broadcast(party_id, network_msg).await;
//                     }
//                     MessageDestination::OneParty(id) => {
//                         let network_msg = Incoming {
//                             id: 0,
//                             sender: party_id,
//                             msg_type: round_based::MessageType::P2P,
//                             msg: outgoing_msg.msg,
//                         };
//                         coordinator.send_to_party(id, network_msg).await.unwrap();
//                     }
//                 }

//                 Ok::<_, std::io::Error>(coordinator)
//             },
//         ));

//         (incoming_stream, outgoing_sink)
//     }

//     pub async fn generate_aux_info(
//         &mut self,
//         n: u16,
//         timestamp: u64,
//     ) -> Result<AuxInfo<SecurityLevelTest>, KeyRefreshError> {
//         tracing::info!("🔧 Party {} starting aux info generation", self.id);
//         let mut rng = OsRng;
//         let eid = Self::create_shared_execution_id("aux_gen", n, timestamp);

//         // Generate Paillier primes (this is expensive and takes time)
//         // Generate Paillier primes (this is expensive)
//         tracing::info!("⏳ Party {} generating Paillier primes...", self.id);
//         let primes = cggmp24::PregeneratedPrimes::generate(&mut rng);
//         tracing::info!("✓ Party {} finished generating primes", self.id);

//         // Setup MPC party with channels
//         let (incoming, outgoing) = self.create_mpc_channels();
//         let mpc_party = MpcParty::connected((incoming, outgoing));
//         // let mpc_party = setup_mpc_party(incoming_rx, outgoing_tx, party_id);

//         // Run aux info generation protocol
//         tracing::info!(
//             "🔄 Party {} running aux info generation protocol...",
//             self.id
//         );
//         let aux_info = cggmp24::aux_info_gen(eid, self.id.clone(), n, primes)
//             .start(&mut rng, mpc_party)
//             .await?;

//         tracing::info!("✅ Party {} completed aux info generation", self.id);
//         tracing::info!("   Party index: {}", self.id);
//         tracing::info!("   Total parties: {}", n);

//         Ok(aux_info)
//     }
// }

// struct KeygenParty {
//     id: PartyId,
//     index: u16,
//     coordinator: KeygenNetworkCoordinator,
//     message_rx: Option<mpsc::UnboundedReceiver<Incoming<ThresholdMessage>>>,
//     execution_id: Vec<u8>,
//     aux_info: AuxInfo<SecurityLevelTest>,
// }

// impl KeygenParty {
//     async fn new(
//         id: PartyId,
//         coordinator: KeygenNetworkCoordinator,
//         aux_info: AuxInfo<SecurityLevelTest>,
//     ) -> Self {
//         let (tx, rx) = mpsc::unbounded_channel();
//         coordinator.register_party(id, tx).await;

//         Self {
//             id,
//             index: id, // Using id as index for simplicity
//             coordinator,
//             message_rx: Some(rx),
//             execution_id: vec![],
//             aux_info,
//         }
//     }

//     /// Create execution ID that all parties will agree on
//     fn create_shared_execution_id(
//         protocol: &str, // No self needed at all!
//         n: u16,
//         timestamp: u64,
//     ) -> ExecutionId<'static> {
//         let mut id_data = Vec::new();
//         id_data.extend_from_slice(protocol.as_bytes());
//         id_data.extend_from_slice(&n.to_be_bytes());
//         id_data.extend_from_slice(&timestamp.to_be_bytes());

//         ExecutionId::new(Box::leak(id_data.into_boxed_slice()))
//     }

//     fn create_mpc_channels(
//         &mut self,
//     ) -> (
//         Pin<Box<dyn Stream<Item = Result<Incoming<ThresholdMessage>, std::io::Error>> + Send>>,
//         Pin<Box<dyn Sink<Outgoing<ThresholdMessage>, Error = std::io::Error> + Send>>,
//     ) {
//         let coordinator = self.coordinator.clone();
//         let party_id = self.id;

//         // Take ownership of message_rx
//         let message_rx = self.message_rx.take().expect("message_rx already taken");

//         // Incoming stream: Convert mpsc receiver to a Stream
//         let incoming_stream = Box::pin(futures::stream::unfold(message_rx, |mut rx| async move {
//             rx.recv().await.map(|msg| (Ok(msg), rx))
//         }));

//         // Outgoing sink: Convert mpsc sender to a Sink
//         let outgoing_sink = Box::pin(futures::sink::unfold(
//             coordinator,
//             move |coordinator: KeygenNetworkCoordinator,
//                   outgoing_msg: Outgoing<ThresholdMessage>| async move {
//                 match outgoing_msg.recipient {
//                     MessageDestination::AllParties => {
//                         let network_msg = Incoming {
//                             id: 0,
//                             sender: party_id,
//                             msg_type: round_based::MessageType::Broadcast,
//                             msg: outgoing_msg.msg,
//                         };
//                         coordinator.broadcast(party_id, network_msg).await;
//                     }
//                     MessageDestination::OneParty(id) => {
//                         let network_msg = Incoming {
//                             id: 0,
//                             sender: party_id,
//                             msg_type: round_based::MessageType::P2P,
//                             msg: outgoing_msg.msg,
//                         };
//                         coordinator.send_to_party(id, network_msg).await.unwrap();
//                     }
//                 }

//                 Ok::<_, std::io::Error>(coordinator)
//             },
//         ));

//         (incoming_stream, outgoing_sink)
//     }

//     pub async fn generate_key_share(
//         &mut self,
//         n: u16,
//         t: u16,
//         timestamp: u64,
//         aux_info: AuxInfo<SecurityLevelTest>,
//     ) -> Result<KeyShare<Secp256k1, SecurityLevelTest>, Box<dyn std::error::Error + Send + Sync>>
//     {
//         tracing::info!(
//             "🔑 Party {} (index {}) starting key generation (t={}, n={})",
//             self.id,
//             self.id,
//             t,
//             n
//         );

//         let mut rng = OsRng;
//         let eid = Self::create_shared_execution_id("keygen", n, timestamp);

//         // Setup MPC party with channels
//         let (incoming, outgoing) = self.create_mpc_channels();
//         let mpc_party = MpcParty::connected((incoming, outgoing));

//         tracing::info!("🔄 Party {} running keygen protocol...", self.id);
//         let incomplete_key_share = cggmp24::keygen::<Secp256k1>(eid, self.id, n)
//             .set_security_level::<SecurityLevelTest>()
//             .set_threshold(t)
//             .start(&mut rng, mpc_party)
//             .await
//             .unwrap();

//         let key_share = cggmp24::KeyShare::from_parts((incomplete_key_share, aux_info)).unwrap();

//         tracing::info!("✅ Party {} completed key generation", self.id);
//         tracing::info!(
//             "   Shared public key: {}",
//             hex::encode(key_share.shared_public_key.to_bytes(true))
//         );

//         Ok(key_share)
//     }
// }

// struct SigningParty {
//     id: PartyId,
//     index: u16,
//     coordinator: SigningNetworkCoordinator,
//     message_rx: Option<mpsc::UnboundedReceiver<Incoming<SigningMessage>>>,
//     execution_id: Vec<u8>,
//     key_share: KeyShare<Secp256k1, SecurityLevelTest>,
//     aux_info: AuxInfo<SecurityLevelTest>,
// }

// impl SigningParty {
//     async fn new(
//         id: PartyId,
//         coordinator: SigningNetworkCoordinator,
//         aux_info: AuxInfo<SecurityLevelTest>,
//         key_share: KeyShare<Secp256k1, SecurityLevelTest>,
//     ) -> Self {
//         let (tx, rx) = mpsc::unbounded_channel();
//         coordinator.register_party(id, tx).await;

//         Self {
//             id,
//             index: id, // Using id as index for simplicity
//             coordinator,
//             message_rx: Some(rx),
//             execution_id: vec![],
//             key_share,
//             aux_info,
//         }
//     }

//     /// Create execution ID that all parties will agree on
//     fn create_shared_execution_id(
//         protocol: &str, // No self needed at all!
//         n: u16,
//         timestamp: u64,
//     ) -> ExecutionId<'static> {
//         let mut id_data = Vec::new();
//         id_data.extend_from_slice(protocol.as_bytes());
//         id_data.extend_from_slice(&n.to_be_bytes());
//         id_data.extend_from_slice(&timestamp.to_be_bytes());

//         ExecutionId::new(Box::leak(id_data.into_boxed_slice()))
//     }

//     fn create_mpc_channels(
//         &mut self,
//     ) -> (
//         Pin<Box<dyn Stream<Item = Result<Incoming<SigningMessage>, std::io::Error>> + Send>>,
//         Pin<Box<dyn Sink<Outgoing<SigningMessage>, Error = std::io::Error> + Send>>,
//     ) {
//         let coordinator = self.coordinator.clone();
//         let party_id = self.id;

//         // Take ownership of message_rx
//         let message_rx = self.message_rx.take().expect("message_rx already taken");

//         // Incoming stream: Convert mpsc receiver to a Stream
//         let incoming_stream = Box::pin(futures::stream::unfold(message_rx, |mut rx| async move {
//             rx.recv().await.map(|msg| (Ok(msg), rx))
//         }));

//         // Outgoing sink: Convert mpsc sender to a Sink
//         let outgoing_sink = Box::pin(futures::sink::unfold(
//             coordinator,
//             move |coordinator: SigningNetworkCoordinator,
//                   outgoing_msg: Outgoing<SigningMessage>| async move {
//                 match outgoing_msg.recipient {
//                     MessageDestination::AllParties => {
//                         let network_msg = Incoming {
//                             id: 0,
//                             sender: party_id,
//                             msg_type: round_based::MessageType::Broadcast,
//                             msg: outgoing_msg.msg,
//                         };
//                         coordinator.broadcast(party_id, network_msg).await;
//                     }
//                     MessageDestination::OneParty(id) => {
//                         let network_msg = Incoming {
//                             id: 0,
//                             sender: party_id,
//                             msg_type: round_based::MessageType::P2P,
//                             msg: outgoing_msg.msg,
//                         };
//                         coordinator.send_to_party(id, network_msg).await.unwrap();
//                     }
//                 }

//                 Ok::<_, std::io::Error>(coordinator)
//             },
//         ));

//         (incoming_stream, outgoing_sink)
//     }

//     pub async fn sign_message(
//         &mut self,
//         signers: &[u16],
//         message: &[u8],
//         timestamp: u64,
//         key_share: &KeyShare<Secp256k1, SecurityLevelTest>,
//     ) -> Result<cggmp24::Signature<Secp256k1>, Box<dyn std::error::Error + Send + Sync>> {
//         tracing::info!("✍️  Party {} starting signing process", self.id);
//         tracing::info!("   Message: {:?}", String::from_utf8_lossy(message));

//         let mut rng = OsRng;
//         let eid = Self::create_shared_execution_id("signing", signers.len() as u16, timestamp);
//         let data_to_sign = cggmp24::DataToSign::<Secp256k1>::digest::<Sha256>(message);
//         // Setup MPC party with channels
//         let (incoming, outgoing) = self.create_mpc_channels();
//         let mpc_party = MpcParty::connected((incoming, outgoing));

//         tracing::info!("🔄 Party {} running signing protocol...", self.id);
//         let signature =
//             cggmp24::signing::<Secp256k1, SecurityLevelTest>(eid, self.id, signers, key_share)
//                 .sign(&mut rng, mpc_party, &data_to_sign)
//                 .await?;

//         tracing::info!("✅ Party {} completed signing", self.id);

//         Ok(signature)
//     }
// }

// // // Sign a message using CGGMP24 threshold signature
// // // Uses Sha256 hash and Secp256k1 curve

// async fn coordinate_parties(n: u16, t: u16) -> (u64, u64, u64) {
//     tracing::info!(
//         "🤝 Coordinating {} parties for {}-of-{} threshold scheme",
//         n,
//         t,
//         n
//     );

//     let now = SystemTime::now()
//         .duration_since(UNIX_EPOCH)
//         .unwrap()
//         .as_secs();

//     // In real implementation, parties would exchange these over authenticated channels
//     let aux_timestamp = now;
//     let keygen_timestamp = now + 1;
//     let signing_timestamp = now + 2;

//     tracing::info!("📋 Agreed timestamps:");
//     tracing::info!("   Aux generation: {}", aux_timestamp);
//     tracing::info!("   Key generation: {}", keygen_timestamp);
//     tracing::info!("   Signing: {}", signing_timestamp);

//     (aux_timestamp, keygen_timestamp, signing_timestamp)
// }

// pub async fn run() {
//     tracing::info!("🚀 Starting CGGMP24 3-party threshold signature simulation");

//     let n = 3u16; // Total parties
//     let t = 2u16; // Threshold (minimum signers)

//     // Coordinate timing
//     let (aux_timestamp, keygen_timestamp, signing_timestamp) = coordinate_parties(n, t).await;

//     // Create network coordinator
//     let coordinator = NetworkCoordinator::new();

//     // Create parties
//     // let mut parties = Vec::new();
//     // for i in 0..n {
//     //     parties.push(Party::new(i, coordinator.clone()));
//     // }
//     let mut alice = AuxParty::new(0, coordinator.clone()).await;
//     let mut bob = AuxParty::new(1, coordinator.clone()).await;
//     let mut charlie = AuxParty::new(2, coordinator.clone()).await;

//     tracing::info!("\n🔧 PHASE 1: Auxiliary Information Generation");

//     // // Generate auxiliary info for each party (can be done independently)
//     // let mut handles = Vec::new();
//     // for (i, mut party) in parties.into_iter().enumerate() {
//     //     let handle = tokio::spawn(async move {
//     //         let aux_info = party.generate_aux_info(n, aux_timestamp).await.unwrap();
//     //         Ok::<_, Box<dyn std::error::Error + Send + Sync>>((party, aux_info))
//     //     });
//     //     handles.push(handle);
//     // }

//     let alice_handle = tokio::task::spawn(async move {
//         let aux_info = alice.generate_aux_info(n, aux_timestamp).await.unwrap();
//         Ok::<_, Box<dyn std::error::Error + Send + Sync>>((alice, aux_info))
//     });

//     let bob_handle = tokio::task::spawn(async move {
//         let aux_info = bob.generate_aux_info(n, aux_timestamp).await.unwrap();
//         Ok::<_, Box<dyn std::error::Error + Send + Sync>>((bob, aux_info))
//     });
//     let charlie_handle = tokio::task::spawn(async move {
//         let aux_info = charlie.generate_aux_info(n, aux_timestamp).await.unwrap();
//         Ok::<_, Box<dyn std::error::Error + Send + Sync>>((charlie, aux_info))
//     });

//     let alice_aux_info = alice_handle.await.unwrap().unwrap().1;
//     let bob_aux_info = bob_handle.await.unwrap().unwrap().1;
//     let charlie_aux_info = charlie_handle.await.unwrap().unwrap().1;

//     // Create network coordinator
//     let coordinator = KeygenNetworkCoordinator::new();

//     let mut alice = KeygenParty::new(0, coordinator.clone(), alice_aux_info).await;
//     let mut bob = KeygenParty::new(1, coordinator.clone(), bob_aux_info).await;
//     let mut charlie = KeygenParty::new(2, coordinator.clone(), charlie_aux_info).await;

//     tracing::info!("\n🔧 PHASE 2: Keygen Information Generation");

//     let alice_handle = tokio::task::spawn(async move {
//         let key_share = alice
//             .generate_key_share(n, t, keygen_timestamp, alice.aux_info.clone())
//             .await
//             .unwrap();
//         Ok::<_, Box<dyn std::error::Error + Send + Sync>>((alice, key_share))
//     });

//     let bob_handle = tokio::task::spawn(async move {
//         let key_share = bob
//             .generate_key_share(n, t, keygen_timestamp, bob.aux_info.clone())
//             .await
//             .unwrap();
//         Ok::<_, Box<dyn std::error::Error + Send + Sync>>((bob, key_share))
//     });
//     let charlie_handle = tokio::task::spawn(async move {
//         let key_share = charlie
//             .generate_key_share(n, t, keygen_timestamp, charlie.aux_info.clone())
//             .await
//             .unwrap();
//         Ok::<_, Box<dyn std::error::Error + Send + Sync>>((charlie, key_share))
//     });

//     let (alice, alice_key_share) = alice_handle.await.unwrap().unwrap();
//     let (bob, bob_key_share) = bob_handle.await.unwrap().unwrap();
//     let (charlie, charlie_key_share) = charlie_handle.await.unwrap().unwrap();

//     // Create network coordinator
//     let coordinator = SigningNetworkCoordinator::new();

//     let mut alice = SigningParty::new(
//         0,
//         coordinator.clone(),
//         alice.aux_info,
//         alice_key_share.clone(),
//     )
//     .await;
//     let mut bob =
//         SigningParty::new(1, coordinator.clone(), bob.aux_info, bob_key_share.clone()).await;
//     let mut charlie = SigningParty::new(
//         2,
//         coordinator.clone(),
//         charlie.aux_info,
//         charlie_key_share.clone(),
//     )
//     .await;

//     tracing::info!("\n🔧 PHASE 3: Message Signing");
//     let message = b"Hello, CGGMP24 threshold cryptography!";

//     let alice_handle = tokio::task::spawn(async move {
//         let signature = alice
//             .sign_message(&[0, 1], message, signing_timestamp, &alice_key_share)
//             .await
//             .unwrap();
//         Ok::<_, Box<dyn std::error::Error + Send + Sync>>((alice, signature))
//     });

//     let bob_handle = tokio::task::spawn(async move {
//         let signature = bob
//             .sign_message(&[0, 1], message, signing_timestamp, &bob_key_share)
//             .await
//             .unwrap();
//         Ok::<_, Box<dyn std::error::Error + Send + Sync>>((bob, signature))
//     });
//     // let charlie_handle = tokio::task::spawn(async move {
//     //     let signature = charlie
//     //         .sign_message(&[0, 1], message, aux_timestamp, &charlie_key_share)
//     //         .await
//     //         .unwrap();
//     //     Ok::<_, Box<dyn std::error::Error + Send + Sync>>((charlie, signature))
//     // });

//     let (alice, alice_signature) = alice_handle.await.unwrap().unwrap();
//     let (bob, bob_signature) = bob_handle.await.unwrap().unwrap();
//     // let (charlie, charlie_signature) = charlie_handle.await.unwrap().unwrap();

//     println!("\n🔍 VERIFICATION");

//     let public_key = alice.key_share.shared_public_key;
//     let data_to_sign = cggmp24::DataToSign::<Secp256k1>::digest::<Sha256>(message);
//     alice_signature.verify(&public_key, &data_to_sign).unwrap();
//     bob_signature.verify(&public_key, &data_to_sign).unwrap();

//     tracing::info!("Signature Verification Complete!");
// }
