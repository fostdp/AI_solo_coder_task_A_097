# 古代圭表测影光学仿真与冬至时刻精度分析系统

> 服务于登封观星台元代圭表复原研究的天文史团队。基于大气折射和地球曲率计算影长和冬至时刻测量精度，蒙特卡洛模拟评估测影不确定度。

## 系统架构

```
┌────────────────────────────────────────────────────────────────────────┐
│                        docker-compose 编排                             │
│                                                                        │
│  ┌──────────────┐   HTTP/WS    ┌──────────────────────────────────┐   │
│  │   frontend   │◄────────────►│           backend (Rust)          │   │
│  │  nginx:8080  │  /api/* /ws  │  Axum 0.7 + Tokio + Prometheus  │   │
│  │  Gzip·Cache  │              │  ┌─────────┐ ┌───────────────┐   │   │
│  └──────────────┘              │  │dtu_     │→│optical_       │→  │   │
│                                │  │receiver │ │simulator      │   │   │
│  ┌──────────────┐   MQTT      │  └─────────┘ └──────┬────────┘   │   │
│  │  simulator   │─────────────│                      ↓ mpsc       │   │
│  │  Python 3.12 │             │              ┌──────────────┐     │   │
│  │  altitude    │             │              │  alarm_ws    │     │   │
│  │  override    │             │              │  告警+WS推送  │     │   │
│  └──────────────┘             │              └──────────────┘     │   │
│       │                       │  ┌─────────────┐                  │   │
│       ↓                       │  │error_       │  POST 直接调用   │   │
│  ┌──────────────┐             │  │analyzer     │←─────────────    │   │
│  │  mosquitto   │             │  │MC+不确定度  │                  │   │
│  │  MQTT Broker │             │  └─────────────┘                  │   │
│  │  :1883       │             │         │ :3000                    │   │
│  └──────────────┘             └─────────┼──────────────────────────┘   │
│                                         │                              │
│                                ┌────────▼─────────┐                   │
│                                │    ClickHouse     │                   │
│                                │    :8123/:9000    │                   │
│                                │  5表+降采样+TTL   │                   │
│                                └──────────────────┘                   │
└────────────────────────────────────────────────────────────────────────┘

数据流:
  POST /api/measurements → validate → insert → simulate → insert → evaluate → broadcast
  DTU通道(预留): ingest → mpsc → simulator_loop → mpsc → alarm_loop → broadcast
  MC分析: POST /api/analyze/monte-carlo → error_analyzer → spawn_blocking → insert

指标采集:
  backend /metrics → Prometheus → Grafana
  指标: http_requests_total, measurements_received, simulations_run, alerts_generated,
        ws_connections_active, monte_carlo_analyses_total, http_request_duration_seconds
```

## 技术栈

| 层级 | 技术 |
|------|------|
| 后端 | Rust 1.77+ / Axum 0.7 / Tokio / tracing / Prometheus |
| 数据库 | ClickHouse 24.1 (MergeTree + 降采样物化视图 + TTL保留策略) |
| 前端 | HTML5 / Canvas / Three.js r128 / Nginx (Gzip压缩) |
| 消息 | MQTT (Eclipse Mosquitto) + tokio mpsc channel |
| 模拟器 | Python 3.12 / PyYAML / paho-mqtt |
| 编排 | Docker Compose (多阶段构建 / scratch静态二进制) |

## 项目结构

