use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use guibiao_backend::alarm_ws::{AlarmConfig, AlarmWsState, run_alarm_loop};
use guibiao_backend::ClickHouseStore;
use guibiao_backend::dtu_receiver::{DtuReceiver, DtuValidationConfig};
use guibiao_backend::error_analyzer::{AnalyzerConfig, ErrorAnalyzerService};
use guibiao_backend::handlers::{create_router, HttpAppState};
use guibiao_backend::optical_simulator::{
    new_simulator_channel,
    run_simulator_loop,
};
use guibiao_backend::storage::SharedStore;

const CHANNEL_BUFFER: usize = 1024;

fn locate_config() -> PathBuf {
    let mut candidates = vec![
        std::env::current_dir().unwrap_or_default().join("config"),
        std::env::current_dir().unwrap_or_default().join("..").join("config"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("config"),
    ];
    if let Ok(p) = std::env::var("GUIBIAO_CONFIG_DIR") {
        candidates.insert(0, PathBuf::from(p));
    }
    candidates.into_iter()
        .find(|p| p.exists() && p.is_dir())
        .unwrap_or_else(|| PathBuf::from("./config"))
}

fn load_all_configs() -> (DtuValidationConfig, AnalyzerConfig, AlarmConfig) {
    let cfg_dir = locate_config();
    tracing::debug!("配置目录: {}", cfg_dir.display());

    let optics_path = cfg_dir.join("optics.json");
    let atmos_path = cfg_dir.join("atmosphere.json");

    let dtu_cfg = if optics_path.exists() {
        DtuValidationConfig::from_json_file(&optics_path).unwrap_or_default()
    } else {
        DtuValidationConfig::default()
    };

    let analyzer_cfg = if optics_path.exists() {
        AnalyzerConfig::from_json_file(&optics_path).unwrap_or_default()
    } else {
        AnalyzerConfig::default()
    };

    let alarm_cfg = if optics_path.exists() {
        AlarmConfig::from_json_file(&optics_path).unwrap_or_default()
    } else {
        AlarmConfig::default()
    };

    if atmos_path.exists() {
        tracing::info!("大气模型配置加载自: {}", atmos_path.display());
    }
    if optics_path.exists() {
        tracing::info!("光学参数配置加载自: {}", optics_path.display());
    }

    (dtu_cfg, analyzer_cfg, alarm_cfg)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "guibiao_backend=info,tower_http=info,axum=info".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    guibiao_backend::metrics::init();
    tracing::info!("Prometheus 指标已注册，端点: /metrics");

    let (dtu_cfg, analyzer_cfg, alarm_cfg) = load_all_configs();
    tracing::info!(
        "告警配置: 阈值={}寸, 去抖={}s, 分级倍数: WARNING≥{}× CRITICAL≥{}×",
        alarm_cfg.deviation_threshold_cun,
        alarm_cfg.debounce_seconds,
        alarm_cfg.warning_high_multiple,
        alarm_cfg.critical_multiple
    );
    tracing::info!(
        "蒙特卡洛默认: N={}, σ表高={}, σ折射={}, 置信水平={}",
        analyzer_cfg.default_simulation_count,
        analyzer_cfg.default_gauge_height_error_std,
        analyzer_cfg.default_refraction_error_std,
        analyzer_cfg.default_confidence_level
    );

    let clickhouse_url =
        std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());
    let clickhouse_db =
        std::env::var("CLICKHOUSE_DB").unwrap_or_else(|_| "guibiao".to_string());
    let server_port = std::env::var("SERVER_PORT").unwrap_or_else(|_| "3000".to_string());

    tracing::info!("Connecting to ClickHouse at {}", clickhouse_url);
    let store: SharedStore = Arc::new(ClickHouseStore::new(&clickhouse_url, &clickhouse_db));

    let (dtu_tx, dtu_rx) = guibiao_backend::dtu_receiver::new_dtu_channel(CHANNEL_BUFFER);
    let (sim_tx, sim_rx) = new_simulator_channel(CHANNEL_BUFFER);

    let dtu = Arc::new(DtuReceiver::new(store.clone(), dtu_cfg, dtu_tx));
    let alarm_state = AlarmWsState::new(store.clone(), alarm_cfg);
    let analyzer = Arc::new(ErrorAnalyzerService::new(store.clone(), analyzer_cfg));

    let sim_store = store.clone();
    tokio::spawn(async move {
        run_simulator_loop(sim_store, dtu_rx, sim_tx).await;
    });

    let alarm_state_clone = alarm_state.clone();
    tokio::spawn(async move {
        run_alarm_loop(alarm_state_clone, sim_rx).await;
    });

    let http_state = HttpAppState {
        store: store.clone(),
        dtu,
        analyzer,
        alarm: alarm_state,
    };
    let app: Router = create_router(http_state);

    let addr = format!("0.0.0.0:{}", server_port);
    tracing::info!("Starting HTTP+WS server on {}", addr);
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
