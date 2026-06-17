#!/usr/bin/env python3
"""
古代圭表测影光学仿真系统 - 传感器模拟器
支持：实时太阳位置计算 / 固定高度角覆盖 / YAML配置 / MQTT+HTTP双通道
"""

import json
import time
import math
import random
import argparse
import sys
import yaml

from datetime import datetime, timezone, timedelta

try:
    import requests
except ImportError:
    requests = None

try:
    import paho.mqtt.client as mqtt_client
except ImportError:
    mqtt_client = None

DEG_TO_RAD = math.pi / 180.0
RAD_TO_DEG = 180.0 / math.pi
CST = timezone(timedelta(hours=8))


def day_of_year(dt):
    return dt.timetuple().tm_yday


def sun_declination(dt):
    doy = day_of_year(dt)
    gamma = 2.0 * math.pi * (doy - 1) / 365.0
    return 23.45 * math.sin(gamma + 0.0733 - 0.0068)


def equation_of_time(dt):
    doy = day_of_year(dt)
    b = 2.0 * math.pi * (doy - 81.0) / 365.0
    return 9.87 * math.sin(2 * b) - 7.53 * math.cos(b) - 1.5 * math.sin(b)


def solar_hour_angle(dt, lon):
    eot = equation_of_time(dt)
    lstm = 15.0 * round(lon / 15.0)
    tc = 4.0 * (lon - lstm) + eot
    utc_dt = dt.astimezone(timezone.utc)
    lst = (utc_dt.hour + 8) + utc_dt.minute / 60.0 + utc_dt.second / 3600.0 + tc / 60.0
    return 15.0 * (lst - 12.0)


def solar_altitude(dt, lat, lon):
    decl = sun_declination(dt)
    lat_rad = lat * DEG_TO_RAD
    decl_rad = decl * DEG_TO_RAD
    hour = solar_hour_angle(dt, lon)
    hour_rad = hour * DEG_TO_RAD
    sin_alt = (math.sin(lat_rad) * math.sin(decl_rad) +
               math.cos(lat_rad) * math.cos(decl_rad) * math.cos(hour_rad))
    return math.asin(max(-1.0, min(1.0, sin_alt))) * RAD_TO_DEG


def solar_azimuth(dt, lat, lon):
    alt = solar_altitude(dt, lat, lon)
    decl = sun_declination(dt)
    lat_rad = lat * DEG_TO_RAD
    alt_rad = alt * DEG_TO_RAD
    decl_rad = decl * DEG_TO_RAD
    cos_azi = ((math.sin(decl_rad) - math.sin(alt_rad) * math.sin(lat_rad)) /
               (math.cos(alt_rad) * math.cos(lat_rad)))
    cos_azi = max(-1.0, min(1.0, cos_azi))
    azi = math.acos(cos_azi) * RAD_TO_DEG
    hour = solar_hour_angle(dt, lon)
    return 360.0 - azi if hour > 0 else azi


def atmospheric_refraction(apparent_alt, temp_c, pressure_hpa):
    if apparent_alt <= -1.0:
        return 0.0
    h = max(apparent_alt, 0.5)
    t = temp_c + 273.15
    p = pressure_hpa
    base = 1.02 / math.tan((h + 10.3 / (h + 5.11)) * DEG_TO_RAD)
    return base * (p * 288.15) / (1013.25 * t) * RAD_TO_DEG


def refractive_index(temp_c, pressure_hpa, humidity_pct):
    t = temp_c + 273.15
    p = pressure_hpa
    es = 6.112 * math.exp((17.67 * temp_c) / (temp_c + 243.5))
    e = es * humidity_pct / 100.0
    return 1.0 + 77.624e-6 * p / t - 12.92e-6 * e / t


def shadow_length(gauge_height, altitude_deg):
    if altitude_deg <= 0.001:
        return 999.999
    return gauge_height / math.tan(altitude_deg * DEG_TO_RAD)


