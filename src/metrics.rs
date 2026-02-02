use lazy_static::lazy_static;
use prometheus::{
    Counter, CounterVec, Encoder, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, Opts,
    Registry, TextEncoder,
};
use warp::Filter;

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    // ========================================================================
    // GENERAL METRICS
    // ========================================================================
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

    // ========================================================================
    // P2P NETWORK METRICS
    // ========================================================================
    pub static ref CONNECTED_PEERS: Gauge =
        Gauge::new("p2p_connected_peers", "Number of currently connected peers")
            .expect("metric can be created");

    pub static ref GOSSIPSUB_MESSAGES_SENT: CounterVec =
        CounterVec::new(
            Opts::new("p2p_gossipsub_messages_sent_total", "Total gossipsub messages sent by topic"),
            &["topic"]
        )
        .expect("metric can be created");

    pub static ref GOSSIPSUB_MESSAGES_RECEIVED: CounterVec =
        CounterVec::new(
            Opts::new("p2p_gossipsub_messages_received_total", "Total gossipsub messages received by topic"),
            &["topic"]
        )
        .expect("metric can be created");

    pub static ref NETWORK_BYTES_SENT: Counter =
        Counter::new("p2p_network_bytes_sent_total", "Total bytes sent over network")
            .expect("metric can be created");

    pub static ref NETWORK_BYTES_RECEIVED: Counter =
        Counter::new("p2p_network_bytes_received_total", "Total bytes received over network")
            .expect("metric can be created");

    pub static ref PEER_DIAL_ATTEMPTS: Counter =
        Counter::new("p2p_peer_dial_attempts_total", "Total peer dial attempts")
            .expect("metric can be created");

    pub static ref PEER_DIAL_FAILURES: Counter =
        Counter::new("p2p_peer_dial_failures_total", "Total peer dial failures")
            .expect("metric can be created");

    // ========================================================================
    // MPC PROTOCOL METRICS
    // ========================================================================

    // Protocol State
    pub static ref MPC_PROTOCOL_STATE: GaugeVec =
        GaugeVec::new(
            Opts::new("mpc_protocol_state", "Current MPC protocol state (1 = active)"),
            &["state"]
        )
        .expect("metric can be created");

    // Aux Generation Metrics
    pub static ref AUX_GENERATION_INITIATED: Counter =
        Counter::new("mpc_aux_generation_initiated_total", "Total aux generation protocols initiated")
            .expect("metric can be created");

    pub static ref AUX_GENERATION_COMPLETED: Counter =
        Counter::new("mpc_aux_generation_completed_total", "Total aux generation protocols completed")
            .expect("metric can be created");

    pub static ref AUX_GENERATION_FAILED: Counter =
        Counter::new("mpc_aux_generation_failed_total", "Total aux generation protocols failed")
            .expect("metric can be created");

    pub static ref AUX_GENERATION_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("mpc_aux_generation_duration_seconds", "Aux generation duration in seconds")
            .buckets(vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0])
    )
    .expect("metric can be created");

    // Keygen Metrics
    pub static ref KEYGEN_INITIATED: Counter =
        Counter::new("mpc_keygen_initiated_total", "Total keygen protocols initiated")
            .expect("metric can be created");

    pub static ref KEYGEN_COMPLETED: Counter =
        Counter::new("mpc_keygen_completed_total", "Total keygen protocols completed")
            .expect("metric can be created");

    pub static ref KEYGEN_FAILED: Counter =
        Counter::new("mpc_keygen_failed_total", "Total keygen protocols failed")
            .expect("metric can be created");

    pub static ref KEYGEN_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("mpc_keygen_duration_seconds", "Keygen duration in seconds")
            .buckets(vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0])
    )
    .expect("metric can be created");

    // Signing Metrics
    pub static ref SIGNING_INITIATED: Counter =
        Counter::new("mpc_signing_initiated_total", "Total signing protocols initiated")
            .expect("metric can be created");

    pub static ref SIGNING_COMPLETED: Counter =
        Counter::new("mpc_signing_completed_total", "Total signing protocols completed")
            .expect("metric can be created");

    pub static ref SIGNING_FAILED: Counter =
        Counter::new("mpc_signing_failed_total", "Total signing protocols failed")
            .expect("metric can be created");

    pub static ref SIGNING_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("mpc_signing_duration_seconds", "Signing duration in seconds")
            .buckets(vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0])
    )
    .expect("metric can be created");

    // MPC Message Metrics
    pub static ref MPC_MESSAGES_SENT: CounterVec =
        CounterVec::new(
            Opts::new("mpc_messages_sent_total", "Total MPC messages sent by protocol"),
            &["protocol"]
        )
        .expect("metric can be created");

    pub static ref MPC_MESSAGES_RECEIVED: CounterVec =
        CounterVec::new(
            Opts::new("mpc_messages_received_total", "Total MPC messages received by protocol"),
            &["protocol"]
        )
        .expect("metric can be created");

    pub static ref MPC_BROADCAST_MESSAGES: CounterVec =
        CounterVec::new(
            Opts::new("mpc_broadcast_messages_total", "Total MPC broadcast messages by protocol"),
            &["protocol"]
        )
        .expect("metric can be created");

    pub static ref MPC_P2P_MESSAGES: CounterVec =
        CounterVec::new(
            Opts::new("mpc_p2p_messages_total", "Total MPC P2P messages by protocol"),
            &["protocol"]
        )
        .expect("metric can be created");

    // Coordination Message Metrics
    pub static ref COORDINATION_MESSAGES_SENT: CounterVec =
        CounterVec::new(
            Opts::new("mpc_coordination_messages_sent_total", "Total coordination messages sent by type"),
            &["message_type"]
        )
        .expect("metric can be created");

    pub static ref COORDINATION_MESSAGES_RECEIVED: CounterVec =
        CounterVec::new(
            Opts::new("mpc_coordination_messages_received_total", "Total coordination messages received by type"),
            &["message_type"]
        )
        .expect("metric can be created");

    // Party Participation
    pub static ref ACTIVE_PARTIES: Gauge =
        Gauge::new("mpc_active_parties", "Number of active parties in current protocol")
            .expect("metric can be created");

    pub static ref THRESHOLD_VALUE: Gauge =
        Gauge::new("mpc_threshold_value", "Current threshold value for keygen/signing")
            .expect("metric can be created");

    // ========================================================================
    // RPC METRICS
    // ========================================================================
    pub static ref RPC_REQUESTS: CounterVec =
        CounterVec::new(
            Opts::new("rpc_requests_total", "Total RPC requests by method"),
            &["method"]
        )
        .expect("metric can be created");

    pub static ref RPC_ERRORS: CounterVec =
        CounterVec::new(
            Opts::new("rpc_errors_total", "Total RPC errors by method"),
            &["method"]
        )
        .expect("metric can be created");

    pub static ref RPC_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new("rpc_duration_seconds", "RPC request duration in seconds"),
        &["method"]
    )
    .expect("metric can be created");

    // ========================================================================
    // CACHE METRICS (for development mode)
    // ========================================================================
    pub static ref PAILLIER_PRIMES_CACHE_HITS: Counter =
        Counter::new("mpc_paillier_primes_cache_hits_total", "Total Paillier primes cache hits")
            .expect("metric can be created");

    pub static ref PAILLIER_PRIMES_CACHE_MISSES: Counter =
        Counter::new("mpc_paillier_primes_cache_misses_total", "Total Paillier primes cache misses")
            .expect("metric can be created");
}

