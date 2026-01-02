use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use std::fs::OpenOptions;
use tracing_log::LogTracer;
use tracing_subscriber::FmtSubscriber;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Layer};

pub fn init_trace(
    jaeger_host: &str,
    jaeger_port: u16,
    enable_jaegar: bool,
    json_trace_output: bool,
    json_trace_file: &str,
    log_level: &str,
) {
    global::set_text_map_propagator(TraceContextPropagator::new());
    LogTracer::init().expect("Could not initialize log tracer");

    // Create the EnvFilter with the provided log level as fallback
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    if enable_jaegar {
        // Use Jaeger tracing with OpenTelemetry
        let tracer = init_jaegar_trace(jaeger_host, jaeger_port)
            .expect("Failed to initialize Jaeger tracer");
        let telemetry =
            tracing_opentelemetry::layer::<tracing_subscriber::Registry>().with_tracer(tracer);

        let subscriber = tracing_subscriber::Registry::default()
            .with(telemetry)
            .with(tracing_subscriber::fmt::layer().with_filter(env_filter));

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default subscriber failed");
    } else if json_trace_output {
        // Output JSON formatted traces to file for later analysis
        let trace_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(json_trace_file)
            .expect("Failed to create trace file");

        let subscriber = FmtSubscriber::builder()
            .json()
            .with_writer(trace_file)
            .with_env_filter(env_filter)
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default subscriber failed");
    } else {
        // Use regular logging without Jaeger
        let subscriber = FmtSubscriber::builder()
            .with_env_filter(env_filter)
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default subscriber failed");
    }
}

pub fn init_jaegar_trace(
    host: &str,
    port: u16,
) -> Result<SdkTracer, Box<dyn std::error::Error + Send + Sync + 'static>> {
    let endpoint = format!("http://{}:{}", host, port);
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;

    let resource = Resource::builder().with_service_name("p2p-worker").build();

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let tracer = tracer_provider.tracer("p2p-worker");
    Ok(tracer)
}