```
.
├── backend/                    # Rust后端
│   ├── Dockerfile              # 多阶段构建: alpine builder → scratch
│   ├── Cargo.toml              # axum+tokio+prometheus+clickhouse
│   └── src/
│       ├── main.rs             # 入口: 配置加载+mpsc管道装配+2 tokio task
│       ├── lib.rs              # 11模块导出
│       ├── metrics.rs          # Prometheus指标注册+ /metrics 端点
│       ├── handlers.rs         # 12 REST端点 + WS闭包转发 + 指标埋点
│       ├── dtu_receiver.rs     # 传感器数据采集+14项值域校验
│       ├── optical_simulator.rs# 大气折射+影长计算+mpsc事件循环
│       ├── optics.rs           # 60层球壳大气+Snell不变量+Bennett融合
│       ├── error_analyzer.rs   # MC+Bootstrap BCa+不确定度评估
│       ├── monte_carlo.rs      # Bootstrap BCa+Jackknife+probit+erf
│       ├── alarm_ws.rs         # 告警评估+WS推送+双检去抖
│       ├── models.rs           # 8种数据结构
│       ├── storage.rs          # ClickHouse 5表CRUD
│       └── websocket.rs        # 兼容导出
├── frontend/                   # 前端可视化
│   ├── Dockerfile              # nginx:1.25-alpine + Gzip
│   ├── nginx.conf              # 反向代理 /api + /ws + Gzip + 缓存
│   ├── index.html              # 主页面
│   ├── gnomon_3d.js            # Three.js 3D圭表渲染 (PCF软阴影+设备分档)
│   ├── shadow_panel.js         # Canvas 2D侧视图 (8层PCF+抗锯齿)
│   ├── app.js                  # 主调度: config加载+模块初始化+WS路由
│   └── config.json             # API/渲染/颜色/PCF配置
├── sensor_simulator/           # 圭表传感器模拟器
│   ├── Dockerfile              # python:3.12-alpine
│   ├── sensor_simulator.py     # v2.0: YAML配置+MQTT+高度角覆盖
│   ├── config.yaml             # 站点/大气/误差/输出配置
│   └── requirements.txt        # pyyaml, requests, paho-mqtt
├── config/                     # 后端参数配置
│   ├── optics.json             # 天文历表/MC参数/告警阈值/单位
│   └── atmosphere.json         # 大气分层/折射率/地球几何
├── clickhouse/                 # ClickHouse
│   ├── init.sql                # 5表+3视图+2降采样MV+5条TTL
│   └── config.xml              # 日志/内存/权限配置
├── mosquitto/                  # MQTT Broker
│   └── mosquitto.conf          # 1883端口+匿名访问
├── docker-compose.yml          # 5服务编排
└── README.md
```

## 快速部署 (Docker Compose)

### 前置条件

- Docker 20.10+
- Docker Compose V2

### 一键启动

```bash
# 启动核心服务 (ClickHouse + Backend + Frontend + Mosquitto)
docker compose up -d

# 等待ClickHouse健康检查通过 (约15秒)
docker compose logs -f clickhouse

# 启动传感器模拟器 (可选profile)
docker compose --profile simulator up -d simulator

# 查看所有服务状态
docker compose ps
```

### 访问地址

| 服务 | URL |
|------|-----|
| 前端界面 | http://localhost:8080 |
| 后端API | http://localhost:3000/api/health |
| Prometheus指标 | http://localhost:3000/metrics |
| ClickHouse HTTP | http://localhost:8123 |
| MQTT Broker | mqtt://localhost:1883 |

### 停止与清理

```bash
docker compose --profile simulator down
docker compose down -v    # 同时删除数据卷
```

## 手动部署 (无Docker)

### 1. 初始化 ClickHouse

```bash
clickhouse-client --multiline < clickhouse/init.sql
```

### 2. 启动 Rust 后端

```bash
cd backend
export CLICKHOUSE_URL="http://localhost:8123"
export CLICKHOUSE_DB="guibiao"
export SERVER_PORT="3000"
export RUST_LOG="guibiao_backend=info,tower_http=info"
cargo run --release
```

### 3. 启动前端

```bash
cd frontend
python -m http.server 8080
# 访问 http://localhost:8080
```

### 4. 运行模拟器

```bash
cd sensor_simulator
pip install -r requirements.txt
python sensor_simulator.py --config config.yaml
```

## 传感器模拟器用法

### 配置文件 (config.yaml)

```yaml
station_id: "dengfeng_001"
station_name: "登封观星台元代圭表"
latitude: 34.4897
longitude: 113.0875
altitude: 420.0
gauge_height: 40.0
mode: "realtime"           # realtime / fixed_altitude / altitude_sweep

atmosphere:
  temperature_base: 5.0         # 基准气温 °C
  temperature_amplitude: 10.0   # 日温差振幅
  temperature_noise_std: 0.5    # 随机噪声
  pressure_base: 1013.25        # 基准气压 hPa
  humidity_base: 50.0           # 基准湿度 %

altitude_override:               # 设为 null 用实时太阳位置
  # - 5                        # 固定5°高度角
  # - [5, 45]                  # 在5°~45°之间随机扫描

error_injection:
  gauge_height_noise_std: 0.005
  shadow_length_noise_std_ratio: 0.005

output:
  http_url: "http://backend:3000"
  mqtt_broker: "mqtt://mosquitto:1883"
  mqtt_topic: "guibiao/sensor/{station_id}"
  mqtt_enabled: true
```

