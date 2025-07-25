use crate::monitoring::{MetricsCollector, ProductionMetrics};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, Json},
    routing::get,
    Router,
};
use prometheus::TextEncoder;
use reqwest::Client;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct AppState {
    pub metrics_collector: Arc<RwLock<MetricsCollector>>,
}

#[derive(Clone)]
pub struct DashboardState {
    pub http_client: Client,
    pub mdi_endpoint: String,
    pub cached_metrics: Arc<RwLock<Option<ProductionMetrics>>>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/api/health", get(health_handler))
        .route("/api/metrics", get(metrics_handler))
        .route("/api/metrics/prometheus", get(prometheus_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub fn create_dashboard_router(state: DashboardState) -> Router {
    Router::new()
        .route("/", get(dashboard_client_handler))
        .route("/api/health", get(dashboard_health_handler))
        .route("/api/metrics", get(dashboard_metrics_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn dashboard_handler() -> Html<&'static str> {
    Html(include_str!("../static/dashboard.html"))
}

async fn dashboard_client_handler() -> Html<&'static str> {
    Html(include_str!("../static/dashboard.html"))
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "market-data-ingestor",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn dashboard_health_handler(State(state): State<DashboardState>) -> Json<serde_json::Value> {
    let can_reach_mdi = state
        .http_client
        .get(format!("{}/api/health", state.mdi_endpoint))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .is_ok();

    Json(serde_json::json!({
        "status": if can_reach_mdi { "healthy" } else { "degraded" },
        "service": "mdi-dashboard",
        "mdi_endpoint": state.mdi_endpoint,
        "mdi_reachable": can_reach_mdi,
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn metrics_handler(
    State(state): State<AppState>,
) -> Result<Json<ProductionMetrics>, StatusCode> {
    match state.metrics_collector.read() {
        Ok(collector) => {
            let metrics = collector.get_production_metrics("unknown", true);
            Ok(Json(metrics))
        }
        Err(e) => {
            error!("Failed to read metrics: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn dashboard_metrics_handler(
    State(state): State<DashboardState>,
) -> Result<Json<ProductionMetrics>, StatusCode> {
    if let Ok(cached) = state.cached_metrics.read() {
        if let Some(ref metrics) = *cached {
            return Ok(Json(metrics.clone()));
        }
    }

    error!("No cached metrics available");
    Err(StatusCode::SERVICE_UNAVAILABLE)
}

async fn prometheus_handler(State(state): State<AppState>) -> Result<String, StatusCode> {
    match state.metrics_collector.read() {
        Ok(collector) => {
            let encoder = TextEncoder::new();
            let metric_families = collector.registry().gather();

            match encoder.encode_to_string(&metric_families) {
                Ok(output) => Ok(output),
                Err(e) => {
                    error!("Failed to encode Prometheus metrics: {}", e);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Err(e) => {
            error!("Failed to read metrics collector: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn start_metrics_server(
    bind_address: String,
    metrics_collector: Arc<RwLock<MetricsCollector>>,
) -> Result<(), anyhow::Error> {
    let app_state = AppState { metrics_collector };
    let app = create_router(app_state);

    info!("Starting metrics server on {}", bind_address);

    let listener = tokio::net::TcpListener::bind(&bind_address).await?;

    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("HTTP server error: {}", e))?;

    Ok(())
}

pub async fn start_dashboard_server(
    bind_address: String,
    mdi_endpoint: String,
    refresh_interval_seconds: u64,
) -> Result<(), anyhow::Error> {
    let http_client = Client::new();
    let cached_metrics = Arc::new(RwLock::new(None));

    let state = DashboardState {
        http_client: http_client.clone(),
        mdi_endpoint: mdi_endpoint.clone(),
        cached_metrics: cached_metrics.clone(),
    };

    // Start background task to fetch metrics from MDI
    let metrics_client = http_client.clone();
    let metrics_endpoint = mdi_endpoint.clone();
    let metrics_cache = cached_metrics.clone();
    tokio::spawn(async move {
        fetch_metrics_task(
            metrics_client,
            metrics_endpoint,
            metrics_cache,
            refresh_interval_seconds,
        )
        .await;
    });

    let app = create_dashboard_router(state);

    info!("Starting dashboard server on {}", bind_address);
    info!("Monitoring MDI instance at: {}", mdi_endpoint);

    let listener = tokio::net::TcpListener::bind(&bind_address).await?;

    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("Dashboard server error: {}", e))?;

    Ok(())
}

async fn fetch_metrics_task(
    client: Client,
    mdi_endpoint: String,
    cached_metrics: Arc<RwLock<Option<ProductionMetrics>>>,
    refresh_interval_seconds: u64,
) {
    let mut interval = time::interval(Duration::from_secs(refresh_interval_seconds));

    loop {
        interval.tick().await;

        match client
            .get(format!("{}/api/metrics", mdi_endpoint))
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(response) => match response.json::<ProductionMetrics>().await {
                Ok(metrics) => {
                    if let Ok(mut cache) = cached_metrics.write() {
                        *cache = Some(metrics);
                    }
                }
                Err(e) => {
                    warn!("Failed to parse metrics response: {}", e);
                }
            },
            Err(e) => {
                warn!("Failed to fetch metrics from {}: {}", mdi_endpoint, e);
            }
        }
    }
}
