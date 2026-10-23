//! Prototype dashboard: JSON API over the in-memory dataset plus a single page.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
use chrono::{Duration, NaiveDate, NaiveTime};
use serde_json::{Value, json};
use tokio::sync::RwLock;

use crate::model::{Dataset, bin_stats, bin_sum, window};

const INDEX: &str = include_str!("../web/index.html");
/// Served by the app itself so the page works without access to a CDN.
const ECHARTS: &str = include_str!("../web/vendor/echarts-5.5.1.min.js");

struct AppState {
    dir: PathBuf,
    data: RwLock<Dataset>,
}

type Shared = Arc<AppState>;

pub async fn serve(dir: PathBuf, port: u16) -> Result<()> {
    let data = Dataset::load(&dir)?;
    tracing::info!(
        "loaded {} heart-rate samples, {} days",
        data.heart_rate.len(),
        data.daily.len()
    );
    let state = Arc::new(AppState {
        dir,
        data: RwLock::new(data),
    });

    let app = Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route(
            "/vendor/echarts-5.5.1.min.js",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                        // Versioned file name, so it never changes.
                        (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                    ],
                    ECHARTS,
                )
            }),
        )
        .route("/api/overview", get(overview))
        .route("/api/day/{date}", get(day))
        .route("/api/reload", post(reload))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    println!("Dashboard on http://0.0.0.0:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn internal(e: anyhow::Error) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
}

async fn reload(State(s): State<Shared>) -> Response {
    match Dataset::load(&s.dir) {
        Ok(d) => {
            *s.data.write().await = d;
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => internal(e),
    }
}

/// All days with their metrics, oldest first.
async fn overview(State(s): State<Shared>) -> Json<Value> {
    let d = s.data.read().await;
    Json(json!({ "days": d.daily.values().collect::<Vec<_>>() }))
}

/// Detail of one day. The time range starts at the beginning of the night
/// before (if there was one) so the sleep that ended that morning is shown.
async fn day(State(s): State<Shared>, Path(date): Path<NaiveDate>) -> Response {
    let d = s.data.read().await;
    let midnight = date.and_time(NaiveTime::MIN);
    let end = midnight + Duration::days(1);
    let night = d.night_before(date);
    let start = night.as_ref().map_or(midnight, |n| n.start.min(midnight));
    // Night-only charts: the sleep window, or the small hours if no sleep was recorded.
    let (night_from, night_to) = night
        .as_ref()
        .map_or((midnight, midnight + Duration::hours(8)), |n| {
            (n.start, n.end)
        });

    let hr: Vec<Value> = bin_stats(window(&d.heart_rate, start, end), 1)
        .into_iter()
        .map(|(t, lo, avg, hi)| json!([t, lo, avg, hi]))
        .collect();
    let steps: Vec<Value> = bin_sum(window(&d.steps, midnight, end), 15)
        .into_iter()
        .map(|(t, v)| json!([t, v]))
        .collect();
    let pts = |s: &[(chrono::NaiveDateTime, f64)]| -> Vec<Value> {
        window(s, night_from, night_to)
            .iter()
            .map(|(t, v)| json!([t, v]))
            .collect()
    };
    let exercises: Vec<_> = d
        .exercise
        .iter()
        .filter(|e| e.start >= start && e.start < end)
        .collect();

    Json(json!({
        "date": date,
        "range": [start, end],
        "metrics": d.daily.get(&date),
        "heartRate": hr,
        "steps": steps,
        "spo2": pts(&d.spo2),
        "hrv": pts(&d.hrv),
        "sleep": night,
        "exercises": exercises,
    }))
    .into_response()
}