class SensorSimulator:
    def __init__(self, config_path):
        with open(config_path, 'r', encoding='utf-8') as f:
            self.cfg = yaml.safe_load(f)

        self.station_id = self.cfg.get('station_id', 'dengfeng_001')
        self.station_name = self.cfg.get('station_name', '登封观星台')
        self.lat = self.cfg.get('latitude', 34.4897)
        self.lon = self.cfg.get('longitude', 113.0875)
        self.alt = self.cfg.get('altitude', 420.0)
        self.gauge_height = self.cfg.get('gauge_height', 40.0)
        self.mode = self.cfg.get('mode', 'realtime')
        self.atm = self.cfg.get('atmosphere', {})
        self.err = self.cfg.get('error_injection', {})
        self.interval = self.cfg.get('interval_seconds', 3)
        self.count = self.cfg.get('count', 0)
        self.alt_override = self.cfg.get('altitude_override', None)

        out = self.cfg.get('output', {})
        self.http_url = out.get('http_url', 'http://localhost:3000')
        self.mqtt_broker = out.get('mqtt_broker', 'mqtt://localhost:1883')
        self.mqtt_topic = out.get('mqtt_topic', f'guibiao/sensor/{self.station_id}')
        self.mqtt_enabled = out.get('mqtt_enabled', False)

        self.mqtt = None
        if self.mqtt_enabled and mqtt_client:
            self._init_mqtt()

    def _init_mqtt(self):
        self.mqtt = mqtt_client.Client(client_id=f"guibiao_sim_{self.station_id}")
        broker = self.mqtt_broker.replace('mqtt://', '').replace('mqtts://', '')
        parts = broker.split(':')
        host = parts[0]
        port = int(parts[1]) if len(parts) > 1 else 1883
        try:
            self.mqtt.connect(host, port, 60)
            self.mqtt.loop_start()
            print(f"[MQTT] 已连接 {host}:{port}, topic={self.mqtt_topic}")
        except Exception as e:
            print(f"[MQTT] 连接失败: {e}, 回退HTTP-only")
            self.mqtt = None

    def _generate_atmosphere(self, dt):
        hour_frac = dt.hour + dt.minute / 60.0
        t_base = self.atm.get('temperature_base', 5.0)
        t_amp = self.atm.get('temperature_amplitude', 10.0)
        t_noise = self.atm.get('temperature_noise_std', 0.5)
        temp = t_base + t_amp * math.sin(2 * math.pi * (hour_frac / 24.0 - 0.25)) + random.gauss(0, t_noise)

        p_base = self.atm.get('pressure_base', 1013.25)
        p_noise = self.atm.get('pressure_noise_std', 2.0)
        pressure = p_base + random.gauss(0, p_noise)

        h_base = self.atm.get('humidity_base', 50.0)
        h_amp = self.atm.get('humidity_amplitude', 20.0)
        h_noise = self.atm.get('humidity_noise_std', 3.0)
        humidity = h_base + h_amp * math.sin(2 * math.pi * (hour_frac / 24.0)) + random.gauss(0, h_noise)
        humidity = max(0, min(100, humidity))

        return temp, pressure, humidity

    def simulate(self, dt, inject_error=True):
        temp, pressure, humidity = self._generate_atmosphere(dt)

        if self.alt_override is not None:
            alt_list = self.alt_override
            if isinstance(alt_list, list):
                true_alt = random.uniform(alt_list[0], alt_list[1])
            else:
                true_alt = float(alt_list)
            azimuth = 180.0 + random.gauss(0, 5)
        else:
            true_alt = solar_altitude(dt, self.lat, self.lon)
            azimuth = solar_azimuth(dt, self.lat, self.lon)

        refr = atmospheric_refraction(true_alt, temp, pressure)
        apparent_alt = true_alt + refr
        n = refractive_index(temp, pressure, humidity)

        gh = self.gauge_height
        if inject_error:
            gh_noise = self.err.get('gauge_height_noise_std', 0.005)
            gh += random.gauss(0, gh_noise)

        measured_shadow = shadow_length(gh, apparent_alt)
        if inject_error:
            s_noise_ratio = self.err.get('shadow_length_noise_std_ratio', 0.005)
            measured_shadow += random.gauss(0, s_noise_ratio * self.gauge_height)

        is_solstice = 1 if (dt.month == 12 and abs(dt.day - 22) <= 1 and abs(dt.hour - 12) <= 2) else 0

        return {
            "station_id": self.station_id,
            "station_name": self.station_name,
            "measurement_time": dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ"),
            "gauge_height": round(gh, 4),
            "shadow_length": round(measured_shadow, 4),
            "sun_altitude": round(apparent_alt, 6),
            "sun_azimuth": round(azimuth, 4),
            "atmospheric_refraction": round(n, 8),
            "temperature": round(temp, 2),
            "pressure": round(pressure, 2),
            "humidity": round(humidity, 1),
            "is_solstice": is_solstice,
        }

    def publish(self, data):
        if self.mqtt:
            try:
                payload = json.dumps(data, ensure_ascii=False)
                self.mqtt.publish(self.mqtt_topic, payload, qos=1)
                print(f"[MQTT] → {self.mqtt_topic} ({len(payload)}B)")
                return True
            except Exception as e:
                print(f"[MQTT] 发布失败: {e}")

        if requests:
            try:
                resp = requests.post(f"{self.http_url}/api/measurements", json=data, timeout=10)
                if resp.status_code == 200:
                    result = resp.json()
                    if result.get('success'):
                        sim = result.get('data', {})
                        print(f"[HTTP] OK 影长={data['shadow_length']:.2f}尺 "
                              f'折射修正={sim.get("atmospheric_refraction_correction", 0):.2f}"')
                        return True
                print(f"[HTTP] {resp.status_code}: {resp.text[:80]}")
            except Exception as e:
                print(f"[HTTP] 连接失败: {e}")

        return False

    def run(self, start_time=None):
        print(f"圭表传感器模拟器 v2.0")
        print(f"  站点: {self.station_name} ({self.lat}N, {self.lon}E)")
        print(f"  模式: {self.mode}" + (f" 高度角覆盖={self.alt_override}" if self.alt_override else ""))
        print(f"  大气: T={self.atm.get('temperature_base','?')}°C±{self.atm.get('temperature_amplitude','?')} "
              f"P={self.atm.get('pressure_base','?')}hPa H={self.atm.get('humidity_base','?')}%")
        print(f"  输出: MQTT={'ON' if self.mqtt else 'OFF'} HTTP={self.http_url}")
        print("=" * 70)

        current = start_time or datetime.now(CST)
        success = 0
        fail = 0
        total = 0

        try:
            while self.count == 0 or total < self.count:
                data = self.simulate(current)
                if self.publish(data):
                    success += 1
                else:
                    fail += 1
                total += 1
                if total % 20 == 0:
                    print(f"  [统计] 总={total} 成功={success} 失败={fail}")
                time.sleep(self.interval)
                current = current + timedelta(minutes=1)
        except KeyboardInterrupt:
            print(f"\n用户中断，总计={total} 成功={success} 失败={fail}")

        if self.mqtt:
            self.mqtt.loop_stop()
            self.mqtt.disconnect()


