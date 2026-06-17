#!/usr/bin/env python3
"""
古代圭表测影光学仿真系统 - 传感器模拟器
模拟登封观星台元代圭表，每分钟上报一次测量数据
"""

import json
import time
import math
import random
import requests
import argparse
from datetime import datetime, timezone, timedelta

DENGFENG_LAT = 34.4897
DENGFENG_LON = 113.0875
DENGFENG_ALT = 420.0
STANDARD_GAUGE_HEIGHT = 40.0
STATION_ID = "dengfeng_001"
STATION_NAME = "登封观星台元代圭表"

DEG_TO_RAD = math.pi / 180.0
RAD_TO_DEG = 180.0 / math.pi
CST = timezone(timedelta(hours=8))


def day_of_year(dt: datetime) -> int:
    return dt.timetuple().tm_yday


def sun_declination(dt: datetime) -> float:
    doy = day_of_year(dt)
    gamma = 2.0 * math.pi * (doy - 1) / 365.0
    return 23.45 * math.sin(gamma + 0.0733 - 0.0068)


def equation_of_time(dt: datetime) -> float:
    doy = day_of_year(dt)
    b = 2.0 * math.pi * (doy - 81.0) / 365.0
    return 9.87 * math.sin(2 * b) - 7.53 * math.cos(b) - 1.5 * math.sin(b)


def solar_hour_angle(dt: datetime, lon: float) -> float:
    eot = equation_of_time(dt)
    lstm = 15.0 * round(lon / 15.0)
    tc = 4.0 * (lon - lstm) + eot
    utc_dt = dt.astimezone(timezone.utc)
    lst = (utc_dt.hour + 8) + utc_dt.minute / 60.0 + utc_dt.second / 3600.0 + tc / 60.0
    return 15.0 * (lst - 12.0)


def solar_altitude(dt: datetime, lat: float, lon: float) -> float:
    decl = sun_declination(dt)
    lat_rad = lat * DEG_TO_RAD
    decl_rad = decl * DEG_TO_RAD
    hour = solar_hour_angle(dt, lon)
    hour_rad = hour * DEG_TO_RAD
    sin_alt = (math.sin(lat_rad) * math.sin(decl_rad) +
               math.cos(lat_rad) * math.cos(decl_rad) * math.cos(hour_rad))
    sin_alt = max(-1.0, min(1.0, sin_alt))
    return math.asin(sin_alt) * RAD_TO_DEG


def solar_azimuth(dt: datetime, lat: float, lon: float) -> float:
    alt = solar_altitude(dt, lat, lon)
    alt_rad = alt * DEG_TO_RAD
    decl = sun_declination(dt)
    decl_rad = decl * DEG_TO_RAD
    lat_rad = lat * DEG_TO_RAD
    cos_azi = ((math.sin(decl_rad) - math.sin(alt_rad) * math.sin(lat_rad)) /
               (math.cos(alt_rad) * math.cos(lat_rad)))
    cos_azi = max(-1.0, min(1.0, cos_azi))
    hour = solar_hour_angle(dt, lon)
    azi = math.acos(cos_azi) * RAD_TO_DEG
    return 360.0 - azi if hour > 0 else azi


def atmospheric_refraction(apparent_alt: float, temp_c: float, pressure_hpa: float) -> float:
    if apparent_alt <= -1.0:
        return 0.0
    h = max(apparent_alt, 0.5)
    t = temp_c + 273.15
    p = pressure_hpa
    base_refraction = 1.02 / math.tan((h + 10.3 / (h + 5.11)) * DEG_TO_RAD)
    temp_correction = (p * 288.15) / (1013.25 * t)
    return base_refraction * temp_correction * RAD_TO_DEG


def refractive_index(temp_c: float, pressure_hpa: float, humidity_pct: float) -> float:
    t = temp_c + 273.15
    p = pressure_hpa
    es = 6.112 * math.exp((17.67 * temp_c) / (temp_c + 243.5))
    e = es * humidity_pct / 100.0
    n_dry = 1.0 + 77.624e-6 * p / t
    n_wet = 1.0 - 12.92e-6 * e / t
    return n_dry - 1.0 + n_wet


def shadow_length(gauge_height: float, altitude_deg: float) -> float:
    if altitude_deg <= 0.001:
        return float('inf')
    return gauge_height / math.tan(altitude_deg * DEG_TO_RAD)


