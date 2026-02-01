use anyhow::Error;
use libp2p::futures::StreamExt;
use libp2p::gossipsub::Sha256Topic;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{
    Multiaddr, NetworkBehaviour, Transport,
    core::{
        PeerId, identity,
        muxing::StreamMuxerBox,
        transport::{Boxed, upgrade::Version},
        upgrade::SelectUpgrade,
    },
    gossipsub::{
        self, Gossipsub, GossipsubEvent, GossipsubMessage, MessageAuthenticity, MessageId,
    },
    identify,
    kad::{Kademlia, KademliaConfig, KademliaEvent, KademliaStoreInserts, store::MemoryStore},
    mplex::MplexConfig,
    ping,
    swarm::Swarm,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize},
};
use std::time::Duration;
use tokio::sync::RwLock as TokioRwLock;
use tokio::sync::mpsc;

use crate::mpc::{CoordinationMessage, MPCOrchestrator};

pub const PROTOCOL_NAME: &str = "/felix/mpc/0.0.0";
pub const MPC_COORDINATION_TOPIC: &str = "mpc-coordination";
pub const MPC_ASSIGNMENT_TOPIC: &str = "mpc-assignment";

pub fn gossip_protocol_name() -> Vec<Cow<'static, [u8]>> {
    vec![PROTOCOL_NAME.as_bytes().into()]
}

/// Assignment message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AssignmentMessage {
    /// Assignment request from coordinator to peers
    Request {
        coordinator_id: String,
        assignments: std::collections::HashMap<String, usize>,
        session_id: String,
    },
    /// Acceptance response from peer to coordinator
    Acceptance {
        peer_id: String,
        assigned_index: usize,
        session_id: String,
    },
}

/// MPC Aux Protocol Message wrapper for P2P transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpcAuxMessage {
    /// Sender's peer ID
    pub from: PeerId,
    /// Optional recipient party index (None = broadcast to all)
    pub to: Option<u16>,
    /// Message type (Broadcast or P2P)
    pub msg_type: String, // "broadcast" or "p2p"
    /// Serialized message payload (bincode serialized AuxMsg)
    pub payload: Vec<u8>,
}

/// Commands that can be sent to the network worker
pub enum NetworkCommand {
    /// Broadcast a message to a specific topic
    Broadcast { topic: String, data: Vec<u8> },
    /// Subscribe to a new topic
    #[allow(unused)]
    Subscribe { topic: String },
    /// Unsubscribe from a topic
    #[allow(unused)]
    Unsubscribe { topic: String },
    /// Dial a specific peer
    #[allow(unused)]
    Dial { address: Multiaddr },
    /// Send MPC aux protocol message (Outgoing from aux party to network)
    SendAuxMessage(round_based::Outgoing<crate::mpc::AuxMsg>),
    /// Send MPC keygen protocol message (Outgoing from keygen party to network)
    SendKeygenMessage(round_based::Outgoing<crate::mpc::ThresholdMessage>),
    /// Send MPC signing protocol message (Outgoing from signing party to network)
    SendSigningMessage(round_based::Outgoing<crate::mpc::SigningMessage>),
}

#[allow(unused)]
pub struct Networker {
    /// Addresses of our node reachable by other swarm nodes
    pub local_addresses: Arc<Mutex<Vec<Multiaddr>>>,
    /// Counter of connected peers
    pub peers_count: Arc<AtomicUsize>,
    /// libp2p Swarm network
    networking: Swarm<LocalNetworkBehaviour>,
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "NetworkEvent")]
struct LocalNetworkBehaviour {
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    gossipsub: Gossipsub,
    kad: Kademlia<MemoryStore>,
}

#[derive(Debug)]
enum NetworkEvent {
    Identify(identify::Event),
    Ping(ping::Event),
    Gossipsub(GossipsubEvent),
    Kad(KademliaEvent),
}

impl From<GossipsubEvent> for NetworkEvent {
    fn from(event: GossipsubEvent) -> Self {
        NetworkEvent::Gossipsub(event)
    }
}

impl From<KademliaEvent> for NetworkEvent {
    fn from(event: KademliaEvent) -> Self {
        NetworkEvent::Kad(event)
    }
}

impl From<ping::Event> for NetworkEvent {
    fn from(event: ping::Event) -> Self {
        NetworkEvent::Ping(event)
    }
}

impl From<identify::Event> for NetworkEvent {
    fn from(event: identify::Event) -> Self {
        NetworkEvent::Identify(event)
    }
}

///builds messaging protocol based on gossip
pub fn build_gossip(local_key: identity::Keypair) -> std::io::Result<Gossipsub> {
    let message_id_fn = |message: &GossipsubMessage| {
        let mut s = DefaultHasher::new();
        message.data.hash(&mut s);
        MessageId::from(s.finish().to_string())
    };

    // Set a custom gossipsub
    let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
        // This is set to aid debugging by not cluttering the log space
        .heartbeat_interval(Duration::from_secs(2))
        .max_transmit_size(65536 * 10000)
        .message_id_fn(message_id_fn)
        .build()
        .expect("Valid config");

    let gossipsub: Gossipsub =
        gossipsub::Gossipsub::new(MessageAuthenticity::Signed(local_key), gossipsub_config)
            .expect("Correct configuration");

    Ok(gossipsub)
}

///builds kademlia behaviour to be use in swarm
pub fn build_kademlia(peer_id: PeerId) -> Kademlia<MemoryStore> {
    let store = MemoryStore::new(peer_id);
    let mut kad_config = KademliaConfig::default();
    kad_config.set_protocol_names(gossip_protocol_name());
    kad_config.set_query_timeout(Duration::from_secs(300));
    kad_config.set_record_filtering(KademliaStoreInserts::FilterBoth);
    // set disjoint_query_paths to true. Ref: https://discuss.libp2p.io/t/s-kademlia-lookups-over-disjoint-paths-in-rust-libp2p/571
    kad_config.disjoint_query_paths(true);
    let kademlia = Kademlia::with_config(peer_id, store, kad_config);
    kademlia
}

