//! Logs and traces.
//!
//! On Cloud Run (or with `LOG_FORMAT=json`) logs are JSON lines in the Cloud
//! Logging structured format: severity, source location, the fields of the
//! event and its spans, and the trace and span ID so Logs Explorer groups them
//! with the request and Cloud Trace shows them under the span. Errors carry
//! the Error Reporting type, so each distinct error gets an error group.
//!
//! Spans go to Cloud Trace through the OTLP endpoint of the Telemetry API,
//! authenticated as the service account. Elsewhere logs are plain text and
//! nothing is exported.

use std::io::IsTerminal;
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};
use bytes::Bytes;
use google_cloud_auth::credentials::{AccessTokenCredentials, Builder};
use opentelemetry::KeyValue;
use opentelemetry::trace::{TraceContextExt, TracerProvider};
use opentelemetry_http::{HttpClient, HttpError, Request, Response};
use opentelemetry_otlp::{Protocol, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use serde_json::{Map, Value, json};
use tracing::dispatcher::WeakDispatch;
use tracing::field::{Field, Visit};
use tracing::{Dispatch, Event, Subscriber};
use tracing_subscriber::fmt::format::{JsonFields, Writer};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

const TRACES_ENDPOINT: &str = "https://telemetry.googleapis.com/v1/traces";

/// Keeps the trace exporter alive; call [`Telemetry::shutdown`] before exit.
pub struct Telemetry {
    provider: Option<SdkTracerProvider>,
}

/// Where the process runs and what it reports itself as.
struct Service {
    /// Cloud Run service or job name.
    name: String,
    version: String,
    project: Option<String>,
}

impl Service {
    fn detect() -> Option<Self> {
        let name = std::env::var("K_SERVICE")
            .or_else(|_| std::env::var("CLOUD_RUN_JOB"))
            .ok()?;
        Some(Self {
            name,
            version: std::env::var("APP_VERSION")
                .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into()),
            project: std::env::var("GCP_PROJECT").ok(),
        })
    }
}

impl Telemetry {
    /// Installs the global subscriber. Must run inside the Tokio runtime.
    pub fn init() -> Self {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "fitdata=info".into());
        let service = Service::detect();
        let json = match std::env::var("LOG_FORMAT").as_deref() {
            Ok("json") => true,
            Ok(_) => false,
            Err(_) => service.is_some(),
        };

        let (provider, export_error) = match service.as_ref().filter(|s| s.project.is_some()) {
            Some(s) => match tracer_provider(s) {
                Ok(p) => (Some(p), None),
                Err(e) => (None, Some(e)),
            },
            None => (None, None),
        };
        let otel = provider.as_ref().map(|p| {
            tracing_opentelemetry::layer()
                .with_tracer(p.tracer("fitdata"))
                .with_error_events_to_status(true)
        });
        let dispatch_cell = Arc::new(OnceLock::new());
        let json_layer = json.then(|| {
            let s = service.as_ref();
            tracing_subscriber::fmt::layer()
                .fmt_fields(JsonFields::new())
                .event_format(GcpFormat {
                    project: s.and_then(|s| s.project.clone()),
                    service: s.map_or("fitdata".into(), |s| s.name.clone()),
                    version: s.map_or(env!("CARGO_PKG_VERSION").into(), |s| s.version.clone()),
                    dispatch: dispatch_cell.clone(),
                })
        });
        let text_layer = (!json).then(|| {
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(std::io::stdout().is_terminal())
        });
        let dispatch = Dispatch::new(
            tracing_subscriber::registry()
                .with(filter)
                .with(otel)
                .with(json_layer)
                .with(text_layer),
        );
        let _ = dispatch_cell.set(dispatch.downgrade());
        dispatch.init();