def simulate_measurement(dt: datetime, inject_error: bool = True) -> dict:
    true_alt = solar_altitude(dt, DENGFENG_LAT, DENGFENG_LON)
    azimuth = solar_azimuth(dt, DENGFENG_LAT, DENGFENG_LON)
    temp = 5.0 + 10.0 * math.sin(2 * math.pi * (dt.hour / 24.0 - 0.25)) + random.gauss(0, 0.5)
    pressure = 1013.25 + random.gauss(0, 2.0)
    humidity = 50.0 + 20.0 * math.sin(2 * math.pi * (dt.hour / 24.0)) + random.gauss(0, 3.0)
    humidity = max(0, min(100, humidity))
    refr = atmospheric_refraction(true_alt, temp, pressure)
    apparent_alt = true_alt + refr
    n = refractive_index(temp, pressure, humidity)
    gauge_height = STANDARD_GAUGE_HEIGHT
    if inject_error:
        gauge_height += random.gauss(0, 0.005)
    theor_shadow = shadow_length(gauge_height, true_alt)
    measured_shadow = shadow_length(gauge_height, apparent_alt)
    if inject_error:
        measured_shadow += random.gauss(0, 0.005 * STANDARD_GAUGE_HEIGHT)
    is_solstice = 1 if (dt.month == 12 and abs(dt.day - 22) <= 1 and abs(dt.hour - 12) <= 2) else 0
    return {
        "station_id": STATION_ID,
        "station_name": STATION_NAME,
        "measurement_time": dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ"),
        "gauge_height": round(gauge_height, 4),
        "shadow_length": round(measured_shadow, 4),
        "sun_altitude": round(apparent_alt, 6),
        "sun_azimuth": round(azimuth, 4),
        "atmospheric_refraction": round(n, 8),
        "temperature": round(temp, 2),
        "pressure": round(pressure, 2),
        "humidity": round(humidity, 1),
        "is_solstice": is_solstice,
    }


def post_measurement(url: str, data: dict) -> bool:
    try:
        resp = requests.post(f"{url}/api/measurements", json=data, timeout=10)
        if resp.status_code == 200:
            result = resp.json()
            if result.get("success"):
                sim_data = result.get("data", {})
                print(f"[OK] {data['measurement_time']} 表高={data['gauge_height']:.2f}尺 "
                      f"影长={data['shadow_length']:.2f}尺 太阳高度={data['sun_altitude']:.2f}° "
                      f"折射修正={sim_data.get('atmospheric_refraction_correction', 0):.2f}\"")
                return True
        print(f"[WARN] HTTP {resp.status_code}: {resp.text[:100]}")
    except requests.exceptions.RequestException as e:
        print(f"[ERROR] 连接失败: {e}")
    return False


def run_simulation(api_url: str, interval_sec: int, start_time: datetime = None):
    print(f"传感器模拟器启动 - 上报地址: {api_url}")
    print(f"站点: {STATION_NAME} (纬度{DENGFENG_LAT}, 经度{DENGFENG_LON})")
    print(f"标准表高: {STANDARD_GAUGE_HEIGHT}尺, 上报间隔: {interval_sec}秒")
    print("=" * 70)
    current_time = start_time or datetime.now(CST)
    error_count = 0
    success_count = 0
    while True:
        try:
            data = simulate_measurement(current_time)
            if post_measurement(api_url, data):
                success_count += 1
            else:
                error_count += 1
            print(f"    [统计] 成功={success_count}, 失败={error_count}")
            time.sleep(interval_sec)
            current_time = current_time + timedelta(minutes=1)
        except KeyboardInterrupt:
            print("\n用户中断，退出模拟器")
            break
        except Exception as e:
            print(f"[异常] {e}")
            time.sleep(interval_sec)


def main():
    parser = argparse.ArgumentParser(description="圭表传感器模拟器")
    parser.add_argument("--url", default="http://localhost:3000", help="后端API地址")
    parser.add_argument("--interval", type=int, default=3, help="上报间隔(秒)，模拟每分钟")
    parser.add_argument("--start-date", help="起始日期 YYYY-MM-DD HH:MM")
    parser.add_argument("--count", type=int, default=0, help="上报次数(0=无限)")
    args = parser.parse_args()
    start_time = None
    if args.start_date:
        try:
            start_time = datetime.strptime(args.start_date, "%Y-%m-%d %H:%M").replace(tzinfo=CST)
        except ValueError:
            print("日期格式错误，使用当前时间")
    if args.count > 0:
        print(f"将上报 {args.count} 次数据")
        current = start_time or datetime.now(CST)
        for i in range(args.count):
            data = simulate_measurement(current)
            post_measurement(args.url, data)
            current = current + timedelta(minutes=1)
            time.sleep(args.interval)
        print(f"完成 {args.count} 次上报")
    else:
        run_simulation(args.url, args.interval, start_time)


if __name__ == "__main__":
    main()
