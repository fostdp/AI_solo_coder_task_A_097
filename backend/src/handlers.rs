use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use crate::models::{ApiResponse, MonteCarloConfig, MonteCarloResult, OpticalSimulationResult, SensorMeasurement};
use crate::monte_carlo::MonteCarloAnalyzer;
use crate::optics::OpticalSimulator;
use crate::websocket::AppState;

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

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health_check))
        .route("/api/stations", get(get_stations))
        .route("/api/stations/:id", get(get_station))
        .route("/api/measurements", get(get_measurements).post(post_measurement))
        .route("/api/measurements/latest", get(get_latest_measurements))
        .route("/api/measurements/:station_id/range", get(get_measurements_range))
        .route("/api/simulate/optics", post(simulate_optics))
        .route("/api/analyze/monte-carlo", post(run_monte_carlo))
        .route("/api/alerts", get(get_alerts))
        .route("/api/solstice/:year", get(get_winter_solstice))
        .route("/ws", get(crate::websocket::ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health_check() -> Json<ApiResponse<String>> {
    Json(ApiResponse::ok("OK".to_string()))
}

async fn get_stations(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<crate::models::Station>>> {
    match state.store.get_stations().await {
        Ok(stations) => Json(ApiResponse::ok(stations)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_station(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<ApiResponse<crate::models::Station>> {
    match state.store.get_station(&id).await {
        Ok(Some(station)) => Json(ApiResponse::ok(station)),
        Ok(None) => Json(ApiResponse::err("Station not found")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_latest_measurements(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<SensorMeasurement>>> {
    match state.store.get_latest_measurements(100).await {
        Ok(measurements) => Json(ApiResponse::ok(measurements)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_measurements(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<SensorMeasurement>>> {
    match state.store.get_latest_measurements(1000).await {
        Ok(measurements) => Json(ApiResponse::ok(measurements)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_measurements_range(
    State(state): State<AppState>,
    Path(station_id): Path<String>,
    Query(params): Query<TimeRangeQuery>,
) -> Json<ApiResponse<Vec<SensorMeasurement>>> {
    let default_start = Utc::now() - chrono::Duration::days(1);
    let default_end = Utc::now();
    let start = params.start
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(default_start);
    let end = params.end
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(default_end);
    match state.store.get_measurements_range(&station_id, start, end).await {
        Ok(measurements) => Json(ApiResponse::ok(measurements)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn post_measurement(
    State(state): State<AppState>,
    Json(mut measurement): Json<SensorMeasurement>,
) -> Json<ApiResponse<OpticalSimulationResult>> {
    if measurement.id.is_nil() {
        measurement.id = Uuid::new_v4();
    }

    let station = match state.store.get_station(&measurement.station_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Json(ApiResponse::err("Station not found"));
        }
        Err(e) => {
            return Json(ApiResponse::err(&e.to_string()));
        }
    };

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

    if let Err(e) = state.store.insert_measurement(&measurement).await {
        tracing::error!("Failed to insert measurement: {}", e);
    }
    if let Err(e) = state.store.insert_simulation(&simulation).await {
        tracing::error!("Failed to insert simulation: {}", e);
    }

    if let Some(alert) = state.check_and_trigger_alert(
        &measurement,
        simulation.refracted_shadow_length,
    ).await {
        if let Err(e) = state.store.insert_alert(&alert).await {
            tracing::error!("Failed to insert alert: {}", e);
        }
        let ws_alert = crate::models::WsMessage::alert(&alert);
        state.broadcast_message(ws_alert).await;
    }

    let ws_meas = crate::models::WsMessage::measurement(&measurement);
    state.broadcast_message(ws_meas).await;
    let ws_sim = crate::models::WsMessage::simulation(&simulation);
    state.broadcast_message(ws_sim).await;

    Json(ApiResponse::ok(simulation))
}

async fn simulate_optics(
    State(state): State<AppState>,
    Json(req): Json<SimulateRequest>,
) -> Json<ApiResponse<SimulateResponse>> {
    let station_id = "dengfeng_001";
    let station = match state.store.get_station(station_id).await {
        Ok(Some(s)) => s,
        _ => {
            return Json(ApiResponse::err("Default station not found"));
        }
    };
    let simulator = OpticalSimulator::new(
        station.latitude,
        station.longitude,
        station.altitude,
    );
    let refraction = simulator.calculate_refraction_arcsec(
        req.sun_altitude,
        req.temperature,
        req.pressure,
    );
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
    State(state): State<AppState>,
    Json(config): Json<MonteCarloConfig>,
) -> Json<ApiResponse<MonteCarloResult>> {
    let measurements = match state.store.get_latest_measurements(1).await {
        Ok(m) if !m.is_empty() => m,
        _ => return Json(ApiResponse::err("No measurements available")),
    };
    let measurement = &measurements[0];
    let station = match state.store.get_station(&measurement.station_id).await {
        Ok(Some(s)) => s,
        _ => return Json(ApiResponse::err("Station not found")),
    };
    let simulator = OpticalSimulator::new(
        station.latitude,
        station.longitude,
        station.altitude,
    );
    let analyzer = MonteCarloAnalyzer::new(simulator);
    let result = analyzer.analyze(measurement, &config);
    if let Err(e) = state.store.insert_monte_carlo(&result).await {
        tracing::error!("Failed to insert monte carlo result: {}", e);
    }
    Json(ApiResponse::ok(result))
}

async fn get_alerts(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<crate::models::AlertEvent>>> {
    match state.store.get_active_alerts().await {
        Ok(alerts) => Json(ApiResponse::ok(alerts)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_winter_solstice(
    State(state): State<AppState>,
    Path(year): Path<i32>,
) -> Json<ApiResponse<DateTime<Utc>>> {
    let station_id = "dengfeng_001";
    let station = match state.store.get_station(station_id).await {
        Ok(Some(s)) => s,
        _ => return Json(ApiResponse::err("Default station not found")),
    };
    let simulator = OpticalSimulator::new(
        station.latitude,
        station.longitude,
        station.altitude,
    );
    let solstice = simulator.find_winter_solstice(year);
    Json(ApiResponse::ok(solstice))
}