        if json {
            // The default hook prints plain text to stderr, which Cloud
            // Logging stores without severity.
            std::panic::set_hook(Box::new(|info| {
                let bt = std::backtrace::Backtrace::force_capture();
                tracing::error!("{info}\n{bt}");
            }));
        }
        if let Some(e) = export_error {
            tracing::warn!("trace export disabled: {e:#}");
        }
        Self { provider }
    }

    /// Flushes pending spans.
    pub async fn shutdown(self) {
        let Some(provider) = self.provider else {
            return;
        };
        // Blocks until the exporter, which runs on this runtime, is done.
        let res = tokio::task::spawn_blocking(move || provider.shutdown()).await;
        if let Ok(Err(e)) = res {
            eprintln!("trace export shutdown: {e}");
        }
    }
}

fn tracer_provider(s: &Service) -> Result<SdkTracerProvider> {
    let project = s.project.clone().unwrap_or_default();
    let client = AuthClient {
        http: reqwest::Client::new(),
        creds: Builder::default()
            .build_access_token_credentials()
            .context("Google credentials for Cloud Trace")?,
        rt: tokio::runtime::Handle::current(),
    };
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_http_client(client)
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint(TRACES_ENDPOINT)
        .build()?;
    let mut attrs = vec![
        // The Telemetry API takes the destination project from here.
        KeyValue::new("gcp.project_id", project),
        KeyValue::new("service.version", s.version.clone()),
        KeyValue::new("cloud.provider", "gcp"),
        KeyValue::new("cloud.platform", "gcp_cloud_run"),
    ];
    for (env, key) in [
        ("GCP_REGION", "cloud.region"),
        ("K_REVISION", "faas.version"),
        ("CLOUD_RUN_EXECUTION", "gcp.cloud_run.job.execution"),
    ] {
        if let Ok(v) = std::env::var(env) {
            attrs.push(KeyValue::new(key, v));
        }
    }
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());
    Ok(SdkTracerProvider::builder()
        // Traffic is tiny; keep every trace, also when Cloud Run's own
        // sampler skipped the request.
        .with_sampler(Sampler::AlwaysOn)
        .with_resource(
            Resource::builder()
                .with_service_name(s.name.clone())
                .with_attributes(attrs)
                .build(),
        )
        .with_batch_exporter(exporter)
        .build())
}

/// OTLP over reqwest with the service account's access token. The batch
/// processor exports from its own thread, so requests run on the Tokio runtime.
#[derive(Debug)]
struct AuthClient {
    http: reqwest::Client,
    creds: AccessTokenCredentials,
    rt: tokio::runtime::Handle,
}

#[async_trait::async_trait]
impl HttpClient for AuthClient {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let http = self.http.clone();
        let creds = self.creds.clone();
        let send = async move {
            let token = creds.access_token().await?.token;
            let (parts, body) = request.into_parts();
            let resp = http
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .bearer_auth(token)
                .body(body)
                .send()
                .await?;
            let mut out = Response::builder().status(resp.status());
            for (k, v) in resp.headers() {
                out = out.header(k, v);
            }
            Ok::<_, HttpError>(out.body(resp.bytes().await?)?)
        };
        self.rt.spawn(send).await?
    }
}

/// Cloud Logging structured JSON, one entry per line.
struct GcpFormat {
    project: Option<String>,
    service: String,
    version: String,
    /// The subscriber this formatter is part of, to look up the OpenTelemetry
    /// span context (the current dispatcher is unset while handling an event).
    dispatch: Arc<OnceLock<WeakDispatch>>,
}

