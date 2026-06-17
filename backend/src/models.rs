use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const CHI_TO_CUN: f64 = 10.0;
const CUN_TO_MM: f64 = 33.33;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorMeasurement {
    #[serde(default)]
    pub id: Uuid,
    pub station_id: String,
    pub station_name: String,
    pub measurement_time: DateTime<Utc>,
    pub gauge_height: f64,
    pub shadow_length: f64,
    pub sun_altitude: f64,
    pub sun_azimuth: f64,
    pub atmospheric_refraction: f64,
    pub temperature: f64,
    pub pressure: f64,
    pub humidity: f64,
    #[serde(default)]
    pub is_solstice: u8,
}

impl SensorMeasurement {
    pub fn shadow_length_cun(&self) -> f64 {
        self.shadow_length * CHI_TO_CUN
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpticalSimulationResult {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    pub simulation_count: u32,
    pub gauge_height_error_std: f64,
    pub refraction_error_std: f64,
    pub confidence_level: f64,
}

impl Default for MonteCarloConfig {
    fn default() -> Self {
        Self {
            simulation_count: 10000,
            gauge_height_error_std: 0.01,
            refraction_error_std: 5.0,
            confidence_level: 0.95,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
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
    #[serde(default)]
    pub is_acknowledged: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Station {
    pub station_id: String,
    pub station_name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub standard_gauge_height: f64,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    pub message_type: String,
    pub data: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

impl WsMessage {
    pub fn measurement(m: &SensorMeasurement) -> Self {
        Self {
            message_type: "measurement".to_string(),
            data: serde_json::to_value(m).unwrap_or_default(),
            timestamp: Utc::now(),
        }
    }

    pub fn simulation(s: &OpticalSimulationResult) -> Self {
        Self {
            message_type: "simulation".to_string(),
            data: serde_json::to_value(s).unwrap_or_default(),
            timestamp: Utc::now(),
        }
    }

    pub fn alert(a: &AlertEvent) -> Self {
        Self {
            message_type: "alert".to_string(),
            data: serde_json::to_value(a).unwrap_or_default(),
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.to_string()),
        }
    }
}