///builds ping behaviour to be use in swarm
pub fn build_ping() -> ping::Behaviour {
    let config = ping::Config::new().with_interval(Duration::from_secs(2)); // Ping every 30 seconds to keep connection alive
    ping::Behaviour::new(config)
}

///builds kademlia behaviour to be use in swarm
pub fn build_identify(local_public_key: identity::PublicKey) -> identify::Behaviour {
    identify::Behaviour::new(identify::Config::new(
        "/felix/id/0.0.1".into(),
        local_public_key,
    ))
}

pub async fn create_tcp_transport(
    local_key_pair: identity::Keypair,
) -> Boxed<(PeerId, StreamMuxerBox)> {
    let transport = {
        let dns_tcp = libp2p::dns::DnsConfig::system(libp2p::tcp::TcpTransport::new(
            libp2p::tcp::GenTcpConfig::new().nodelay(true),
        ))
        .await
        .unwrap();
        let ws_dns_tcp = libp2p::websocket::WsConfig::new(
            libp2p::dns::DnsConfig::system(libp2p::tcp::TcpTransport::new(
                libp2p::tcp::GenTcpConfig::new().nodelay(true),
            ))
            .await
            .unwrap(),
        );
        /*
        Adds a fallback transport that is used when encountering errors while establishing inbound or outbound connections.
        The returned transport will act like self, except that if listen_on or dial return an error then other will be tried.
         */
        dns_tcp.or_transport(ws_dns_tcp)
    };
    transport
        .upgrade(Version::V1)
        .authenticate(libp2p::noise::NoiseAuthenticated::xx(&local_key_pair).unwrap())
        .multiplex(SelectUpgrade::new(
            libp2p::yamux::YamuxConfig::default(),
            MplexConfig::default(),
        ))
        .timeout(Duration::from_secs(20))
        .boxed()
}

// TODO: Event handler
pub struct Network {
    pub id: PeerId,
    pub port: u16,
    pub peers: Arc<AtomicUsize>,
    #[allow(unused)]
    node_keys: Keypair,
    swarm: Arc<Swarm<LocalNetworkBehaviour>>,
    bootstrapped: Arc<AtomicBool>,
    command_receiver: mpsc::UnboundedReceiver<NetworkCommand>,
    command_sender: mpsc::UnboundedSender<NetworkCommand>,
    // Shared state with RPC
    peer_list: Arc<TokioRwLock<Vec<PeerId>>>,
    listen_addresses: Arc<TokioRwLock<Vec<Multiaddr>>>,
    // Session tracking
    sessions: Arc<TokioRwLock<HashMap<String, crate::rpc::SessionInfo>>>,
    current_session_id: Arc<TokioRwLock<Option<String>>>,
    // MPC Orchestrator
    mpc_orchestrator: Arc<TokioRwLock<Option<MPCOrchestrator>>>,
}

impl Network {
    pub async fn new(
        id: PeerId,
        port: u16,
        node_keys: Keypair,
        peer_list: Arc<TokioRwLock<Vec<PeerId>>>,
        listen_addresses: Arc<TokioRwLock<Vec<Multiaddr>>>,
        sessions: Arc<TokioRwLock<HashMap<String, crate::rpc::SessionInfo>>>,
        current_session_id: Arc<TokioRwLock<Option<String>>>,
    ) -> (
        Box<Self>,
        mpsc::UnboundedSender<NetworkCommand>,
        Arc<TokioRwLock<Option<MPCOrchestrator>>>,
    ) {
        let transport = create_tcp_transport(node_keys.clone()).await;
        let behaviour = LocalNetworkBehaviour {
            gossipsub: build_gossip(node_keys.clone()).expect("Incorrect keys provided"),
            kad: build_kademlia(id.clone()),
            identify: build_identify(node_keys.public().clone()),
            ping: build_ping(),
        };
        let swarm = Arc::new(
            SwarmBuilder::new(transport, behaviour, id.clone())
                .executor(Box::new(|fut| {
                    tokio::spawn(fut);
                }))
                .build(),
        );

        // Create command channel
        let (command_sender, command_receiver) = mpsc::unbounded_channel();

        // Create shared mpc_orchestrator
        let mpc_orchestrator = Arc::new(TokioRwLock::new(None));

        let network = Box::new(Network {
            id,
            port,
            peers: Arc::new(AtomicUsize::new(0)),
            node_keys,
            swarm,
            bootstrapped: Arc::new(AtomicBool::new(false)),
            command_receiver,
            command_sender: command_sender.clone(),
            peer_list,
            listen_addresses,
            sessions,
            current_session_id,
            mpc_orchestrator: mpc_orchestrator.clone(),
        });

        (network, command_sender, mpc_orchestrator)
    }

    /// Returns `true` if number of all swarm's gossip_sub peers greater or equal to target
    /// # Parameters
    /// * target - number of nodes we expect at least in the gossip sub network
    #[allow(unused)]
    pub fn have_sufficient_peers(&self, target: usize) -> bool {
        self.peers.load(Ordering::Relaxed) >= target
    }

