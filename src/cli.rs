use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Configuration structure that can be loaded from YAML
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,

    #[serde(default)]
    pub enable_jaeger: bool,

    #[serde(default = "default_jaeger_host")]
    pub jaeger_host: String,

    #[serde(default = "default_jaeger_port")]
    pub jaeger_port: u16,

    #[serde(default)]
    pub json_trace: bool,

    #[serde(default = "default_json_trace_file")]
    pub json_trace_file: String,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    // Network configuration
    #[serde(default = "default_network_port")]
    pub network_port: u16,

    #[serde(default)]
    pub bootnodes: Vec<String>,

    #[serde(default)]
    pub seed_nodes: Vec<String>,

    #[serde(default)]
    pub topics: Vec<String>,

    #[serde(default)]
    pub explicit_peer: Option<String>,

    #[serde(default)]
    pub identity_key_path: Option<String>,
}

fn default_metrics_port() -> u16 {
    9090
}
fn default_jaeger_host() -> String {
    "localhost".to_string()
}
fn default_jaeger_port() -> u16 {
    4317
}
fn default_json_trace_file() -> String {
    "traces.json".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_network_port() -> u16 {
    0
} // 0 means random available port

impl Default for Config {
    fn default() -> Self {
        Self {
            metrics_port: default_metrics_port(),
            enable_jaeger: false,
            jaeger_host: default_jaeger_host(),
            jaeger_port: default_jaeger_port(),
            json_trace: false,
            json_trace_file: default_json_trace_file(),
            log_level: default_log_level(),
            network_port: default_network_port(),
            bootnodes: Vec::new(),
            seed_nodes: Vec::new(),
            topics: Vec::new(),
            explicit_peer: None,
            identity_key_path: None,
        }
    }
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_file(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
}

/// P2P MPC - Peer-to-peer Multi-Party Computation
#[derive(Parser, Debug)]
#[command(name = "p2p-mpc")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Path to YAML configuration file
    #[arg(short = 'c', long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Port number for the Prometheus metrics server
    #[arg(short = 'm', long)]
    pub metrics_port: Option<u16>,

    /// Enable Jaeger distributed tracing
    #[arg(short = 'j', long)]
    pub enable_jaeger: Option<bool>,

    /// Jaeger collector host address
    #[arg(long)]
    pub jaeger_host: Option<String>,

    /// Jaeger collector port
    #[arg(long)]
    pub jaeger_port: Option<u16>,

    /// Enable JSON formatted trace output to file
    #[arg(long)]
    pub json_trace: Option<bool>,

    /// File path for JSON trace output
    #[arg(long)]
    pub json_trace_file: Option<String>,

    /// Set the logging level (trace, debug, info, warn, error)
    #[arg(short = 'l', long)]
    pub log_level: Option<String>,

    // Network configuration
    /// P2P network listening port (0 for random available port)
    #[arg(short = 'p', long)]
    pub network_port: Option<u16>,

    /// Bootstrap nodes (can be specified multiple times)
    #[arg(long)]
    pub bootnode: Vec<String>,

    /// Seed nodes (can be specified multiple times)
    #[arg(long)]
    pub seed_node: Vec<String>,

    /// Topics to subscribe to (can be specified multiple times)
    #[arg(short = 't', long)]
    pub topic: Vec<String>,

    /// Explicit peer ID to add
    #[arg(long)]
    pub explicit_peer: Option<String>,

    /// Path to identity key file (will generate new if not provided)
    #[arg(long)]
    pub identity_key_path: Option<String>,
}

impl Cli {
    /// Parse CLI arguments and merge with config file if provided
    pub fn get_config() -> Result<Config, Box<dyn std::error::Error>> {
        let cli = Cli::parse();

        // Start with config from file, or use default
        let mut config = if let Some(config_path) = &cli.config {
            Config::from_file(config_path)?
        } else {
            Config::default()
        };

        // Override with CLI arguments if provided
        if let Some(port) = cli.metrics_port {
            config.metrics_port = port;
        }
        if let Some(enable) = cli.enable_jaeger {
            config.enable_jaeger = enable;
        }
        if let Some(host) = cli.jaeger_host {
            config.jaeger_host = host;
        }
        if let Some(port) = cli.jaeger_port {
            config.jaeger_port = port;
        }
        if let Some(enable) = cli.json_trace {
            config.json_trace = enable;
        }
        if let Some(file) = cli.json_trace_file {
            config.json_trace_file = file;
        }
        if let Some(level) = cli.log_level {
            config.log_level = level;
        }

        // Network configuration overrides
        if let Some(port) = cli.network_port {
            config.network_port = port;
        }
        if !cli.bootnode.is_empty() {
            config.bootnodes = cli.bootnode;
        }
        if !cli.seed_node.is_empty() {
            config.seed_nodes = cli.seed_node;
        }
        if !cli.topic.is_empty() {
            config.topics = cli.topic;
        }
        if cli.explicit_peer.is_some() {
            config.explicit_peer = cli.explicit_peer;
        }
        if cli.identity_key_path.is_some() {
            config.identity_key_path = cli.identity_key_path;
        }

        Ok(config)
    }
}
