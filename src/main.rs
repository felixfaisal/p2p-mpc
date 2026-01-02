mod metrics;
mod tracing_config;

#[tokio::main]
async fn main() {
    // Initialize tracing configuration
    tracing_config::init_trace(None, None, false, false, None);

    // Start the Prometheus metrics server on port 9090
    metrics::start_metrics_server(9090).await;
}
