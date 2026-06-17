-- ============================================================
-- 古代圭表测影光学仿真与冬至时刻精度分析系统
-- ClickHouse 数据库初始化脚本
-- ============================================================

CREATE DATABASE IF NOT EXISTS guibiao
    COMMENT '圭表测影仿真数据库'
    ENGINE = Atomic;

USE guibiao;

-- ============================================================
-- 1. 圭表传感器实时测量数据表
-- ============================================================
CREATE TABLE IF NOT EXISTS sensor_measurements (
    id UUID DEFAULT generateUUIDv4(),
    station_id String COMMENT '圭表站点ID',
    station_name String COMMENT '圭表站点名称',
    measurement_time DateTime64(3, 'Asia/Shanghai') COMMENT '测量时间（毫秒精度）',
    gauge_height Float64 COMMENT '表高（尺）',
    shadow_length Float64 COMMENT '影长（尺）',
    shadow_length_cun Float64 COMMENT '影长（寸）',
    sun_altitude Float64 COMMENT '太阳高度角（度）',
    sun_azimuth Float64 COMMENT '太阳方位角（度）',
    atmospheric_refraction Float64 COMMENT '大气折射率',
    temperature Float64 COMMENT '气温（摄氏度）',
    pressure Float64 COMMENT '气压（百帕）',
    humidity Float64 COMMENT '相对湿度（%）',
    is_solstice UInt8 DEFAULT 0 COMMENT '是否冬至时刻 0-否 1-是',
    created_at DateTime64(3, 'Asia/Shanghai') DEFAULT now64(3)
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(measurement_time)
ORDER BY (station_id, measurement_time)
TTL measurement_time + INTERVAL 90 DAY
COMMENT '圭表传感器每分钟测量数据表';

-- ============================================================
-- 2. 光学仿真计算结果表
-- ============================================================
CREATE TABLE IF NOT EXISTS optical_simulations (
    id UUID DEFAULT generateUUIDv4(),
    measurement_id UUID COMMENT '关联测量记录ID',
    station_id String COMMENT '圭表站点ID',
    simulation_time DateTime64(3, 'Asia/Shanghai') COMMENT '仿真计算时间',
    true_sun_altitude Float64 COMMENT '真实太阳高度角（度，考虑大气折射前）',
    apparent_sun_altitude Float64 COMMENT '视太阳高度角（度，考虑大气折射后）',
    atmospheric_refraction_correction Float64 COMMENT '大气折射修正量（角秒）',
    earth_curvature_correction Float64 COMMENT '地球曲率修正量（尺）',
    theoretical_shadow_length Float64 COMMENT '理论影长（尺，无折射）',
    refracted_shadow_length Float64 COMMENT '折射影长（尺，含蒙气差）',
    shadow_deviation Float64 COMMENT '影长偏差（寸）',
    winter_solstice_moment DateTime64(6, 'Asia/Shanghai') COMMENT '计算的冬至精确时刻',
    solstice_uncertainty Float64 COMMENT '冬至时刻不确定度（秒）',
    created_at DateTime64(3, 'Asia/Shanghai') DEFAULT now64(3)
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(simulation_time)
ORDER BY (station_id, simulation_time)
COMMENT '光学仿真计算结果表';

-- ============================================================
-- 3. 蒙特卡洛误差分析结果表
-- ============================================================
CREATE TABLE IF NOT EXISTS monte_carlo_analysis (
    id UUID DEFAULT generateUUIDv4(),
    station_id String COMMENT '圭表站点ID',
    analysis_time DateTime64(3, 'Asia/Shanghai') COMMENT '分析时间',
    reference_time DateTime64(3, 'Asia/Shanghai') COMMENT '分析参考时间点',
    simulation_count UInt32 COMMENT '蒙特卡洛模拟次数',
    gauge_height_error_mean Float64 COMMENT '表高误差均值（尺）',
    gauge_height_error_std Float64 COMMENT '表高误差标准差（尺）',
    refraction_error_mean Float64 COMMENT '蒙气差误差均值（角秒）',
    refraction_error_std Float64 COMMENT '蒙气差误差标准差（角秒）',
    shadow_length_mean Float64 COMMENT '影长分布均值（尺）',
    shadow_length_std Float64 COMMENT '影长分布标准差（尺）',
    shadow_length_95ci_low Float64 COMMENT '影长95%置信区间下限（尺）',
    shadow_length_95ci_high Float64 COMMENT '影长95%置信区间上限（尺）',
    solstice_time_mean Float64 COMMENT '冬至时刻均值偏移（秒）',
    solstice_time_std Float64 COMMENT '冬至时刻标准差（秒）',
    solstice_time_95ci_low Float64 COMMENT '冬至时刻95%置信区间下限（秒）',
    solstice_time_95ci_high Float64 COMMENT '冬至时刻95%置信区间上限（秒）',
    combined_uncertainty Float64 COMMENT '合成标准不确定度',
    expanded_uncertainty Float64 COMMENT '扩展不确定度（k=2）',
    created_at DateTime64(3, 'Asia/Shanghai') DEFAULT now64(3)
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(analysis_time)
ORDER BY (station_id, analysis_time)
COMMENT '蒙特卡洛误差分析结果表';

-- ============================================================
-- 4. 告警事件表
-- ============================================================
CREATE TABLE IF NOT EXISTS alert_events (
    id UUID DEFAULT generateUUIDv4(),
    station_id String COMMENT '圭表站点ID',
    alert_time DateTime64(3, 'Asia/Shanghai') COMMENT '告警时间',
    alert_type String COMMENT '告警类型: SHADOW_DEVIATION/DEVICE_FAULT/WEATHER',
    alert_level String COMMENT '告警级别: INFO/WARNING/CRITICAL',
    measured_shadow_length Float64 COMMENT '测量影长（尺）',
    expected_shadow_length Float64 COMMENT '预期影长（尺）',
    deviation_cun Float64 COMMENT '偏差（寸）',
    threshold_cun Float64 COMMENT '告警阈值（寸）',
    message String COMMENT '告警消息',
    is_acknowledged UInt8 DEFAULT 0 COMMENT '是否已确认',
    acknowledged_at Nullable(DateTime64(3, 'Asia/Shanghai')),
    created_at DateTime64(3, 'Asia/Shanghai') DEFAULT now64(3)
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(alert_time)
ORDER BY (station_id, alert_time, alert_level)
COMMENT '告警事件表';

-- ============================================================
-- 5. 站点元数据表
-- ============================================================
CREATE TABLE IF NOT EXISTS stations (
    station_id String COMMENT '站点ID',
    station_name String COMMENT '站点名称',
    latitude Float64 COMMENT '纬度（度）',
    longitude Float64 COMMENT '经度（度）',
    altitude Float64 COMMENT '海拔高度（米）',
    standard_gauge_height Float64 COMMENT '标准表高（尺）',
    location String COMMENT '位置描述',
    created_at DateTime64(3, 'Asia/Shanghai') DEFAULT now64(3),
    updated_at DateTime64(3, 'Asia/Shanghai') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY station_id
PRIMARY KEY station_id
COMMENT '圭表站点元数据表';

-- ============================================================
-- 插入登封观星台默认站点数据
-- ============================================================
INSERT INTO stations (station_id, station_name, latitude, longitude, altitude, standard_gauge_height, location)
VALUES (
    'dengfeng_001',
    '登封观星台元代圭表',
    34.4897,
    113.0875,
    420.0,
    40.0,
    '河南省登封市告成镇，元代郭守敬所建'
);

-- ============================================================
-- 创建视图：最新测量数据
-- ============================================================
CREATE VIEW IF NOT EXISTS latest_measurements AS
SELECT
    sm.station_id,
    sm.station_name,
    sm.measurement_time,
    sm.gauge_height,
    sm.shadow_length,
    sm.shadow_length_cun,
    sm.sun_altitude,
    sm.sun_azimuth,
    sm.atmospheric_refraction,
    sm.temperature,
    sm.pressure,
    sm.humidity,
    sm.is_solstice
FROM sensor_measurements sm
INNER JOIN (
    SELECT station_id, max(measurement_time) as max_time
    FROM sensor_measurements
    GROUP BY station_id
) lm ON sm.station_id = lm.station_id AND sm.measurement_time = lm.max_time;

-- ============================================================
-- 创建视图：未处理告警
-- ============================================================
CREATE VIEW IF NOT EXISTS active_alerts AS
SELECT *
FROM alert_events
WHERE is_acknowledged = 0
ORDER BY alert_time DESC;

-- ============================================================
-- 创建聚合视图：小时统计
-- ============================================================
CREATE MATERIALIZED VIEW IF NOT EXISTS hourly_stats_mv
TO hourly_stats
AS
SELECT
    station_id,
    toStartOfHour(measurement_time) AS hour_start,
    count() AS measurement_count,
    avg(shadow_length) AS avg_shadow_length,
    min(shadow_length) AS min_shadow_length,
    max(shadow_length) AS max_shadow_length,
    avg(sun_altitude) AS avg_sun_altitude,
    max(sun_altitude) AS max_sun_altitude,
    avg(temperature) AS avg_temperature
FROM sensor_measurements
GROUP BY station_id, hour_start;

CREATE TABLE IF NOT EXISTS hourly_stats (
    station_id String,
    hour_start DateTime64(3, 'Asia/Shanghai'),
    measurement_count UInt64,
    avg_shadow_length Float64,
    min_shadow_length Float64,
    max_shadow_length Float64,
    avg_sun_altitude Float64,
    max_sun_altitude Float64,
    avg_temperature Float64
)
ENGINE = SummingMergeTree()
PARTITION BY toYYYYMM(hour_start)
ORDER BY (station_id, hour_start);

-- ============================================================
-- 6. 日降采样统计表（从小时聚合再聚合为日级）
-- ============================================================
CREATE TABLE IF NOT EXISTS daily_stats (
    station_id String,
    day_start Date COMMENT '日期',
    measurement_count UInt64,
    avg_shadow_length Float64,
    min_shadow_length Float64,
    max_shadow_length Float64,
    avg_sun_altitude Float64,
    max_sun_altitude Float64,
    avg_temperature Float64,
    alert_count UInt64 DEFAULT 0
)
ENGINE = SummingMergeTree()
PARTITION BY toYYYYMM(day_start)
ORDER BY (station_id, day_start)
TTL day_start + INTERVAL 3 YEAR
COMMENT '日级降采样统计表，保留3年';

-- ============================================================
-- 日降采样物化视图：小时→日聚合
-- ============================================================
CREATE MATERIALIZED VIEW IF NOT EXISTS daily_stats_mv
TO daily_stats
AS
SELECT
    station_id,
    toDate(hour_start) AS day_start,
    sum(measurement_count) AS measurement_count,
    avg(avg_shadow_length) AS avg_shadow_length,
    min(min_shadow_length) AS min_shadow_length,
    max(max_shadow_length) AS max_shadow_length,
    avg(avg_sun_altitude) AS avg_sun_altitude,
    max(max_sun_altitude) AS max_sun_altitude,
    avg(avg_temperature) AS avg_temperature,
    0 AS alert_count
FROM hourly_stats
GROUP BY station_id, day_start;

-- ============================================================
-- 仿真结果保留策略：6个月后移至冷存储分区
-- ============================================================
ALTER TABLE optical_simulations MODIFY TTL simulation_time + INTERVAL 180 DAY;

-- ============================================================
-- 告警事件保留策略：1年
-- ============================================================
ALTER TABLE alert_events MODIFY TTL alert_time + INTERVAL 1 YEAR;

-- ============================================================
-- 蒙特卡洛分析结果保留：2年
-- ============================================================
ALTER TABLE monte_carlo_analysis MODIFY TTL analysis_time + INTERVAL 2 YEAR;

-- ============================================================
-- 小时统计保留：6个月
-- ============================================================
ALTER TABLE hourly_stats MODIFY TTL hour_start + INTERVAL 180 DAY;
