use std::sync::Arc;
use anyhow::Result;
use chrono::{DateTime, Utc};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{AlertEvent, MonteCarloResult, OpticalSimulationResult, SensorMeasurement, Station};

#[derive(Clone)]
pub struct ClickHouseStore {
    client: Client,
}

#[derive(Row, Serialize, Deserialize)]
pub struct SensorRow {
    pub id: Uuid,
    pub station_id: String,
    pub station_name: String,
    pub measurement_time: DateTime<Utc>,
    pub gauge_height: f64,
    pub shadow_length: f64,
    pub shadow_length_cun: f64,
    pub sun_altitude: f64,
    pub sun_azimuth: f64,
    pub atmospheric_refraction: f64,
    pub temperature: f64,
    pub pressure: f64,
    pub humidity: f64,
    pub is_solstice: u8,
}

#[derive(Row, Serialize, Deserialize)]
pub struct SimulationRow {
    pub id: Uuid,
    pub measurement_id: Uuid,
    pub station_id: String,
    pub simulation_time: DateTime<Utc>,
    pub true_sun_altitude: f64,
    pub apparent_sun_altitude: f64,
    pub atmospheric_refraction_correction: f64,
    pub earth_curvature_correction: f64,
    pub theoretical_shadow_length: f64,
    pub refracted_shadow_length: f64,
    pub shadow_deviation: f64,
    pub winter_solstice_moment: Option<DateTime<Utc>>,
    pub solstice_uncertainty: f64,
}

#[derive(Row, Serialize, Deserialize)]
pub struct MonteCarloRow {
    pub id: Uuid,
    pub station_id: String,
    pub analysis_time: DateTime<Utc>,
    pub reference_time: DateTime<Utc>,
    pub simulation_count: u32,
    pub gauge_height_error_mean: f64,
    pub gauge_height_error_std: f64,
    pub refraction_error_mean: f64,
    pub refraction_error_std: f64,
    pub shadow_length_mean: f64,
    pub shadow_length_std: f64,
    pub shadow_length_95ci_low: f64,
    pub shadow_length_95ci_high: f64,
    pub solstice_time_mean: f64,
    pub solstice_time_std: f64,
    pub solstice_time_95ci_low: f64,
    pub solstice_time_95ci_high: f64,
    pub combined_uncertainty: f64,
    pub expanded_uncertainty: f64,
}

#[derive(Row, Serialize, Deserialize)]
pub struct AlertRow {
    pub id: Uuid,
    pub station_id: String,
    pub alert_time: DateTime<Utc>,
    pub alert_type: String,
    pub alert_level: String,
    pub measured_shadow_length: f64,
    pub expected_shadow_length: f64,
    pub deviation_cun: f64,
    pub threshold_cun: f64,
    pub message: String,
    pub is_acknowledged: u8,
}

impl ClickHouseStore {
    pub fn new(url: &str, database: &str) -> Self {
        let client = Client::default()
            .with_url(url)
            .with_database(database);
        Self { client }
    }

    pub async fn insert_measurement(&self, m: &SensorMeasurement) -> Result<Uuid> {
        let row = SensorRow {
            id: m.id,
            station_id: m.station_id.clone(),
            station_name: m.station_name.clone(),
            measurement_time: m.measurement_time,
            gauge_height: m.gauge_height,
            shadow_length: m.shadow_length,
            shadow_length_cun: m.shadow_length_cun(),
            sun_altitude: m.sun_altitude,
            sun_azimuth: m.sun_azimuth,
            atmospheric_refraction: m.atmospheric_refraction,
            temperature: m.temperature,
            pressure: m.pressure,
            humidity: m.humidity,
            is_solstice: m.is_solstice,
        };
        self.client.insert("sensor_measurements", row).await?;
        Ok(m.id)
    }

    pub async fn insert_simulation(&self, s: &OpticalSimulationResult) -> Result<Uuid> {
        let row = SimulationRow {
            id: s.id,
            measurement_id: s.measurement_id,
            station_id: s.station_id.clone(),
            simulation_time: s.simulation_time,
            true_sun_altitude: s.true_sun_altitude,
            apparent_sun_altitude: s.apparent_sun_altitude,
            atmospheric_refraction_correction: s.atmospheric_refraction_correction,
            earth_curvature_correction: s.earth_curvature_correction,
            theoretical_shadow_length: s.theoretical_shadow_length,
            refracted_shadow_length: s.refracted_shadow_length,
            shadow_deviation: s.shadow_deviation,
            winter_solstice_moment: s.winter_solstice_moment,
            solstice_uncertainty: s.solstice_uncertainty,
        };
        self.client.insert("optical_simulations", row).await?;
        Ok(s.id)
    }

    pub async fn insert_monte_carlo(&self, r: &MonteCarloResult) -> Result<Uuid> {
        let row = MonteCarloRow {
            id: r.id,
            station_id: r.station_id.clone(),
            analysis_time: r.analysis_time,
            reference_time: r.reference_time,
            simulation_count: r.simulation_count,
            gauge_height_error_mean: r.gauge_height_error_mean,
            gauge_height_error_std: r.gauge_height_error_std,
            refraction_error_mean: r.refraction_error_mean,
            refraction_error_std: r.refraction_error_std,
            shadow_length_mean: r.shadow_length_mean,
            shadow_length_std: r.shadow_length_std,
            shadow_length_95ci_low: r.shadow_length_95ci_low,
            shadow_length_95ci_high: r.shadow_length_95ci_high,
            solstice_time_mean: r.solstice_time_mean,
            solstice_time_std: r.solstice_time_std,
            solstice_time_95ci_low: r.solstice_time_95ci_low,
            solstice_time_95ci_high: r.solstice_time_95ci_high,
            combined_uncertainty: r.combined_uncertainty,
            expanded_uncertainty: r.expanded_uncertainty,
        };
        self.client.insert("monte_carlo_analysis", row).await?;
        Ok(r.id)
    }