def main():
    parser = argparse.ArgumentParser(description="圭表传感器模拟器 v2.0")
    parser.add_argument("--config", default="config.yaml", help="配置文件路径")
    parser.add_argument("--url", help="覆盖后端API地址")
    parser.add_argument("--interval", type=int, help="覆盖上报间隔(秒)")
    parser.add_argument("--start-date", help="起始日期 YYYY-MM-DD HH:MM")
    parser.add_argument("--altitude", type=float, help="覆盖太阳高度角(度), 0=实时计算")
    parser.add_argument("--altitude-range", help="高度角范围 如 5:45")
    parser.add_argument("--temperature", type=float, help="覆盖基准气温")
    parser.add_argument("--pressure", type=float, help="覆盖基准气压")
    parser.add_argument("--humidity", type=float, help="覆盖基准湿度")
    parser.add_argument("--mqtt", action="store_true", help="启用MQTT输出")
    args = parser.parse_args()

    sim = SensorSimulator(args.config)

    if args.url:
        sim.http_url = args.url
    if args.interval:
        sim.interval = args.interval
    if args.altitude is not None and args.altitude > 0:
        sim.alt_override = args.altitude
        sim.mode = "fixed_altitude"
    if args.altitude_range:
        parts = args.altitude_range.split(':')
        if len(parts) == 2:
            sim.alt_override = [float(parts[0]), float(parts[1])]
            sim.mode = "altitude_sweep"
    if args.temperature is not None:
        sim.atm['temperature_base'] = args.temperature
        sim.atm['temperature_amplitude'] = 0
    if args.pressure is not None:
        sim.atm['pressure_base'] = args.pressure
        sim.atm['pressure_noise_std'] = 0
    if args.humidity is not None:
        sim.atm['humidity_base'] = args.humidity
        sim.atm['humidity_amplitude'] = 0
    if args.mqtt:
        sim.mqtt_enabled = True
        if not sim.mqtt and mqtt_client:
            sim._init_mqtt()

    start_time = None
    if args.start_date:
        try:
            start_time = datetime.strptime(args.start_date, "%Y-%m-%d %H:%M").replace(tzinfo=CST)
        except ValueError:
            print("日期格式错误")
            sys.exit(1)

    sim.run(start_time)


if __name__ == "__main__":
    main()
