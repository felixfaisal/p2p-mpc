use clap::Parser;

/// P2P MPC - Peer-to-peer Multi-Party Computation
#[derive(Parser, Debug)]
#[command(name = "p2p-mpc")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Port number for the Prometheus metrics server
    #[arg(short = 'm', long, default_value_t = 9090)]
    pub metrics_port: u16,

    /// Enable Jaeger distributed tracing
    #[arg(short = 'j', long, default_value_t = false)]
    pub enable_jaeger: bool,

    /// Jaeger collector host address
    #[arg(long, default_value = "localhost")]
    pub jaeger_host: String,

    /// Jaeger collector port
    #[arg(long, default_value_t = 4317)]
    pub jaeger_port: u16,

    /// Enable JSON formatted trace output to file
    #[arg(long, default_value_t = false)]
    pub json_trace: bool,

    /// File path for JSON trace output
    #[arg(long, default_value = "traces.json")]
    pub json_trace_file: String,

    /// Set the logging level (trace, debug, info, warn, error)
    #[arg(short = 'l', long, default_value = "info")]
    pub log_level: String,
}
