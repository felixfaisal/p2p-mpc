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
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, atomic::AtomicUsize};
use std::time::Duration;

pub const PROTOCOL_NAME: &str = "/felix/mpc/0.0.0";
pub fn gossip_protocol_name() -> Vec<Cow<'static, [u8]>> {
    vec![PROTOCOL_NAME.as_bytes().into()]
}

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
        .heartbeat_interval(Duration::from_secs(10))
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
    ping::Behaviour::new(ping::Config::new())
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
    node_keys: Keypair,
    swarm: Arc<Swarm<LocalNetworkBehaviour>>,
}

impl Network {
    pub async fn new(id: PeerId, port: u16, node_keys: Keypair) -> Box<Self> {
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
        Box::new(Network {
            id,
            port,
            peers: Arc::new(AtomicUsize::new(0)),
            node_keys,
            swarm,
        })
    }

    /// Returns `true` if number of all swarm's gossip_sub peers greater or equal to target
    /// # Parameters
    /// * target - number of nodes we expect at least in the gossip sub network
    pub fn have_sufficient_peers(&self, target: usize) -> bool {
        self.peers.load(Ordering::Relaxed) >= target
    }

    pub async fn run(
        mut self,
        topics: impl AsRef<[String]>,
        seed_nodes: impl AsRef<[String]>,
        boot_nodes: impl AsRef<[String]>,
        explicit_peer: Option<&str>,
    ) -> anyhow::Result<()> {
        tracing::info!(target:"GossipNode","Our id: {}", &self.id.to_string());
        let mut swarm = Arc::get_mut(&mut self.swarm).expect("Failed to get mutable swarm");
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
            // self.bootstrapped.store(true, Ordering::Relaxed);
        }
        // if self.bootstrapped.load(Ordering::Relaxed) {
        //     swarm.behaviour_mut().kademlia.bootstrap()?;
        // }
        loop {
            tokio::select! {
                event = swarm.select_next_some() => match event {
                    SwarmEvent::Behaviour(NetworkEvent::Gossipsub(msg)) => {
                        // Handle GossipSub message
                    },
                    SwarmEvent::OutgoingConnectionError {peer_id, ..} => if let Some(pid) = peer_id {
                        tracing::info!(target:"GossipNode","Peer with id {pid} exited.");
                    },
                    SwarmEvent::Behaviour(NetworkEvent::Identify(event)) => match event {
                        identify::Event::Received { peer_id, info } => {
                            for address in info.listen_addrs {
                                swarm.behaviour_mut().kad.add_address(&peer_id, address.clone());
                                swarm.dial(address)?;
                            }
                            // if !self.bootstrapped.load(Ordering::Relaxed) {
                            //     swarm.behaviour_mut().kademlia.bootstrap()?;
                            //     self.bootstrapped.store(true, Ordering::Relaxed);
                            //     log::info!(target: "bootnode", "Bootstrapped");
                            // }
                            tracing::info!(target:"GossipNode","New peer identification received: {peer_id}");
                            let new_peers = self.peers.load(Ordering::Relaxed) + 1;
                            self.peers.store(new_peers, Ordering::Relaxed);
                        },
                        _ => tracing::debug!(target:"GossipNode","Other Identify event received? :>")
                    },
                    SwarmEvent::Behaviour(NetworkEvent::Kad(event)) => {
                        tracing::debug!(target:"GossipNode","Kademlia event received.");
                    },
                    // TODO: Ping event handle
                    SwarmEvent::Behaviour(NetworkEvent::Ping(event)) => {
                        tracing::debug!(target:"GossipNode","Ping event received");
                    },
                    // TODO: implement events handling
                    _ => tracing::debug!(target:"GossipNode","Got new event from network"),
                }
            }
        }
    }
}
