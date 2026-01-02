mod cli;
mod metrics;
mod tracing_config;

use clap::Parser;
use cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize tracing configuration
    tracing_config::init_trace(
        &cli.jaeger_host,
        cli.jaeger_port,
        cli.enable_jaeger,
        cli.json_trace,
        &cli.json_trace_file,
        &cli.log_level,
    );

    // Start the Prometheus metrics server
    metrics::start_metrics_server(cli.metrics_port).await;
}
