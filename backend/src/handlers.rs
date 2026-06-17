use std::sync::Arc;

use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{Json, IntoResponse},
    routing::{get, post},
    Router,
};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use crate::alarm_ws::AlarmWsState;
use crate::dtu_receiver::DtuReceiver;
use crate::error_analyzer::SharedErrorAnalyzer;
use crate::models::{
    ApiResponse, MonteCarloConfig, MonteCarloResult, OpticalSimulationResult, SensorMeasurement,
};
use crate::optics::OpticalSimulator;
use crate::storage::SharedStore;

const DEFAULT_STATION_ID: &str = "dengfeng_001";

#[derive(Clone)]
pub struct HttpAppState {
    pub store: SharedStore,
    pub dtu: Arc<DtuReceiver>,
    pub analyzer: SharedErrorAnalyzer,
    pub alarm: AlarmWsState,
}

#[derive(Debug, Deserialize)]
pub struct TimeRangeQuery {
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SimulateRequest {
    pub gauge_height: f64,
    pub sun_altitude: f64,
    pub temperature: f64,
    pub pressure: f64,
}

#[derive(Debug, Serialize)]
pub struct SimulateResponse {
    pub theoretical_shadow: f64,
    pub refracted_shadow: f64,
    pub refraction_correction_arcsec: f64,
    pub earth_curvature_correction: f64,
}

pub fn create_router(state: HttpAppState) -> Router {
    Router::new()
        .route("/api/health", get(health_check))
        .route("/api/stations", get(get_stations))
        .route("/api/stations/:id", get(get_station))
        .route(
            "/api/measurements",
            get(get_measurements).post(post_measurement),
        )
        .route("/api/measurements/latest", get(get_latest_measurements))
        .route(
            "/api/measurements/:station_id/range",
            get(get_measurements_range),
        )
        .route("/api/simulate/optics", post(simulate_optics))
        .route("/api/analyze/monte-carlo", post(run_monte_carlo))
        .route("/api/alerts", get(get_alerts))
        .route("/api/solstice/:year", get(get_winter_solstice))
        .route("/metrics", get(crate::metrics::metrics_handler))
        .route("/ws", get(|ws: WebSocketUpgrade, State(s): State<HttpAppState>| async move {
            crate::alarm_ws::ws_handler(ws, s.alarm).await
        }))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health_check() -> Json<ApiResponse<String>> {
    Json(ApiResponse::ok("OK".to_string()))
}

async fn get_stations(
    State(state): State<HttpAppState>,
) -> Json<ApiResponse<Vec<crate::models::Station>>> {
    match state.store.get_stations().await {
        Ok(stations) => Json(ApiResponse::ok(stations)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_station(
    State(state): State<HttpAppState>,
    Path(id): Path<String>,
) -> Json<ApiResponse<crate::models::Station>> {
    match state.store.get_station(&id).await {
        Ok(Some(station)) => Json(ApiResponse::ok(station)),
        Ok(None) => Json(ApiResponse::err("Station not found")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_latest_measurements(
    State(state): State<HttpAppState>,
) -> Json<ApiResponse<Vec<SensorMeasurement>>> {
    match state.store.get_latest_measurements(100).await {
        Ok(measurements) => Json(ApiResponse::ok(measurements)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_measurements(
    State(state): State<HttpAppState>,
) -> Json<ApiResponse<Vec<SensorMeasurement>>> {
    match state.store.get_latest_measurements(1000).await {
        Ok(measurements) => Json(ApiResponse::ok(measurements)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_measurements_range(
    State(state): State<HttpAppState>,
    Path(station_id): Path<String>,
    Query(params): Query<TimeRangeQuery>,
) -> Json<ApiResponse<Vec<SensorMeasurement>>> {
    let default_start = Utc::now() - chrono::Duration::days(1);
    let default_end = Utc::now();
    let start = params
        .start
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(default_start);
    let end = params
        .end
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(default_end);
    match state
        .store
        .get_measurements_range(&station_id, start, end)
        .await
    {
        Ok(measurements) => Json(ApiResponse::ok(measurements)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn post_measurement(
    State(state): State<HttpAppState>,
    Json(mut measurement): Json<SensorMeasurement>,
) -> Json<ApiResponse<OpticalSimulationResult>> {
    if measurement.id.is_nil() {
        measurement.id = Uuid::new_v4();
    }

    let station_id = measurement.station_id.clone();
    let station = match state.store.get_station(&station_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return Json(ApiResponse::err("Station not found")),
        Err(e) => return Json(ApiResponse::err(&e.to_string())),
    };

    if let Err(e) = state.dtu.validate(&measurement) {
        return Json(ApiResponse::err(&format!("校验失败: {}", e)));
    }

    crate::metrics::MEASUREMENTS_RECEIVED
        .with_label_values(&[&measurement.station_id])
        .inc();

    if let Err(e) = state.store.insert_measurement(&measurement).await {
        tracing::error!("测量数据入库失败: {}", e);
    }

    let simulator = OpticalSimulator::new(
        station.latitude,
        station.longitude,
        station.altitude,
    );
    let simulation = simulator.simulate_optics(
        measurement.id,
        &measurement.station_id,
        measurement.gauge_height,
        measurement.shadow_length,
        measurement.sun_altitude,
        measurement.temperature,
        measurement.pressure,
        measurement.measurement_time,
    );

    if let Err(e) = state.store.insert_simulation(&simulation).await {
        tracing::error!("仿真结果入库失败: {}", e);
    }

    crate::metrics::SIMULATIONS_RUN
        .with_label_values(&[&measurement.station_id])
        .inc();

    if let Some(alert) = state
        .alarm
        .evaluate(&measurement, simulation.refracted_shadow_length)
        .await
    {
        if let Err(e) = state.store.insert_alert(&alert).await {
            tracing::error!("告警入库失败: {}", e);
        }
        let ws_alert = crate::models::WsMessage::alert(&alert);
        state.alarm.broadcast_message(ws_alert).await;
        crate::metrics::ALERTS_GENERATED
            .with_label_values(&[alert.alert_level.as_str()])
            .inc();
    }

    let ws_meas = crate::models::WsMessage::measurement(&measurement);
    state.alarm.broadcast_message(ws_meas).await;
    let ws_sim = crate::models::WsMessage::simulation(&simulation);
    state.alarm.broadcast_message(ws_sim).await;

    Json(ApiResponse::ok(simulation))
}

async fn simulate_optics(
    State(state): State<HttpAppState>,
    Json(req): Json<SimulateRequest>,
) -> Json<ApiResponse<SimulateResponse>> {
    let station = match state.store.get_station(DEFAULT_STATION_ID).await {
        Ok(Some(s)) => s,
        _ => return Json(ApiResponse::err("Default station not found")),
    };
    let simulator =
        OpticalSimulator::new(station.latitude, station.longitude, station.altitude);
    let refraction =
        simulator.calculate_refraction_arcsec(req.sun_altitude, req.temperature, req.pressure);
    let true_alt = req.sun_altitude - refraction / 3600.0;
    let theoretical = simulator.shadow_length_from_altitude(req.gauge_height, true_alt);
    let refracted = simulator.shadow_length_from_altitude(req.gauge_height, req.sun_altitude);
    let curvature = simulator.earth_curvature_correction(theoretical);
    Json(ApiResponse::ok(SimulateResponse {
        theoretical_shadow: theoretical,
        refracted_shadow: refracted,
        refraction_correction_arcsec: refraction,
        earth_curvature_correction: curvature,
    }))
}

async fn run_monte_carlo(
    State(state): State<HttpAppState>,
    Json(config): Json<MonteCarloConfig>,
) -> Json<ApiResponse<MonteCarloResult>> {
    match state.analyzer.analyze_from_store(config).await {
        Ok(result) => {
            crate::metrics::MC_ANALYSES_RUN
                .with_label_values(&[&result.station_id])
                .inc();
            Json(ApiResponse::ok(result))
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_alerts(
    State(state): State<HttpAppState>,
) -> Json<ApiResponse<Vec<crate::models::AlertEvent>>> {
    match state.store.get_active_alerts().await {
        Ok(alerts) => Json(ApiResponse::ok(alerts)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_winter_solstice(
    State(state): State<HttpAppState>,
    Path(year): Path<i32>,
) -> Json<ApiResponse<DateTime<Utc>>> {
    let station = match state.store.get_station(DEFAULT_STATION_ID).await {
        Ok(Some(s)) => s,
        _ => return Json(ApiResponse::err("Default station not found")),
    };
    let simulator =
        OpticalSimulator::new(station.latitude, station.longitude, station.altitude);
    let solstice = simulator.find_winter_solstice(year);
    Json(ApiResponse::ok(solstice))
}
