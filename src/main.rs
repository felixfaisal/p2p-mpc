mod cli;
mod crypto;
mod metrics;
mod mpc;
mod network;
mod rpc;
mod tracing_config;

use cli::Cli;
use crypto::run;
use libp2p::{PeerId, identity};
use network::Network;
use rpc::NetworkInfo;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration from file and/or CLI arguments
    let config = Cli::get_config().expect("Failed to load configuration");

    // Initialize tracing configuration
    tracing_config::init_trace(
        &config.jaeger_host,
        config.jaeger_port,
        config.enable_jaeger,
        config.json_trace,
        &config.json_trace_file,
        &config.log_level,
    );

    // Start the Prometheus metrics server in background
    let metrics_port = config.metrics_port;
    tokio::spawn(async move {
        metrics::start_metrics_server(metrics_port).await;
    });

    // Load or generate identity keypair
    let local_key = if let Some(key_path) = &config.identity_key_path {
        // Try to load existing key from file
        match fs::read(key_path) {
            Ok(key_bytes) => {
                tracing::info!(target: "GossipNode", "Loading identity key from {}", key_path);
                identity::Keypair::from_protobuf_encoding(&key_bytes)
                    .map_err(|e| anyhow::anyhow!("Failed to decode identity key: {}", e))?
            }
            Err(_) => {
                // Generate new key and save it
                tracing::info!(target: "GossipNode", "Generating new identity key and saving to {}", key_path);
                let keypair = identity::Keypair::generate_ed25519();
                let key_bytes = keypair
                    .to_protobuf_encoding()
                    .map_err(|e| anyhow::anyhow!("Failed to encode identity key: {}", e))?;

                // Create parent directories if they don't exist
                if let Some(parent) = std::path::Path::new(key_path).parent() {
                    fs::create_dir_all(parent)?;
                }

                fs::write(key_path, key_bytes)?;
                keypair
            }
        }
    } else {
        // Generate ephemeral key (not saved)
        tracing::info!(target: "GossipNode", "Generating ephemeral identity key");
        identity::Keypair::generate_ed25519()
    };

    let local_peer_id = PeerId::from(local_key.public());
    tracing::info!(target: "GossipNode", "Local peer ID: {}", local_peer_id);

    // Create shared state for RPC and Network
    let peer_list = Arc::new(RwLock::new(Vec::new()));
    let listen_addresses = Arc::new(RwLock::new(Vec::new()));
    let sessions = Arc::new(RwLock::new(HashMap::new()));
    let current_session_id = Arc::new(RwLock::new(None));

    // Initialize P2P network
    let (p2p_net, network_command_sender, mpc_orchestrator) = Network::new(
        local_peer_id,
        config.network_port,
        local_key,
        peer_list.clone(),
        listen_addresses.clone(),
        sessions.clone(),
        current_session_id.clone(),
    )
    .await;

    // Create shared network info for RPC server
    let network_info = NetworkInfo {
        peer_id: local_peer_id,
        listen_addresses,
        peers: peer_list,
        topics: Arc::new(RwLock::new(config.topics.clone())),
        protocol_name: network::PROTOCOL_NAME.to_string(),
        network_command_sender: network_command_sender.clone(),
        peer_assignments: Arc::new(RwLock::new(HashMap::new())),
        assignment_acceptances: Arc::new(RwLock::new(HashMap::new())),
        sessions,
        current_session_id,
        mpc_orchestrator,
    };

    // Start the RPC server in background
    let rpc_port = config.rpc_port;
    let rpc_info = network_info.clone();
    tokio::spawn(async move {
        if let Err(e) = rpc::start_rpc_server(rpc_port, rpc_info).await {
            tracing::error!("RPC server error: {}", e);
        }
    });

    tracing::info!(target: "GossipNode", "Starting network on port {}", config.network_port);
    tracing::info!(target: "GossipNode", "Network command channel ready for broadcasting messages");

    // Run the network with configuration
    p2p_net
        .run(
            config.topics,
            config.seed_nodes,
            config.bootnodes,
            config.explicit_peer.as_deref(),
        )
        .await
}