### 命令行参数 (覆盖配置)

```bash
# 实时模式 - 模拟当天太阳轨迹
python sensor_simulator.py --config config.yaml

# 固定高度角 - 测试低角度大气折射
python sensor_simulator.py --altitude 3.5 --temperature -15

# 高度角扫描 - 5°到45°范围随机采样
python sensor_simulator.py --altitude-range 5:45

# 冬至日回放 - 从冬至正午开始
python sensor_simulator.py --start-date "2024-12-22 11:00"

# 极端大气条件 - 高海拔低温低压
python sensor_simulator.py --temperature -25 --pressure 650 --humidity 10

# MQTT输出 - 同时发布到MQTT Broker
python sensor_simulator.py --mqtt

# Docker内运行
docker compose --profile simulator up simulator
```

### 场景示例

| 场景 | 命令 |
|------|------|
| 正午测影 | `--altitude 35 --temperature 25` |
| 低角度折射测试 | `--altitude 2 --temperature -10 --pressure 1020` |
| 冬至日模拟 | `--start-date "2024-12-22 12:00"` |
| 全天扫描 | `--altitude-range 0:70` |
| 极端天气 | `--temperature -30 --pressure 850 --humidity 5` |

## Prometheus 指标

端点: `GET /metrics`

| 指标名 | 类型 | 标签 | 说明 |
|--------|------|------|------|
| `guibiao_http_requests_total` | Counter | method, path | HTTP请求总数 |
| `guibiao_http_request_duration_seconds` | Histogram | method, path | 请求延迟分布 |
| `guibiao_measurements_received_total` | Counter | station_id | 接收测量数 |
| `guibiao_simulations_run_total` | Counter | station_id | 仿真执行数 |
| `guibiao_alerts_generated_total` | Counter | level | 告警数(WARNING/CRITICAL) |
| `guibiao_ws_connections_active` | Gauge | - | 活跃WS连接数 |
| `guibiao_monte_carlo_analyses_total` | Counter | station_id | MC分析执行数 |

Grafana 推荐面板导入: 使用以上指标构建「圭表系统运行仪表盘」。

## ClickHouse 保留策略

| 表 | TTL | 说明 |
|----|-----|------|
| sensor_measurements | 90天 | 原始分钟级数据，90天后自动清理 |
| hourly_stats | 180天 | 小时聚合，6个月 |
| daily_stats | 3年 | 日降采样，长期趋势分析 |
| optical_simulations | 180天 | 仿真结果 |
| alert_events | 1年 | 告警事件 |
| monte_carlo_analysis | 2年 | MC分析结果 |

降采样链: `sensor_measurements → hourly_stats_mv → hourly_stats → daily_stats_mv → daily_stats`

## REST API

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/health` | 健康检查 |
| GET | `/api/stations` | 所有站点 |
| GET | `/api/stations/:id` | 指定站点 |
| GET | `/api/measurements/latest` | 最新100条 |
| GET | `/api/measurements/:sid/range?start=&end=` | 时间范围 |
| POST | `/api/measurements` | 上报测量 → 返回仿真结果 |
| POST | `/api/simulate/optics` | 单独光学仿真 |
| POST | `/api/analyze/monte-carlo` | 蒙特卡洛分析 |
| GET | `/api/alerts` | 活跃告警 |
| GET | `/api/solstice/:year` | 冬至时刻 |
| GET | `/metrics` | Prometheus指标 |
| GET | `/ws` | WebSocket实时推送 |

## 告警规则

影长偏差超1寸触发，分级:
- `WARNING`: 1×~2× 阈值 (1~2寸)
- `WARNING`: 2×~3× 阈值 (2~3寸)
- `CRITICAL`: ≥3× 阈值 (≥3寸)
- 同站60秒去抖，WebSocket实时推送

## 单位

| 量 | 单位 | 换算 |
|----|------|------|
| 表高 | 尺 | 元代1尺≈33.33cm |
| 影长 | 尺/寸 | 1尺=10寸，1寸≈33.33mm |
| 角度 | 度 | 1°=3600″ |