    /// Handle assignment messages (both requests and acceptances)
    async fn handle_assignment_message(
        local_peer_id: &PeerId,
        assignment_msg: AssignmentMessage,
        swarm: &mut Swarm<LocalNetworkBehaviour>,
        sessions: &Arc<TokioRwLock<HashMap<String, crate::rpc::SessionInfo>>>,
        current_session_id: &Arc<TokioRwLock<Option<String>>>,
        mpc_orchestrator: &Arc<TokioRwLock<Option<MPCOrchestrator>>>,
        network_command_sender: &mpsc::UnboundedSender<NetworkCommand>,
    ) {
        match assignment_msg {
            AssignmentMessage::Request {
                coordinator_id,
                assignments,
                session_id,
            } => {
                // Check if we're in the assignment list
                let my_id = local_peer_id.to_string();
                if let Some(&assigned_index) = assignments.get(&my_id) {
                    // Update current session ID to track which session we're participating in
                    {
                        let mut current = current_session_id.write().await;
                        *current = Some(session_id.clone());
                    }

                    // Store session info locally
                    {
                        let mut sessions_map = sessions.write().await;
                        if !sessions_map.contains_key(&session_id) {
                            let session_info = crate::rpc::SessionInfo::new(
                                session_id.clone(),
                                coordinator_id.clone(),
                                assignments.clone(),
                            );
                            sessions_map.insert(session_id.clone(), session_info);
                        }
                    }

                    // Check if we're the coordinator
                    if coordinator_id == my_id {
                        tracing::info!(
                            target:"GossipNode",
                            "🎯 Coordinator confirmed own assignment: index {} (total nodes: {})",
                            assigned_index,
                            assignments.len()
                        );
                        // Coordinator doesn't need to send acceptance to itself
                        return;
                    }

                    tracing::info!(
                        target:"GossipNode",
                        "🎯 Received assignment from coordinator {}: index {}",
                        coordinator_id,
                        assigned_index
                    );

                    // Send acceptance
                    let acceptance = AssignmentMessage::Acceptance {
                        peer_id: my_id,
                        assigned_index,
                        session_id: session_id.clone(),
                    };

                    match serde_json::to_string(&acceptance) {
                        Ok(json) => {
                            let topic = Sha256Topic::new("mpc-assignment");
                            match swarm
                                .behaviour_mut()
                                .gossipsub
                                .publish(topic, json.into_bytes())
                            {
                                Ok(_) => {
                                    tracing::info!(
                                        target:"GossipNode",
                                        "✅ Sent acceptance for index {} to coordinator",
                                        assigned_index
                                    );

                                    // Initialize MPC Orchestrator after accepting assignment
                                    // Convert String peer IDs to PeerId and usize indices to u16
                                    let mut party_assignments: HashMap<PeerId, u16> =
                                        HashMap::new();
                                    let mut participants: Vec<PeerId> = Vec::new();

                                    for (peer_id_str, &index) in &assignments {
                                        match peer_id_str.parse::<PeerId>() {
                                            Ok(peer_id) => {
                                                party_assignments.insert(peer_id, index as u16);
                                                participants.push(peer_id);
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    target:"GossipNode",
                                                    "Failed to parse peer ID '{}': {}",
                                                    peer_id_str,
                                                    e
                                                );
                                            }
                                        }
                                    }

                                    // Sort participants for deterministic ordering
                                    participants.sort();

                                    // Create orchestrator with default 5-minute timeout
                                    let orchestrator = MPCOrchestrator::new(
                                        *local_peer_id,
                                        participants.clone(),
                                        party_assignments.clone(),
                                        Duration::from_secs(300),
                                        network_command_sender.clone(),
                                    );

                                    // Store orchestrator
                                    let mut orch_lock = mpc_orchestrator.write().await;
                                    *orch_lock = Some(orchestrator);

                                    tracing::info!(
                                        target:"GossipNode",
                                        "✅ Non-coordinator initialized MPC Orchestrator with {} participants (state: Idle)",
                                        participants.len()
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        target:"GossipNode",
                                        "Failed to send acceptance: {:?}",
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(target:"GossipNode", "Failed to serialize acceptance: {}", e);
                        }
                    }
                } else {
                    tracing::warn!(
                        target:"GossipNode",
                        "Received assignment but our peer ID not included"
                    );
                }
            }
            AssignmentMessage::Acceptance {
                peer_id,
                assigned_index,
                session_id,
            } => {
                // Update session info with acceptance
                let mut sessions_map = sessions.write().await;
                if let Some(session) = sessions_map.get_mut(&session_id) {
                    session.acceptances.insert(peer_id.clone(), true);

                    let acceptance_count = session.acceptance_count();
                    let total_expected = session.total_nodes - 1; // Coordinator doesn't send acceptance to itself

                    tracing::info!(
                        target:"GossipNode",
                        "✅ Received acceptance from peer {} for index {} (session: {}) - Progress: {}/{}",
                        peer_id,
                        assigned_index,
                        session_id,
                        acceptance_count,
                        total_expected
                    );

                    if session.all_accepted() {
                        tracing::info!(
                            target:"GossipNode",
                            "🎉 All peers accepted their assignments for session {}!",
                            session_id
                        );

                        // Clone assignments before dropping the lock
                        let assignments_clone = session.assignments.clone();

                        // Initialize MPC Orchestrator after all assignments are accepted
                        drop(sessions_map); // Release lock before async operation

                        // Convert String peer IDs to PeerId and usize indices to u16
                        let mut party_assignments: HashMap<PeerId, u16> = HashMap::new();
                        let mut participants: Vec<PeerId> = Vec::new();

                        for (peer_id_str, &index) in &assignments_clone {
                            match peer_id_str.parse::<PeerId>() {
                                Ok(peer_id) => {
                                    party_assignments.insert(peer_id, index as u16);
                                    participants.push(peer_id);
                                }
                                Err(e) => {
                                    tracing::error!(
                                        target:"GossipNode",
                                        "Failed to parse peer ID '{}': {}",
                                        peer_id_str,
                                        e
                                    );
                                }
                            }
                        }

                        // Sort participants for deterministic ordering
                        participants.sort();

                        // Create orchestrator with default 5-minute timeout
                        let orchestrator = MPCOrchestrator::new(
                            *local_peer_id,
                            participants.clone(),
                            party_assignments.clone(),
                            Duration::from_secs(300),
                            network_command_sender.clone(),
                        );

                        // Store orchestrator
                        let mut orch_lock = mpc_orchestrator.write().await;
                        *orch_lock = Some(orchestrator);

                        tracing::info!(
                            target:"GossipNode",
                            "✅ Initialized MPC Orchestrator with {} participants (state: Idle)",
                            participants.len()
                        );
                    }
                } else {
                    tracing::warn!(
                        target:"GossipNode",
                        "Received acceptance for unknown session: {}",
                        session_id
                    );
                }
            }
        }
    }

    pub async fn run(
        mut self,
        topics: impl AsRef<[String]>,
        seed_nodes: impl AsRef<[String]>,
        boot_nodes: impl AsRef<[String]>,
        explicit_peer: Option<&str>,
    ) -> anyhow::Result<()> {
        tracing::info!(target:"GossipNode","Our id: {}", &self.id.to_string());
        let swarm = Arc::get_mut(&mut self.swarm).expect("Failed to get mutable swarm");

        // Always subscribe to the assignment and coordination topics
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&Sha256Topic::new(MPC_ASSIGNMENT_TOPIC))?;
        tracing::info!(target:"GossipNode","Subscribed to mpc-assignment topic");

        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&Sha256Topic::new(MPC_COORDINATION_TOPIC))?;
        tracing::info!(target:"GossipNode","Subscribed to mpc-coordination topic");

        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&Sha256Topic::new("mpc-aux-protocol"))?;
        tracing::info!(target:"GossipNode","Subscribed to mpc-aux-protocol topic");

        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&Sha256Topic::new("mpc-keygen-protocol"))?;
        tracing::info!(target:"GossipNode","Subscribed to mpc-keygen-protocol topic");

        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&Sha256Topic::new("mpc-signing-protocol"))?;
        tracing::info!(target:"GossipNode","Subscribed to mpc-signing-protocol topic");

        for topic in topics.as_ref() {
            swarm
                .behaviour_mut()
                .gossipsub
                .subscribe(&Sha256Topic::new(topic))?;
        }
        if let Some(ep) = explicit_peer {
            match ep.parse() {
                Ok(id) => swarm.behaviour_mut().gossipsub.add_explicit_peer(&id),
                Err(e) => return Err(Error::msg(e.to_string())),
            }
        }
        let str_port = self.port.to_string();
        swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{str_port}").parse()?)?;

        for node in seed_nodes.as_ref() {
            swarm.dial(Multiaddr::try_from(node.as_ref())?)?;
        }

        for boot_node in boot_nodes.as_ref() {
            let mut address = Multiaddr::try_from(boot_node.as_ref())?;
            let peer_id = match address.pop() {
                Some(Protocol::P2p(hash)) => match PeerId::from_multihash(hash) {
                    Ok(id) => id,
                    // TODO: verify this logic
                    Err(_) => continue,
                },
                // TODO: verify this logic
                _ => continue,
            };
            // inject
            swarm
                .behaviour_mut()
                .kad
                .add_address(&peer_id, address.clone());
            swarm.dial(address)?;
            self.bootstrapped.store(true, Ordering::Relaxed);
        }
        if self.bootstrapped.load(Ordering::Relaxed) {
            if let Err(e) = swarm.behaviour_mut().kad.bootstrap() {
                tracing::warn!("Failed to bootstrap Kademlia: {:?}", e);
            } else {
                tracing::info!("Kademlia bootstrap initiated");
            }
        }
        'event_loop: loop {
            tokio::select! {
                // Handle commands from the command channel
                Some(command) = self.command_receiver.recv() => {
                    match command {
                        NetworkCommand::Broadcast { topic, data } => {
                            let topic = Sha256Topic::new(topic);
                            match swarm.behaviour_mut().gossipsub.publish(topic.clone(), data) {
                                Ok(message_id) => {
                                    tracing::info!(target:"GossipNode", "Published message {:?} to topic: {}", message_id, topic);
                                }
                                Err(e) => {
                                    tracing::error!(target:"GossipNode", "Failed to publish message to topic {}: {:?}", topic, e);
                                }
                            }
                        }
                        NetworkCommand::Subscribe { topic } => {
                            let topic = Sha256Topic::new(topic);
                            match swarm.behaviour_mut().gossipsub.subscribe(&topic) {
                                Ok(_) => {
                                    tracing::info!(target:"GossipNode", "Subscribed to topic: {}", topic);
                                }
                                Err(e) => {
                                    tracing::error!(target:"GossipNode", "Failed to subscribe to topic {}: {:?}", topic, e);
                                }
                            }
                        }
                        NetworkCommand::Unsubscribe { topic } => {
                            let topic = Sha256Topic::new(topic);
                            match swarm.behaviour_mut().gossipsub.unsubscribe(&topic) {
                                Ok(_) => {
                                    tracing::info!(target:"GossipNode", "Unsubscribed from topic: {}", topic);
                                }
                                Err(e) => {
                                    tracing::error!(target:"GossipNode", "Failed to unsubscribe from topic {}: {:?}", topic, e);
                                }
                            }
                        }
                        NetworkCommand::Dial { address } => {
                            match swarm.dial(address.clone()) {
                                Ok(_) => {
                                    tracing::info!(target:"GossipNode", "Dialing peer at: {}", address);
                                }
                                Err(e) => {
                                    tracing::error!(target:"GossipNode", "Failed to dial {}: {:?}", address, e);
                                }
                            }
                        }
                        NetworkCommand::SendAuxMessage(outgoing_msg) => {
                            // Serialize the inner message
                            let payload = match bincode::serialize(&outgoing_msg.msg) {
                                Ok(bytes) => bytes,
                                Err(e) => {
                                    tracing::error!(
                                        target:"GossipNode",
                                        "Failed to serialize aux message payload: {}",
                                        e
                                    );
                                    continue 'event_loop;
                                }
                            };

                            // Determine message type and recipient
                            let (msg_type, to) = match outgoing_msg.recipient {
                                round_based::MessageDestination::AllParties => {
                                    ("broadcast".to_string(), None)
                                }
                                round_based::MessageDestination::OneParty(party_idx) => {
                                    ("p2p".to_string(), Some(party_idx))
                                }
                            };

                            // Wrap in a structure with sender and optional recipient
                            let wrapped_msg = MpcAuxMessage {
                                from: self.id,
                                to,
                                msg_type: msg_type.clone(),
                                payload,
                            };

                            // Serialize and broadcast via gossipsub
                            match serde_json::to_string(&wrapped_msg) {
                                Ok(json) => {
                                    let topic = Sha256Topic::new("mpc-aux-protocol");
                                    match swarm.behaviour_mut().gossipsub.publish(topic.clone(), json.into_bytes()) {
                                        Ok(_) => {
                                            tracing::info!(
                                                target:"GossipNode",
                                                "📤 Broadcasted aux message (type: {}, to: {:?})",
                                                msg_type,
                                                wrapped_msg.to
                                            );
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                target:"GossipNode",
                                                "Failed to broadcast aux message: {:?}",
                                                e
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        target:"GossipNode",
                                        "Failed to serialize wrapped aux message: {}",
                                        e
                                    );
                                }
                            }
                        }
                        NetworkCommand::SendKeygenMessage(outgoing_msg) => {
                            // Serialize the inner message
                            let payload = match bincode::serialize(&outgoing_msg.msg) {
                                Ok(bytes) => bytes,
                                Err(e) => {
                                    tracing::error!(
                                        target:"GossipNode",
                                        "Failed to serialize keygen message payload: {}",
                                        e
                                    );
                                    continue 'event_loop;
                                }
                            };

                            // Determine message type and recipient
                            let (msg_type, to) = match outgoing_msg.recipient {
                                round_based::MessageDestination::AllParties => {
                                    ("broadcast".to_string(), None)
                                }
                                round_based::MessageDestination::OneParty(party_idx) => {
                                    ("p2p".to_string(), Some(party_idx))
                                }
                            };

                            // Wrap in a structure with sender and optional recipient
                            let wrapped_msg = MpcAuxMessage {
                                from: self.id,
                                to,
                                msg_type: msg_type.clone(),
                                payload,
                            };

                            // Serialize and broadcast via gossipsub
                            match serde_json::to_string(&wrapped_msg) {
                                Ok(json) => {
                                    let topic = Sha256Topic::new("mpc-keygen-protocol");
                                    match swarm.behaviour_mut().gossipsub.publish(topic.clone(), json.into_bytes()) {
                                        Ok(_) => {
                                            tracing::info!(
                                                target:"GossipNode",
                                                "📤 Broadcasted keygen message (type: {}, to: {:?})",
                                                msg_type,
                                                wrapped_msg.to
                                            );
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                target:"GossipNode",
                                                "Failed to broadcast keygen message: {:?}",
                                                e
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        target:"GossipNode",
                                        "Failed to serialize wrapped keygen message: {}",
                                        e
                                    );
                                }
                            }
                        }
                        NetworkCommand::SendSigningMessage(outgoing_msg) => {
                            // Serialize the inner message
                            let payload = match bincode::serialize(&outgoing_msg.msg) {
                                Ok(bytes) => bytes,
                                Err(e) => {
                                    tracing::error!(
                                        target:"GossipNode",
                                        "Failed to serialize signing message payload: {}",
                                        e
                                    );
                                    continue 'event_loop;
                                }
                            };

                            // Determine message type and recipient
                            let (msg_type, to) = match outgoing_msg.recipient {
                                round_based::MessageDestination::AllParties => {
                                    ("broadcast".to_string(), None)
                                }
                                round_based::MessageDestination::OneParty(party_idx) => {
                                    ("p2p".to_string(), Some(party_idx))
                                }
                            };

                            // Wrap in a structure with sender and optional recipient
                            let wrapped_msg = MpcAuxMessage {
                                from: self.id,
                                to,
                                msg_type: msg_type.clone(),
                                payload,
                            };

                            // Serialize and broadcast via gossipsub
                            match serde_json::to_string(&wrapped_msg) {
                                Ok(json) => {
                                    let topic = Sha256Topic::new("mpc-signing-protocol");
                                    match swarm.behaviour_mut().gossipsub.publish(topic.clone(), json.into_bytes()) {
                                        Ok(_) => {
                                            tracing::info!(
                                                target:"GossipNode",
                                                "📤 Broadcasted signing message (type: {}, to: {:?})",
                                                msg_type,
                                                wrapped_msg.to
                                            );
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                target:"GossipNode",
                                                "Failed to broadcast signing message: {:?}",
                                                e
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        target:"GossipNode",
                                        "Failed to serialize wrapped signing message: {}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
                event = swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(NetworkEvent::Gossipsub(gossipsub_event)) => {
                        match gossipsub_event {
                            GossipsubEvent::Message {
                                propagation_source: _,
                                message_id,
                                message,
                            } => {
                                // Filter out our own messages
                                if message.source.as_ref() == Some(&self.id) {
                                    tracing::debug!(target:"GossipNode", "Ignoring our own message: {:?}", message_id);
                                } else {
                                    let message_str = String::from_utf8_lossy(&message.data);

                                    // Check if this is an assignment message
                                    let assignment_topic = Sha256Topic::new(MPC_ASSIGNMENT_TOPIC);
                                    let coordination_topic = Sha256Topic::new(MPC_COORDINATION_TOPIC);

                                    if message.topic == assignment_topic.hash() {
                                        match serde_json::from_str::<AssignmentMessage>(&message_str) {
                                            Ok(assignment_msg) => {
                                                Self::handle_assignment_message(&self.id, assignment_msg, swarm, &self.sessions, &self.current_session_id, &self.mpc_orchestrator, &self.command_sender).await;
                                            }
                                            Err(e) => {
                                                tracing::warn!(target:"GossipNode", "Failed to parse assignment message: {}", e);
                                            }
                                        }
                                    } else if message.topic == coordination_topic.hash() {
                                        // Handle MPC coordination messages inline to avoid borrow conflicts
                                        match serde_json::from_str::<CoordinationMessage>(&message_str) {
                                            Ok(coord_msg) => {
                                                if let Some(source) = message.source {
                                                    tracing::info!(
                                                        target:"GossipNode",
                                                        "📡 Received MPC coordination message from {}: {:?}",
                                                        source,
                                                        coord_msg
                                                    );

                                                    // Get mutable access to orchestrator
                                                    let mut orchestrator_lock = self.mpc_orchestrator.write().await;
                                                    if let Some(orchestrator) = orchestrator_lock.as_mut() {
                                                        if let Err(e) = orchestrator.handle_coordination_message(source, coord_msg).await {
                                                            tracing::error!(
                                                                target:"GossipNode",
                                                                "Failed to handle MPC coordination message from {}: {:?}",
                                                                source,
                                                                e
                                                            );
                                                        }
                                                    } else {
                                                        tracing::warn!(
                                                            target:"GossipNode",
                                                            "Received MPC coordination message but orchestrator not initialized"
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(target:"GossipNode", "Failed to parse MPC coordination message: {}", e);
                                            }
                                        }
                                    } else if message.topic == Sha256Topic::new("mpc-aux-protocol").hash() {
                                        // Handle MPC aux protocol messages
                                        match serde_json::from_str::<MpcAuxMessage>(&message_str) {
                                            Ok(aux_msg) => {
                                                tracing::info!(
                                                    target:"GossipNode",
                                                    "📦 Received aux message from {} (type: {}, to: {:?})",
                                                    aux_msg.from,
                                                    aux_msg.msg_type,
                                                    aux_msg.to
                                                );

                                                // Get orchestrator to access party assignments and aux_incoming_tx
                                                let orchestrator_lock = self.mpc_orchestrator.read().await;
                                                if let Some(orchestrator) = orchestrator_lock.as_ref() {
                                                    // Filter P2P messages by recipient
                                                    if let Some(target_party_idx) = aux_msg.to {
                                                        // This is a P2P message - check if it's for us
                                                        let our_party_idx = orchestrator.party_assignments.get(&self.id);

                                                        if our_party_idx != Some(&target_party_idx) {
                                                            // Not for us, skip
                                                            tracing::trace!(
                                                                target:"GossipNode",
                                                                "Skipping P2P message not meant for us (target: {}, our_idx: {:?})",
                                                                target_party_idx,
                                                                our_party_idx
                                                            );
                                                            continue 'event_loop;
                                                        }
                                                    }

                                                    // Deserialize inner AuxMsg payload
                                                    match bincode::deserialize::<crate::mpc::AuxMsg>(&aux_msg.payload) {
                                                        Ok(inner_msg) => {
                                                            // Get sender's party index
                                                            let sender_party_idx = orchestrator.party_assignments.get(&aux_msg.from)
                                                                .copied();

                                                            if sender_party_idx.is_none() {
                                                                tracing::warn!(
                                                                    target:"GossipNode",
                                                                    "Received aux message from unknown peer: {}",
                                                                    aux_msg.from
                                                                );
                                                                continue 'event_loop;
                                                            }

                                                            // Create Incoming message
                                                            let incoming = round_based::Incoming {
                                                                id: 0, // This is hardcoded for now, TODO: It is used for de-duplication
                                                                sender: sender_party_idx.unwrap(),
                                                                msg_type: if aux_msg.msg_type == "broadcast" {
                                                                    round_based::MessageType::Broadcast
                                                                } else {
                                                                    round_based::MessageType::P2P
                                                                },
                                                                msg: inner_msg,
                                                            };

                                                            // Forward to aux_incoming_tx if available
                                                            if let Some(aux_tx) = &orchestrator.aux_incoming_tx {
                                                                if let Err(e) = aux_tx.send(incoming) {
                                                                    tracing::error!(
                                                                        target:"GossipNode",
                                                                        "Failed to forward aux message to aux task: {:?}",
                                                                        e
                                                                    );
                                                                } else {
                                                                    tracing::debug!(
                                                                        target:"GossipNode",
                                                                        "✅ Forwarded aux message from party {} to aux task",
                                                                        sender_party_idx.unwrap()
                                                                    );
                                                                }
                                                            } else {
                                                                tracing::warn!(
                                                                    target:"GossipNode",
                                                                    "Received aux message but aux_incoming_tx not initialized"
                                                                );
                                                            }
                                                        }
                                                        Err(e) => {
                                                            tracing::error!(
                                                                target:"GossipNode",
                                                                "Failed to deserialize aux message payload: {}",
                                                                e
                                                            );
                                                        }
                                                    }
                                                } else {
                                                    tracing::warn!(
                                                        target:"GossipNode",
                                                        "Received aux message but orchestrator not initialized"
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    target:"GossipNode",
                                                    "Failed to parse MPC aux message: {}",
                                                    e
                                                );
                                            }
                                        }
                                    } else if message.topic == Sha256Topic::new("mpc-keygen-protocol").hash() {
                                        // Handle MPC keygen protocol messages
                                        match serde_json::from_str::<MpcAuxMessage>(&message_str) {
                                            Ok(keygen_msg) => {
                                                tracing::info!(
                                                    target:"GossipNode",
                                                    "🔑 Received keygen message from {} (type: {}, to: {:?})",
                                                    keygen_msg.from,
                                                    keygen_msg.msg_type,
                                                    keygen_msg.to
                                                );

                                                // Get orchestrator to access party assignments and keygen_incoming_tx
                                                let orchestrator_lock = self.mpc_orchestrator.read().await;
                                                if let Some(orchestrator) = orchestrator_lock.as_ref() {
                                                    // Filter P2P messages by recipient
                                                    if let Some(target_party_idx) = keygen_msg.to {
                                                        // This is a P2P message - check if it's for us
                                                        let our_party_idx = orchestrator.party_assignments.get(&self.id);

                                                        if our_party_idx != Some(&target_party_idx) {
                                                            // Not for us, skip
                                                            tracing::trace!(
                                                                target:"GossipNode",
                                                                "Skipping P2P keygen message not meant for us (target: {}, our_idx: {:?})",
                                                                target_party_idx,
                                                                our_party_idx
                                                            );
                                                            continue 'event_loop;
                                                        }
                                                    }

                                                    // Deserialize inner ThresholdMessage payload
                                                    match bincode::deserialize::<crate::mpc::ThresholdMessage>(&keygen_msg.payload) {
                                                        Ok(inner_msg) => {
                                                            // Get sender's party index
                                                            let sender_party_idx = orchestrator.party_assignments.get(&keygen_msg.from)
                                                                .copied();

                                                            if sender_party_idx.is_none() {
                                                                tracing::warn!(
                                                                    target:"GossipNode",
                                                                    "Received keygen message from unknown peer: {}",
                                                                    keygen_msg.from
                                                                );
                                                                continue 'event_loop;
                                                            }

                                                            // Create Incoming message
                                                            let incoming = round_based::Incoming {
                                                                id: 0, // This is hardcoded for now, TODO: It is used for de-duplication
                                                                sender: sender_party_idx.unwrap(),
                                                                msg_type: if keygen_msg.msg_type == "broadcast" {
                                                                    round_based::MessageType::Broadcast
                                                                } else {
                                                                    round_based::MessageType::P2P
                                                                },
                                                                msg: inner_msg,
                                                            };

                                                            // Forward to keygen_incoming_tx if available
                                                            if let Some(keygen_tx) = &orchestrator.keygen_incoming_tx {
                                                                if let Err(e) = keygen_tx.send(incoming) {
                                                                    tracing::error!(
                                                                        target:"GossipNode",
                                                                        "Failed to forward keygen message to keygen task: {:?}",
                                                                        e
                                                                    );
                                                                } else {
                                                                    tracing::debug!(
                                                                        target:"GossipNode",
                                                                        "✅ Forwarded keygen message from party {} to keygen task",
                                                                        sender_party_idx.unwrap()
                                                                    );
                                                                }
                                                            } else {
                                                                tracing::warn!(
                                                                    target:"GossipNode",
                                                                    "Received keygen message but keygen_incoming_tx not initialized"
                                                                );
                                                            }
                                                        }
                                                        Err(e) => {
                                                            tracing::error!(
                                                                target:"GossipNode",
                                                                "Failed to deserialize keygen message payload: {}",
                                                                e
                                                            );
                                                        }
                                                    }
                                                } else {
                                                    tracing::warn!(
                                                        target:"GossipNode",
                                                        "Received keygen message but orchestrator not initialized"
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    target:"GossipNode",
                                                    "Failed to parse MPC keygen message: {}",
                                                    e
                                                );
                                            }
                                        }
                                    } else if message.topic == Sha256Topic::new("mpc-signing-protocol").hash() {
                                        // Handle MPC signing protocol messages
                                        match serde_json::from_str::<MpcAuxMessage>(&message_str) {
                                            Ok(signing_msg) => {
                                                tracing::info!(
                                                    target:"GossipNode",
                                                    "✍️  Received signing message from {} (type: {}, to: {:?})",
                                                    signing_msg.from,
                                                    signing_msg.msg_type,
                                                    signing_msg.to
                                                );

                                                // Get orchestrator to access party assignments and signing_incoming_tx
                                                let orchestrator_lock = self.mpc_orchestrator.read().await;
                                                if let Some(orchestrator) = orchestrator_lock.as_ref() {
                                                    // Filter P2P messages by recipient
                                                    if let Some(target_party_idx) = signing_msg.to {
                                                        // This is a P2P message - check if it's for us
                                                        let our_party_idx = orchestrator.party_assignments.get(&self.id);

                                                        if our_party_idx != Some(&target_party_idx) {
                                                            // Not for us, skip
                                                            tracing::trace!(
                                                                target:"GossipNode",
                                                                "Skipping P2P signing message not meant for us (target: {}, our_idx: {:?})",
                                                                target_party_idx,
                                                                our_party_idx
                                                            );
                                                            continue 'event_loop;
                                                        }
                                                    }

                                                    // Deserialize inner SigningMessage payload
                                                    match bincode::deserialize::<crate::mpc::SigningMessage>(&signing_msg.payload) {
                                                        Ok(inner_msg) => {
                                                            // Get sender's party index
                                                            let sender_party_idx = orchestrator.party_assignments.get(&signing_msg.from)
                                                                .copied();

                                                            if sender_party_idx.is_none() {
                                                                tracing::warn!(
                                                                    target:"GossipNode",
                                                                    "Received signing message from unknown peer: {}",
                                                                    signing_msg.from
                                                                );
                                                                continue 'event_loop;
                                                            }

                                                            // Create Incoming message
                                                            let incoming = round_based::Incoming {
                                                                id: 0, // This is hardcoded for now, TODO: It is used for de-duplication
                                                                sender: sender_party_idx.unwrap(),
                                                                msg_type: if signing_msg.msg_type == "broadcast" {
                                                                    round_based::MessageType::Broadcast
                                                                } else {
                                                                    round_based::MessageType::P2P
                                                                },
                                                                msg: inner_msg,
                                                            };

                                                            // Forward to signing_incoming_tx if available
                                                            if let Some(signing_tx) = &orchestrator.signing_incoming_tx {
                                                                if let Err(e) = signing_tx.send(incoming) {
                                                                    tracing::error!(
                                                                        target:"GossipNode",
                                                                        "Failed to forward signing message to signing task: {:?}",
                                                                        e
                                                                    );
                                                                } else {
                                                                    tracing::debug!(
                                                                        target:"GossipNode",
                                                                        "✅ Forwarded signing message from party {} to signing task",
                                                                        sender_party_idx.unwrap()
                                                                    );
                                                                }
                                                            } else {
                                                                tracing::warn!(
                                                                    target:"GossipNode",
                                                                    "Received signing message but signing_incoming_tx not initialized"
                                                                );
                                                            }
                                                        }
                                                        Err(e) => {
                                                            tracing::error!(
                                                                target:"GossipNode",
                                                                "Failed to deserialize signing message payload: {}",
                                                                e
                                                            );
                                                        }
                                                    }
                                                } else {
                                                    tracing::warn!(
                                                        target:"GossipNode",
                                                        "Received signing message but orchestrator not initialized"
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    target:"GossipNode",
                                                    "Failed to parse MPC signing message: {}",
                                                    e
                                                );
                                            }
                                        }
                                    } else {
                                        // Log regular received message
                                        tracing::info!(
                                            target:"GossipNode",
                                            "📨 Received message from {:?} on topic '{}': \"{}\" ({} bytes)",
                                            message.source,
                                            message.topic,
                                            message_str,
                                            message.data.len()
                                        );
                                    }
                                }
                            }
                            GossipsubEvent::Subscribed { peer_id, topic } => {
                                tracing::info!(target:"GossipNode", "Peer {} subscribed to topic: {}", peer_id, topic);
                            }
                            GossipsubEvent::Unsubscribed { peer_id, topic } => {
                                tracing::info!(target:"GossipNode", "Peer {} unsubscribed from topic: {}", peer_id, topic);
                            }
                            _ => {
                                tracing::debug!(target:"GossipNode", "Other gossipsub event: {:?}", gossipsub_event);
                            }
                        }
                    },
                    SwarmEvent::OutgoingConnectionError {peer_id, ..} => if let Some(pid) = peer_id {
                        tracing::info!(target:"GossipNode","Peer with id {pid} exited.");
                    },
                    SwarmEvent::Behaviour(NetworkEvent::Identify(event)) => match event {
                        identify::Event::Received { peer_id, info } => {
                            // Filter out our own identify events
                            if peer_id == self.id {
                                tracing::debug!(target:"GossipNode","Ignoring identify event from ourselves");
                                continue 'event_loop;
                            }

                            // Add addresses to Kademlia DHT
                            for address in info.listen_addrs {
                                swarm.behaviour_mut().kad.add_address(&peer_id, address);
                            }

                            // Check if this is a new peer in our list
                            let is_new_peer = {
                                let peers = self.peer_list.read().await;
                                !peers.contains(&peer_id)
                            };

                            if is_new_peer {
                                tracing::info!(target:"GossipNode","New peer identification received: {peer_id}");

                                // Increment peer counter only for new peers
                                let new_peers = self.peers.load(Ordering::Relaxed) + 1;
                                self.peers.store(new_peers, Ordering::Relaxed);

                                // Update shared peer list for RPC
                                let mut peers = self.peer_list.write().await;
                                peers.push(peer_id);
                                tracing::info!(target:"GossipNode","Added peer {} to shared peer list", peer_id);

                                // Bootstrap Kademlia if not already done
                                if !self.bootstrapped.load(Ordering::Relaxed) {
                                    if let Err(e) = swarm.behaviour_mut().kad.bootstrap() {
                                        tracing::warn!("Failed to bootstrap Kademlia: {:?}", e);
                                    } else {
                                        self.bootstrapped.store(true, Ordering::Relaxed);
                                        tracing::info!(target: "GossipNode", "Kademlia bootstrap initiated");
                                    }
                                }
                            } else {
                                tracing::debug!(target:"GossipNode","Identify update from known peer: {peer_id}");
                            }
                        },
                        identify::Event::Sent { peer_id } => {
                            tracing::debug!(target:"GossipNode","Sent identify info to peer: {peer_id}");
                        },
                        identify::Event::Pushed { peer_id } => {
                            tracing::debug!(target:"GossipNode","Pushed identify info to peer: {peer_id}");
                        },
                        identify::Event::Error { peer_id, error } => {
                            tracing::warn!(target:"GossipNode","Identify error with peer {peer_id:?}: {error}");
                        },
                    },
                    SwarmEvent::Behaviour(NetworkEvent::Kad(event)) => match event {
                        libp2p::kad::KademliaEvent::InboundRequest { request: _ } => {
                            tracing::debug!(target:"GossipNode", "Kademlia inbound request received");
                        }
                        libp2p::kad::KademliaEvent::OutboundQueryCompleted { id: _, result, stats: _ } => {
                            tracing::debug!(target:"GossipNode", "Kademlia query completed: {:?}", result);

                            // Check if this was a bootstrap query
                            match result {
                                libp2p::kad::QueryResult::Bootstrap(Ok(bootstrap_ok)) => {
                                    tracing::info!(target:"GossipNode",
                                        "Kademlia bootstrap successful! Discovered {} peers",
                                        bootstrap_ok.num_remaining
                                    );
                                }
                                libp2p::kad::QueryResult::Bootstrap(Err(e)) => {
                                    tracing::warn!(target:"GossipNode", "Kademlia bootstrap failed: {:?}", e);
                                }
                                _ => {
                                    tracing::debug!(target:"GossipNode", "Other query completed: {:?}", result);
                                }
                            }
                        }
                        libp2p::kad::KademliaEvent::RoutingUpdated {
                            peer,
                            is_new_peer,
                            old_peer,
                            ..
                        } => {
                            if is_new_peer {
                                tracing::info!(target:"GossipNode", "New peer added to routing table: {}", peer);
                            } else {
                                tracing::debug!(target:"GossipNode", "Routing table updated for peer: {}", peer);
                            }
                            if let Some(evicted) = old_peer {
                                tracing::debug!(target:"GossipNode", "Peer {} evicted from routing table", evicted);
                            }
                        }
                        libp2p::kad::KademliaEvent::UnroutablePeer { peer } => {
                            tracing::debug!(target:"GossipNode", "Unroutable peer (no listen address): {}", peer);
                        }
                        libp2p::kad::KademliaEvent::RoutablePeer { peer, address } => {
                            tracing::debug!(target:"GossipNode", "Routable peer found: {} at {}", peer, address);
                        }
                        libp2p::kad::KademliaEvent::PendingRoutablePeer { peer, address } => {
                            tracing::debug!(target:"GossipNode", "Pending routable peer: {} at {}", peer, address);
                        }
                    },
                    // TODO: Ping event handle
                    SwarmEvent::Behaviour(NetworkEvent::Ping(_event)) => {
                        tracing::debug!(target:"GossipNode","Ping event received");
                    },
                    SwarmEvent::NewListenAddr { address, .. } => {
                        tracing::info!(target:"GossipNode", "Listening on {}", address);
                        // Update shared listen addresses for RPC
                        let mut addrs = self.listen_addresses.write().await;
                        if !addrs.contains(&address) {
                            addrs.push(address);
                        }
                    },
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        tracing::info!(target:"GossipNode", "Connection established with peer: {}", peer_id);
                    },
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        tracing::info!(target:"GossipNode", "Connection closed with peer {}: {:?}", peer_id, cause);

                        // Remove peer from shared list
                        let mut peers = self.peer_list.write().await;
                        peers.retain(|p| p != &peer_id);
                        let count = self.peers.load(Ordering::Relaxed).saturating_sub(1);
                        self.peers.store(count, Ordering::Relaxed);
                        tracing::debug!(target:"GossipNode", "Removed peer {} from shared peer list", peer_id);
                    },
                    // TODO: implement events handling
                    _ => tracing::debug!(target:"GossipNode","Got new event from network"),
                }
            }
        }
    }
}
