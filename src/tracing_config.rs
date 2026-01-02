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
    host: Option<&str>,
    port: Option<u16>,
    enable_jaegar: bool,
    json_trace_output: bool,
    json_trace_file: Option<&str>,
) {
    global::set_text_map_propagator(TraceContextPropagator::new());
    LogTracer::init().expect("Could not initialize log tracer");

    if enable_jaegar {
        // Use Jaeger tracing with OpenTelemetry
        let tracer = init_jaegar_trace(host.unwrap(), port.unwrap()).unwrap();
        let telemetry =
            tracing_opentelemetry::layer::<tracing_subscriber::Registry>().with_tracer(tracer);

        let subscriber = tracing_subscriber::Registry::default()
            .with(telemetry)
            .with(tracing_subscriber::fmt::layer().with_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            ));

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default subscriber failed");
    } else if json_trace_output {
        // Output JSON formatted traces to file for later analysis
        let trace_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(json_trace_file.expect("Json Trace file path is required"))
            .expect("Failed to create trace file");

        let subscriber = FmtSubscriber::builder()
            .json()
            .with_writer(trace_file)
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default subscriber failed");
    } else {
        // Use regular logging without Jaeger
        let subscriber = FmtSubscriber::builder()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
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

    let resource = Resource::builder()
        .with_service_name("executor-worker")
        .build();

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let tracer = tracer_provider.tracer("executor-worker");
    Ok(tracer)
}
