# 古代圭表测影光学仿真与冬至时刻精度分析系统

## 项目概述

本系统为某天文史团队对登封观星台元代圭表进行复原研究而开发，实现了基于大气折射和地球曲率的测影光学仿真模型，以及基于蒙气差和表高误差的蒙特卡洛误差分析。

## 技术栈

| 层级 | 技术 |
|------|------|
| 后端 | Rust 1.75+ / Axum / Tokio |
| 数据库 | ClickHouse 23.x |
| 前端 | HTML5 / Canvas / Three.js r128 |
| 模拟器 | Python 3.9+ |
| 通信 | REST API + WebSocket |

## 项目结构

```
.
├── backend/                 # Rust后端服务
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs          # 程序入口
│       ├── lib.rs           # 模块导出
│       ├── models.rs        # 数据模型
│       ├── optics.rs        # 光学仿真模型
│       ├── monte_carlo.rs   # 蒙特卡洛误差分析
│       ├── storage.rs       # ClickHouse存储层
│       ├── websocket.rs     # WebSocket与告警服务
│       └── handlers.rs      # REST API处理器
├── frontend/                # 前端可视化
│   ├── index.html
│   └── app.js
├── sensor_simulator/        # 圭表传感器模拟器
│   └── sensor_simulator.py
└── clickhouse/              # 数据库初始化脚本
    └── init.sql
```

## 核心算法

### 1. 光学仿真模型（optics.rs）

**大气折射修正**（基于标准大气公式）：
```
R(h) = 1.02 / tan(h + 10.3/(h+5.11)) * (P*T0)/(P0*T)
```
其中 h 为视高度角，P为气压，T为绝对温度。

**Bennett蒙气差公式**（低高度角精确计算）：
```
R(arcmin) = 1.0 / tan(z + 7.31/(z+4.4)) * (P/1013.25) * (288.15/T)
```

**地球曲率修正**：
```
Δh = R - √(R² - s²)
```
其中R=6371km为地球半径，s为影长对应的地表距离。

**影长公式**：
```
L = H / tan(α)
```
H为表高，α为太阳高度角。

### 2. 蒙特卡洛误差分析（monte_carlo.rs）

误差源建模：
- 表高误差：N(0, σ_h²)，默认σ=0.01尺
- 蒙气差误差：N(0, σ_r²)，默认σ=5角秒
- 模拟次数：10000次，95%置信区间

输出统计量：均值、标准差、95%置信区间、合成标准不确定度、扩展不确定度(k=2)

## 快速启动

### 1. 初始化ClickHouse

```bash
clickhouse-client --multiline < clickhouse/init.sql
```

### 2. 启动Rust后端

```bash
cd backend
export CLICKHOUSE_URL="http://localhost:8123"
export CLICKHOUSE_DB="guibiao"
export SERVER_PORT="3000"
cargo run --release
```

### 3. 运行传感器模拟器

```bash
cd sensor_simulator
pip install requests
python sensor_simulator.py --url http://localhost:3000 --interval 3
```

参数说明：
- `--url`: 后端API地址
- `--interval`: 上报间隔秒数（模拟每分钟）
- `--start-date "2023-12-22 12:00"`: 指定起始日期
- `--count 100`: 上报指定次数后退出

### 4. 打开前端

直接用浏览器打开 `frontend/index.html`，或通过静态文件服务器：

```bash
cd frontend
python -m http.server 8080
# 访问 http://localhost:8080
```

## REST API

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/health` | 健康检查 |
| GET | `/api/stations` | 获取所有站点 |
| GET | `/api/stations/:id` | 获取指定站点 |
| GET | `/api/measurements/latest` | 最新100条测量 |
| GET | `/api/measurements/:station_id/range?start=&end=` | 时间范围查询 |
| POST | `/api/measurements` | 上报测量数据，返回仿真结果 |
| POST | `/api/simulate/optics` | 单独光学仿真计算 |
| POST | `/api/analyze/monte-carlo` | 运行蒙特卡洛分析 |
| GET | `/api/alerts` | 获取活跃告警 |
| GET | `/api/solstice/:year` | 计算指定年份冬至时刻 |
| GET | `/ws` | WebSocket实时推送 |

### POST /api/measurements 请求示例

```json
{
  "station_id": "dengfeng_001",
  "station_name": "登封观星台元代圭表",
  "measurement_time": "2023-12-22T04:35:00Z",
  "gauge_height": 40.0,
  "shadow_length": 88.235,
  "sun_altitude": 24.5123,
  "sun_azimuth": 180.0,
  "atmospheric_refraction": 1.000293,
  "temperature": 5.2,
  "pressure": 1013.25,
  "humidity": 50.0,
  "is_solstice": 1
}
```

## WebSocket推送消息

三种消息类型：
- `measurement`: 测量数据
- `simulation`: 光学仿真结果
- `alert`: 告警事件（影长偏差≥1寸触发）

## 数据库表设计

详见 [clickhouse/init.sql](file:///c:/AI_solo_coder_task_A/AI_solo_coder_task_A_097/clickhouse/init.sql)：

- `sensor_measurements`: 传感器原始测量（TTL 1年）
- `optical_simulations`: 光学仿真计算结果
- `monte_carlo_analysis`: 蒙特卡洛误差分析结果
- `alert_events`: 告警事件
- `stations`: 站点元数据（登封观星台已预置）
- `hourly_stats`: 小时聚合统计（物化视图）

## 告警规则

影长测量偏差超过1寸触发告警，分级：
- `WARNING`: 1~2寸偏差
- `CRITICAL`: >3寸偏差
- 同站60秒内不重复告警
- 通过WebSocket实时推送前端

## 单位说明

| 量 | 单位 | 换算 |
|----|------|------|
| 表高 | 尺 | 元代1尺≈33.33cm |
| 影长 | 尺/寸 | 1尺=10寸 |
| 角度 | 度 | 1度=3600角秒 |
| 温度 | 摄氏度 | |
| 气压 | 百帕(hPa) | |