pub fn register_metrics() {
    // General metrics
    REGISTRY
        .register(Box::new(REQUEST_COUNTER.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(ACTIVE_CONNECTIONS.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(REQUEST_DURATION.clone()))
        .expect("collector can be registered");

    // P2P network metrics
    REGISTRY
        .register(Box::new(CONNECTED_PEERS.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(GOSSIPSUB_MESSAGES_SENT.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(GOSSIPSUB_MESSAGES_RECEIVED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(NETWORK_BYTES_SENT.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(NETWORK_BYTES_RECEIVED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(PEER_DIAL_ATTEMPTS.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(PEER_DIAL_FAILURES.clone()))
        .expect("collector can be registered");

    // MPC protocol metrics
    REGISTRY
        .register(Box::new(MPC_PROTOCOL_STATE.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(AUX_GENERATION_INITIATED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(AUX_GENERATION_COMPLETED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(AUX_GENERATION_FAILED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(AUX_GENERATION_DURATION.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(KEYGEN_INITIATED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(KEYGEN_COMPLETED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(KEYGEN_FAILED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(KEYGEN_DURATION.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(SIGNING_INITIATED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(SIGNING_COMPLETED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(SIGNING_FAILED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(SIGNING_DURATION.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(MPC_MESSAGES_SENT.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(MPC_MESSAGES_RECEIVED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(MPC_BROADCAST_MESSAGES.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(MPC_P2P_MESSAGES.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(COORDINATION_MESSAGES_SENT.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(COORDINATION_MESSAGES_RECEIVED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(ACTIVE_PARTIES.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(THRESHOLD_VALUE.clone()))
        .expect("collector can be registered");

    // RPC metrics
    REGISTRY
        .register(Box::new(RPC_REQUESTS.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(RPC_ERRORS.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(RPC_DURATION.clone()))
        .expect("collector can be registered");

    // Cache metrics
    REGISTRY
        .register(Box::new(PAILLIER_PRIMES_CACHE_HITS.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(PAILLIER_PRIMES_CACHE_MISSES.clone()))
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
