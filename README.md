# p2p-mpc

A peer-to-peer multi-party computation framework built with Rust and libp2p, featuring distributed networking, observability, and flexible configuration.

## Features

- **P2P Networking**: Built on [libp2p](https://libp2p.io/) with support for:
  - Gossipsub for pub/sub messaging
  - Kademlia DHT for peer discovery
  - Multiple transport protocols (TCP, WebSocket)
  - Configurable bootstrap and seed nodes
  
- **Observability**: Comprehensive monitoring and tracing
  - Prometheus metrics endpoint
  - Jaeger distributed tracing support
  - JSON trace output for analysis
  - Structured logging with configurable levels

- **Flexible Configuration**: Configure via YAML files or CLI arguments
  - Network parameters (ports, peers, topics)
  - Tracing and metrics settings
  - Identity key management
  - Priority system: CLI args > Config file > Defaults

## Quick Start

### Prerequisites

- Rust 1.70+ (edition 2024)
- Cargo

### Installation

```bash
git clone https://github.com/yourusername/p2p-mpc.git
cd p2p-mpc
cargo build --release
```

### Running a Single Node

```bash
# With default settings
cargo run

# With custom configuration
cargo run -- --config config.yml

# With CLI arguments
cargo run -- --network-port 9000 --topic mpc-test --log-level debug
```

### Running Multiple Nodes

#### Terminal 1 (Node 1):
```bash
cargo run -- \
  --network-port 9000 \
  --topic mpc-test \
  --identity-key-path ./keys/node1.key \
  --metrics-port 9090
```

Copy the peer ID from the logs (e.g., `12D3KooWXXXXXXXXXXX`)

#### Terminal 2 (Node 2):
```bash
cargo run -- \
  --network-port 9001 \
  --topic mpc-test \
  --identity-key-path ./keys/node2.key \
  --bootnode /ip4/127.0.0.1/tcp/9000/p2p/<NODE1_PEER_ID> \
  --metrics-port 9091
```

## Configuration

### Using Config Files

Create a `config.yml` file (see `config.example.yml` for reference):

```yaml
# Metrics
metrics_port: 9090

# Tracing
log_level: info
enable_jaeger: false

# Network
network_port: 9000
topics:
  - mpc-computation
  - mpc-control
bootnodes:
  - /ip4/127.0.0.1/tcp/9001/p2p/12D3KooWExample
identity_key_path: ./keys/node.key
```

Then run:
```bash
cargo run -- --config config.yml
```

### CLI Arguments

All configuration options are available via CLI flags. Use `--help` to see all options:

```bash
cargo run -- --help
```

#### Key Arguments:

| Argument | Short | Description | Default |
|----------|-------|-------------|---------|
| `--config` | `-c` | Path to YAML config file | None |
| `--network-port` | `-p` | P2P network listening port | 0 (random) |
| `--topic` | `-t` | Gossipsub topics (repeatable) | None |
| `--bootnode` | | Bootstrap nodes (repeatable) | None |
| `--seed-node` | | Seed nodes (repeatable) | None |
| `--identity-key-path` | | Path to identity key file | None (ephemeral) |
| `--metrics-port` | `-m` | Prometheus metrics port | 9090 |
| `--log-level` | `-l` | Logging level | info |
| `--enable-jaeger` | `-j` | Enable Jaeger tracing | false |

See `cli.yml` for complete documentation of all arguments.

## Architecture

```
┌─────────────────────────────────────────┐
│           Application Layer             │
│  (MPC Computation, File Sharing, etc.)  │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│         P2P Network Layer               │
│  ┌──────────┐  ┌──────────┐            │
│  │ Gossipsub│  │ Kademlia │            │
│  │  Pub/Sub │  │   DHT    │            │
│  └──────────┘  └──────────┘            │
│  ┌──────────┐  ┌──────────┐            │
│  │ Identify │  │   Ping   │            │
│  └──────────┘  └──────────┘            │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│      Transport Layer (libp2p)           │
│     TCP / WebSocket / DNS               │
└─────────────────────────────────────────┘
```

### Components

- **`cli.rs`**: CLI argument parsing and configuration management
- **`network.rs`**: P2P networking logic using libp2p
- **`metrics.rs`**: Prometheus metrics server
- **`tracing_config.rs`**: Logging and distributed tracing setup
- **`main.rs`**: Application entry point and orchestration

## Monitoring & Observability

### Prometheus Metrics

Access metrics at `http://localhost:9090/metrics` (or your configured port)

Available metrics:
- `p2p_mpc_requests_total` - Total number of requests
- `p2p_mpc_active_connections` - Number of active connections
- `p2p_mpc_request_duration_seconds` - Request duration histogram

### Jaeger Tracing

Enable distributed tracing:

```bash
cargo run -- \
  --enable-jaeger \
  --jaeger-host localhost \
  --jaeger-port 4317
```

### JSON Trace Output

Export traces to file for analysis:

```bash
cargo run -- \
  --json-trace \
  --json-trace-file ./logs/traces.json
```

## Identity Management

The application supports persistent identity keys:

```bash
# Generate and save identity key
cargo run -- --identity-key-path ./keys/node.key

# Subsequent runs will reuse the same peer ID
cargo run -- --identity-key-path ./keys/node.key
```

Without specifying a key path, an ephemeral key is generated (new peer ID each run).

## Development

### Project Structure

```
p2p-mpc/
├── src/
│   ├── main.rs           # Entry point
│   ├── cli.rs            # Configuration & CLI
│   ├── network.rs        # P2P networking
│   ├── metrics.rs        # Prometheus metrics
│   └── tracing_config.rs # Logging & tracing
├── cli.yml               # CLI documentation
├── config.example.yml    # Example configuration
├── Cargo.toml           # Dependencies
└── README.md            # This file
```

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Check without building
cargo check
```

### Dependencies

Key dependencies:
- **libp2p** - P2P networking framework
- **tokio** - Async runtime
- **clap** - CLI parsing
- **prometheus** - Metrics
- **opentelemetry** - Distributed tracing
- **tracing** - Logging
- **serde** - Serialization

See `Cargo.toml` for complete list.

## Examples

### Basic P2P Chat

```bash
# Node 1
cargo run -- --network-port 9000 --topic chat

# Node 2 (connect to node 1)
cargo run -- --network-port 9001 --topic chat \
  --bootnode /ip4/127.0.0.1/tcp/9000/p2p/<PEER_ID>
```

### Multiple Topics

```bash
cargo run -- \
  --topic mpc-computation \
  --topic mpc-control \
  --topic mpc-results \
  --network-port 9000
```

### With Full Observability

```bash
cargo run -- \
  --network-port 9000 \
  --metrics-port 9090 \
  --enable-jaeger \
  --jaeger-host localhost \
  --jaeger-port 4317 \
  --log-level debug \
  --topic mpc-test
```

## Troubleshooting

### Nodes not connecting

1. Check that ports are not blocked by firewall
2. Verify the bootnode multiaddr includes the correct peer ID
3. Ensure both nodes subscribe to the same topic
4. Check logs for connection errors

### High memory usage

1. Reduce the number of topics subscribed to
2. Implement message size limits
3. Add peer connection limits in the network configuration

### Metrics not available

1. Verify the metrics port is not in use
2. Check firewall allows access to the metrics port
3. Ensure Prometheus server is accessible at the configured address

## Roadmap

- [ ] Implement actual MPC computation protocols
- [ ] Add reputation system for peer trust
- [ ] Implement distributed file storage
- [ ] Add NAT traversal support
- [ ] Create web dashboard for monitoring
- [ ] Add message encryption
- [ ] Implement content validation

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## License

[MIT License](LICENSE) - See LICENSE file for details

## Resources

- [libp2p Documentation](https://docs.libp2p.io/)
- [Gossipsub Specification](https://github.com/libp2p/specs/blob/master/pubsub/gossipsub/README.md)
- [Kademlia DHT](https://pdos.csail.mit.edu/~petar/papers/maymounkov-kademlia-lncs.pdf)
- [Prometheus Best Practices](https://prometheus.io/docs/practices/)
- [OpenTelemetry Rust](https://opentelemetry.io/docs/instrumentation/rust/)

## Acknowledgments

Built with Rust and the amazing libp2p library.
