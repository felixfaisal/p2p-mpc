use lazy_static::lazy_static;
use prometheus::{Counter, Encoder, Gauge, Histogram, HistogramOpts, Registry, TextEncoder};
use warp::Filter;

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref REQUEST_COUNTER: Counter =
        Counter::new("p2p_mpc_requests_total", "Total number of requests")
            .expect("metric can be created");
    pub static ref ACTIVE_CONNECTIONS: Gauge =
        Gauge::new("p2p_mpc_active_connections", "Number of active connections")
            .expect("metric can be created");
    pub static ref REQUEST_DURATION: Histogram = Histogram::with_opts(HistogramOpts::new(
        "p2p_mpc_request_duration_seconds",
        "Request duration in seconds"
    ))
    .expect("metric can be created");
}

pub fn register_metrics() {
    REGISTRY
        .register(Box::new(REQUEST_COUNTER.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(ACTIVE_CONNECTIONS.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(REQUEST_DURATION.clone()))
        .expect("collector can be registered");
}

async fn metrics_handler() -> Result<impl warp::Reply, warp::Rejection> {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();

    Ok(warp::reply::with_header(
        buffer,
        "Content-Type",
        encoder.format_type(),
    ))
}

pub async fn start_metrics_server(port: u16) {
    register_metrics();

    let metrics_route = warp::path("metrics")
        .and(warp::get())
        .and_then(metrics_handler);

    tracing::info!("Starting Prometheus metrics server on port {}", port);
    warp::serve(metrics_route).run(([0, 0, 0, 0], port)).await;
}
