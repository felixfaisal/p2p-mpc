mod cli;
mod metrics;
mod network;
mod tracing_config;

use cli::Cli;
use libp2p::{PeerId, identity};
use network::Network;
use std::fs;

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

    // Initialize P2P network
    let p2p_net = Network::new(local_peer_id, config.network_port, local_key).await;

    tracing::info!(target: "GossipNode", "Starting network on port {}", config.network_port);

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