    pub async fn insert_alert(&self, a: &AlertEvent) -> Result<Uuid> {
        let row = AlertRow {
            id: a.id,
            station_id: a.station_id.clone(),
            alert_time: a.alert_time,
            alert_type: a.alert_type.clone(),
            alert_level: a.alert_level.clone(),
            measured_shadow_length: a.measured_shadow_length,
            expected_shadow_length: a.expected_shadow_length,
            deviation_cun: a.deviation_cun,
            threshold_cun: a.threshold_cun,
            message: a.message.clone(),
            is_acknowledged: a.is_acknowledged,
        };
        self.client.insert("alert_events", row).await?;
        Ok(a.id)
    }

    pub async fn get_latest_measurements(&self, limit: u64) -> Result<Vec<SensorMeasurement>> {
        let rows: Vec<SensorRow> = self.client
            .query("SELECT id, station_id, station_name, measurement_time, gauge_height, shadow_length, sun_altitude, sun_azimuth, atmospheric_refraction, temperature, pressure, humidity, is_solstice FROM sensor_measurements ORDER BY measurement_time DESC LIMIT ?")
            .bind(limit)
            .fetch_all()
            .await?;
        Ok(rows.into_iter().map(|r| SensorMeasurement {
            id: r.id,
            station_id: r.station_id,
            station_name: r.station_name,
            measurement_time: r.measurement_time,
            gauge_height: r.gauge_height,
            shadow_length: r.shadow_length,
            sun_altitude: r.sun_altitude,
            sun_azimuth: r.sun_azimuth,
            atmospheric_refraction: r.atmospheric_refraction,
            temperature: r.temperature,
            pressure: r.pressure,
            humidity: r.humidity,
            is_solstice: r.is_solstice,
        }).collect())
    }

    pub async fn get_measurements_range(
        &self,
        station_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<SensorMeasurement>> {
        let rows: Vec<SensorRow> = self.client
            .query("SELECT id, station_id, station_name, measurement_time, gauge_height, shadow_length, sun_altitude, sun_azimuth, atmospheric_refraction, temperature, pressure, humidity, is_solstice FROM sensor_measurements WHERE station_id = ? AND measurement_time BETWEEN ? AND ? ORDER BY measurement_time")
            .bind(station_id)
            .bind(start)
            .bind(end)
            .fetch_all()
            .await?;
        Ok(rows.into_iter().map(|r| SensorMeasurement {
            id: r.id,
            station_id: r.station_id,
            station_name: r.station_name,
            measurement_time: r.measurement_time,
            gauge_height: r.gauge_height,
            shadow_length: r.shadow_length,
            sun_altitude: r.sun_altitude,
            sun_azimuth: r.sun_azimuth,
            atmospheric_refraction: r.atmospheric_refraction,
            temperature: r.temperature,
            pressure: r.pressure,
            humidity: r.humidity,
            is_solstice: r.is_solstice,
        }).collect())
    }

    pub async fn get_station(&self, station_id: &str) -> Result<Option<Station>> {
        let row: Option<(String, String, f64, f64, f64, f64, String)> = self.client
            .query("SELECT station_id, station_name, latitude, longitude, altitude, standard_gauge_height, location FROM stations WHERE station_id = ?")
            .bind(station_id)
            .fetch_optional()
            .await?;
        Ok(row.map(|r| Station {
            station_id: r.0,
            station_name: r.1,
            latitude: r.2,
            longitude: r.3,
            altitude: r.4,
            standard_gauge_height: r.5,
            location: r.6,
        }))
    }

    pub async fn get_stations(&self) -> Result<Vec<Station>> {
        let rows: Vec<(String, String, f64, f64, f64, f64, String)> = self.client
            .query("SELECT station_id, station_name, latitude, longitude, altitude, standard_gauge_height, location FROM stations")
            .fetch_all()
            .await?;
        Ok(rows.into_iter().map(|r| Station {
            station_id: r.0,
            station_name: r.1,
            latitude: r.2,
            longitude: r.3,
            altitude: r.4,
            standard_gauge_height: r.5,
            location: r.6,
        }).collect())
    }

    pub async fn get_active_alerts(&self) -> Result<Vec<AlertEvent>> {
        let rows: Vec<AlertRow> = self.client
            .query("SELECT id, station_id, alert_time, alert_type, alert_level, measured_shadow_length, expected_shadow_length, deviation_cun, threshold_cun, message, is_acknowledged FROM alert_events WHERE is_acknowledged = 0 ORDER BY alert_time DESC")
            .fetch_all()
            .await?;
        Ok(rows.into_iter().map(|r| AlertEvent {
            id: r.id,
            station_id: r.station_id,
            alert_time: r.alert_time,
            alert_type: r.alert_type,
            alert_level: r.alert_level,
            measured_shadow_length: r.measured_shadow_length,
            expected_shadow_length: r.expected_shadow_length,
            deviation_cun: r.deviation_cun,
            threshold_cun: r.threshold_cun,
            message: r.message,
            is_acknowledged: r.is_acknowledged,
        }).collect())
    }
}

pub type SharedStore = Arc<ClickHouseStore>;