impl<S, N> FormatEvent<S, N> for GcpFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        let mut entry = Map::new();

        // Span fields, outermost first so inner spans and the event win.
        let mut leaf = None;
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                if let Some(fields) = span.extensions().get::<FormattedFields<N>>()
                    && let Ok(Value::Object(m)) = serde_json::from_str(fields)
                {
                    // otel.* fields only name the span for the exporter.
                    entry.extend(m.into_iter().filter(|(k, _)| !k.starts_with("otel.")));
                }
                leaf = Some(span.id());
            }
        }
        event.record(&mut JsonVisitor(&mut entry));
        let message = match entry.remove("message") {
            Some(Value::String(s)) => s,
            Some(v) => v.to_string(),
            None => String::new(),
        };

        let severity = match *meta.level() {
            tracing::Level::ERROR => "ERROR",
            tracing::Level::WARN => "WARNING",
            tracing::Level::INFO => "INFO",
            _ => "DEBUG",
        };
        entry.insert("severity".into(), severity.into());
        entry.insert("message".into(), message.into());
        entry.insert(
            "time".into(),
            chrono::Utc::now()
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
                .into(),
        );
        entry.insert(
            "logging.googleapis.com/sourceLocation".into(),
            json!({
                "file": meta.file(),
                "line": meta.line().map(|l| l.to_string()),
                "function": meta.target(),
            }),
        );

        let dispatch = self.dispatch.get().and_then(WeakDispatch::upgrade);
        if let (Some(project), Some(id), Some(d)) = (&self.project, leaf, dispatch)
            && let Some(cx) = tracing_opentelemetry::get_otel_context(&id, &d)
            && let sc = cx.span().span_context().clone()
            && sc.is_valid()
        {
            entry.insert(
                "logging.googleapis.com/trace".into(),
                format!("projects/{project}/traces/{}", sc.trace_id()).into(),
            );
            entry.insert(
                "logging.googleapis.com/spanId".into(),
                sc.span_id().to_string().into(),
            );
            entry.insert(
                "logging.googleapis.com/trace_sampled".into(),
                sc.is_sampled().into(),
            );
        }

        if *meta.level() == tracing::Level::ERROR {
            entry.insert(
                "@type".into(),
                "type.googleapis.com/google.devtools.clouderrorreporting.v1beta1.ReportedErrorEvent"
                    .into(),
            );
            entry.insert(
                "serviceContext".into(),
                json!({ "service": self.service, "version": self.version }),
            );
        }

        let line = serde_json::to_string(&entry).map_err(|_| std::fmt::Error)?;
        writeln!(writer, "{line}")
    }
}

struct JsonVisitor<'a>(&'a mut Map<String, Value>);

impl Visit for JsonVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.0.insert(field.name().into(), value.to_string().into());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().into(), format!("{value:?}").into());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn json_entry_has_severity_fields_and_trace() {
        let buf = Buf::default();
        let out = buf.clone();
        let provider = SdkTracerProvider::builder().build();
        let cell = Arc::new(OnceLock::new());
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("test")))
            .with(
                tracing_subscriber::fmt::layer()
                    .fmt_fields(JsonFields::new())
                    .event_format(GcpFormat {
                        project: Some("p".into()),
                        service: "svc".into(),
                        version: "v1".into(),
                        dispatch: cell.clone(),
                    })
                    .with_writer(move || out.clone()),
            );
        let dispatch = Dispatch::new(subscriber);
        cell.set(dispatch.downgrade()).unwrap();
        tracing::dispatcher::with_default(&dispatch, || {
            let span = tracing::info_span!("fetch", otel.name = "x", data_type = "steps");
            let _g = span.enter();
            tracing::warn!(points = 3, "slot {}", "2026-10");
            tracing::error!("boom");
        });

        let text = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let warn = &lines[0];
        assert_eq!(warn["severity"], "WARNING");
        assert_eq!(warn["message"], "slot 2026-10");
        assert_eq!(warn["points"], 3);
        assert_eq!(warn["data_type"], "steps");
        assert!(warn.get("otel.name").is_none());
        assert!(warn.get("@type").is_none());
        let trace = warn["logging.googleapis.com/trace"].as_str().unwrap();
        assert!(trace.starts_with("projects/p/traces/") && trace.len() == 18 + 32);
        assert_eq!(
            warn["logging.googleapis.com/spanId"]
                .as_str()
                .unwrap()
                .len(),
            16
        );

        let err = &lines[1];
        assert_eq!(err["severity"], "ERROR");
        assert_eq!(err["serviceContext"]["service"], "svc");
        assert_eq!(
            err["logging.googleapis.com/trace"],
            warn["logging.googleapis.com/trace"]
        );
    }
}
