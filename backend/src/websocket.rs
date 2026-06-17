use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::{broadcast, Mutex, RwLock};
use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State,
    response::IntoResponse,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use chrono::Utc;
use uuid::Uuid;

use crate::models::{AlertEvent, SensorMeasurement, WsMessage};
use crate::storage::SharedStore;

const ALERT_THRESHOLD_CUN: f64 = 1.0;
const CHANNEL_CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct AppState {
    pub store: SharedStore,
    pub broadcast: broadcast::Sender<WsMessage>,
    pub alert_threshold_cun: f64,
    pub last_alert: Arc<RwLock<HashMap<String, chrono::DateTime<Utc>>>>,
}

impl AppState {
    pub fn new(store: SharedStore) -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            store,
            broadcast: tx,
            alert_threshold_cun: ALERT_THRESHOLD_CUN,
            last_alert: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn broadcast_message(&self, msg: WsMessage) {
        let _ = self.broadcast.send(msg);
    }

    pub async fn check_and_trigger_alert(
        &self,
        measurement: &SensorMeasurement,
        expected_shadow: f64,
    ) -> Option<AlertEvent> {
        let deviation_cun = (measurement.shadow_length - expected_shadow).abs() * 10.0;
        if deviation_cun < self.alert_threshold_cun {
            return None;
        }
        let mut last_alerts = self.last_alert.write().await;
        let now = Utc::now();
        let station_key = measurement.station_id.clone();
        if let Some(last) = last_alerts.get(&station_key) {
            if (now - *last).num_seconds() < 60 {
                return None;
            }
        }
        last_alerts.insert(station_key, now);
        let level = if deviation_cun >= 3.0 * self.alert_threshold_cun {
            "CRITICAL".to_string()
        } else if deviation_cun >= 2.0 * self.alert_threshold_cun {
            "WARNING".to_string()
        } else {
            "WARNING".to_string()
        };
        let alert = AlertEvent {
            id: Uuid::new_v4(),
            station_id: measurement.station_id.clone(),
            alert_time: now,
            alert_type: "SHADOW_DEVIATION".to_string(),
            alert_level: level,
            measured_shadow_length: measurement.shadow_length,
            expected_shadow_length: expected_shadow,
            deviation_cun,
            threshold_cun: self.alert_threshold_cun,
            message: format!(
                "影长偏差超过阈值: 测量 {:.2}尺, 预期 {:.2}尺, 偏差 {:.2}寸 (阈值 {:.2}寸)",
                measurement.shadow_length, expected_shadow, deviation_cun, self.alert_threshold_cun
            ),
            is_acknowledged: 0,
        };
        Some(alert)
    }
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.broadcast.subscribe();
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Ok(text) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
        }
        let _ = sender.close().await;
    });
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    tracing::debug!("Received WS message: {}", text);
                }
                Message::Binary(_) => {}
                Message::Ping(_) => {}
                Message::Pong(_) => {}
                Message::Close(_) => {
                    break;
                }
            }
        }
    });
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::ClickHouseStore;
    use chrono::TimeZone;

    #[tokio::test]
    async fn test_alert_threshold() {
        let store = std::sync::Arc::new(ClickHouseStore::new("http://localhost:8123", "guibiao"));
        let state = AppState::new(store);
        let measurement = SensorMeasurement {
            id: Uuid::new_v4(),
            station_id: "test".to_string(),
            station_name: "Test".to_string(),
            measurement_time: Utc::now(),
            gauge_height: 40.0,
            shadow_length: 80.0,
            sun_altitude: 30.0,
            sun_azimuth: 180.0,
            atmospheric_refraction: 1.00029,
            temperature: 10.0,
            pressure: 1013.25,
            humidity: 50.0,
            is_solstice: 0,
        };
        let alert = state.check_and_trigger_alert(&measurement, 79.8).await;
        assert!(alert.is_some());
        let deviation = alert.unwrap().deviation_cun;
        assert!(deviation >= 1.0);
    }
}
