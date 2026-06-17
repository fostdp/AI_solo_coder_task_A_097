use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use crate::models::OpticalSimulationResult;
use uuid::Uuid;

const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;
const RAD_TO_DEG: f64 = 180.0 / std::f64::consts::PI;
const ARCSEC_TO_DEG: f64 = 1.0 / 3600.0;
const EARTH_RADIUS_KM: f64 = 6371.0;
const STANDARD_ATMOSPHERE: f64 = 1013.25;
const STANDARD_TEMP_K: f64 = 288.15;
const CHI_TO_CUN: f64 = 10.0;

pub struct OpticalSimulator {
    pub station_latitude: f64,
    pub station_longitude: f64,
    pub station_altitude: f64,
}

impl OpticalSimulator {
    pub fn new(lat: f64, lon: f64, alt: f64) -> Self {
        Self {
            station_latitude: lat,
            station_longitude: lon,
            station_altitude: alt,
        }
    }

    pub fn calculate_atmospheric_refraction(
        &self,
        apparent_altitude_deg: f64,
        temperature_c: f64,
        pressure_hpa: f64,
    ) -> f64 {
        if apparent_altitude_deg <= -1.0 {
            return 0.0;
        }
        let h = apparent_altitude_deg.max(0.5);
        let t = temperature_c + 273.15;
        let p = pressure_hpa;
        let base_refraction = 1.02 / (h + 10.3 / (h + 5.11)).to_radians().tan();
        let temp_correction = (p * STANDARD_TEMP_K) / (STANDARD_ATMOSPHERE * t);
        let refraction_deg = base_refraction * temp_correction * RAD_TO_DEG;
        refraction_deg
    }

    pub fn calculate_refraction_arcsec(
        &self,
        apparent_altitude_deg: f64,
        temperature_c: f64,
        pressure_hpa: f64,
    ) -> f64 {
        self.calculate_atmospheric_refraction(
            apparent_altitude_deg,
            temperature_c,
            pressure_hpa,
        ) * 3600.0
    }

    pub fn bennett_refraction(
        &self,
        apparent_altitude_deg: f64,
        temperature_c: f64,
        pressure_hpa: f64,
    ) -> f64 {
        if apparent_altitude_deg <= -0.5 {
            return 0.0;
        }
        let h = apparent_altitude_deg;
        let t = temperature_c;
        let p = pressure_hpa;
        let z = 90.0 - h;
        let tan_term = (z + 7.31 / (z + 4.4)).to_radians().tan();
        let r_arcmin = (1.0 / tan_term) * (p / STANDARD_ATMOSPHERE) * (STANDARD_TEMP_K / (t + 273.15));
        r_arcmin * 60.0
    }

    pub fn correct_for_refraction(
        &self,
        true_altitude_deg: f64,
        temperature_c: f64,
        pressure_hpa: f64,
    ) -> f64 {
        let mut apparent = true_altitude_deg;
        for _ in 0..5 {
            let refr = self.calculate_atmospheric_refraction(apparent, temperature_c, pressure_hpa);
            apparent = true_altitude_deg + refr;
        }
        apparent
    }

    pub fn earth_curvature_correction(&self, shadow_length_chi: f64) -> f64 {
        let s_km = shadow_length_chi * 0.3333 / 1000.0;
        let delta_h_km = EARTH_RADIUS_KM - (EARTH_RADIUS_KM.powi(2) - s_km.powi(2)).sqrt();
        delta_h_km * 1000.0 / 0.3333
    }

    pub fn shadow_length_from_altitude(
        &self,
        gauge_height_chi: f64,
        altitude_deg: f64,
    ) -> f64 {
        if altitude_deg <= 0.001 {
            return f64::MAX;
        }
        gauge_height_chi / altitude_deg.to_radians().tan()
    }

    pub fn altitude_from_shadow_length(
        &self,
        gauge_height_chi: f64,
        shadow_length_chi: f64,
    ) -> f64 {
        if shadow_length_chi <= 0.0 {
            return 90.0;
        }
        (gauge_height_chi / shadow_length_chi).atan().to_degrees()
    }

    pub fn sun_declination(&self, date: DateTime<Utc>) -> f64 {
        let day_of_year = date.ordinal() as f64;
        let gamma = 2.0 * std::f64::consts::PI * (day_of_year - 1.0) / 365.0;
        23.45 * (gamma + 0.0733 - 0.0068).sin()
    }

    pub fn equation_of_time(&self, date: DateTime<Utc>) -> f64 {
        let day_of_year = date.ordinal() as f64;
        let b = 2.0 * std::f64::consts::PI * (day_of_year - 81.0) / 365.0;
        9.87 * (2.0 * b).sin() - 7.53 * b.cos() - 1.5 * b.sin()
    }

    pub fn solar_altitude(&self, date: DateTime<Utc>) -> f64 {
        let decl = self.sun_declination(date);
        let lat_rad = self.station_latitude.to_radians();
        let decl_rad = decl.to_radians();
        let hour = self.solar_hour_angle(date);
        let hour_rad = hour.to_radians();
        let sin_alt = lat_rad.sin() * decl_rad.sin()
            + lat_rad.cos() * decl_rad.cos() * hour_rad.cos();
        sin_alt.clamp(-1.0, 1.0).asin().to_degrees()
    }

    pub fn solar_azimuth(&self, date: DateTime<Utc>) -> f64 {
        let alt = self.solar_altitude(date);
        let alt_rad = alt.to_radians();
        let decl = self.sun_declination(date);
        let decl_rad = decl.to_radians();
        let lat_rad = self.station_latitude.to_radians();
        let cos_azi = (decl_rad.sin() - alt_rad.sin() * lat_rad.sin())
            / (alt_rad.cos() * lat_rad.cos());
        let cos_azi = cos_azi.clamp(-1.0, 1.0);
        let hour = self.solar_hour_angle(date);
        let azi = cos_azi.acos().to_degrees();
        if hour > 0.0 {
            360.0 - azi
        } else {
            azi
        }
    }

    fn solar_hour_angle(&self, date: DateTime<Utc>) -> f64 {
        let eot = self.equation_of_time(date);
        let lstm = 15.0 * (self.station_longitude / 15.0).round();
        let tc = 4.0 * (self.station_longitude - lstm) + eot;
        let lst = date.hour() as f64 + date.minute() as f64 / 60.0
            + date.second() as f64 / 3600.0 + tc / 60.0;
        15.0 * (lst - 12.0)
    }

    pub fn find_winter_solstice(&self, year: i32) -> DateTime<Utc> {
        let dec_21 = Utc.with_ymd_and_hms(year, 12, 21, 12, 0, 0).unwrap();
        let mut best_time = dec_21;
        let mut min_alt = self.solar_altitude(dec_21);
        let mut hour = -24;
        while hour <= 24 {
            let candidate = dec_21 + Duration::hours(hour);
            let alt = self.solar_altitude(candidate);
            if alt < min_alt {
                min_alt = alt;
                best_time = candidate;
            }
            hour += 1;
        }
        let mut minute = -60.0;
        while minute <= 60.0 {
            let candidate = best_time + Duration::minutes(minute as i64);
            let alt = self.solar_altitude(candidate);
            if alt < min_alt {
                min_alt = alt;
                best_time = candidate;
            }
            minute += 0.1;
        }
        best_time
    }

    pub fn simulate_optics(
        &self,
        measurement_id: Uuid,
        station_id: &str,
        gauge_height: f64,
        measured_shadow: f64,
        measured_altitude: f64,
        temperature: f64,
        pressure: f64,
        measurement_time: DateTime<Utc>,
    ) -> OpticalSimulationResult {
        let refraction_correction = self.calculate_refraction_arcsec(
            measured_altitude,
            temperature,
            pressure,
        );
        let true_altitude = measured_altitude - refraction_correction * ARCSEC_TO_DEG;
        let apparent_altitude = self.correct_for_refraction(
            true_altitude,
            temperature,
            pressure,
        );
        let theoretical_shadow = self.shadow_length_from_altitude(
            gauge_height,
            true_altitude,
        );
        let refracted_shadow = self.shadow_length_from_altitude(
            gauge_height,
            apparent_altitude,
        );
        let curvature_corr = self.earth_curvature_correction(theoretical_shadow);
        let shadow_deviation = (measured_shadow - refracted_shadow) * CHI_TO_CUN;
        let solstice = if measured_altitude < 35.0 {
            Some(self.find_winter_solstice(measurement_time.year()))
        } else {
            None
        };
        OpticalSimulationResult {
            id: Uuid::new_v4(),
            measurement_id,
            station_id: station_id.to_string(),
            simulation_time: Utc::now(),
            true_sun_altitude: true_altitude,
            apparent_sun_altitude: apparent_altitude,
            atmospheric_refraction_correction: refraction_correction,
            earth_curvature_correction: curvature_corr,
            theoretical_shadow_length: theoretical_shadow,
            refracted_shadow_length: refracted_shadow,
            shadow_deviation,
            winter_solstice_moment: solstice,
            solstice_uncertainty: 120.0,
        }
    }

    pub fn atmospheric_refractive_index(&self, temperature_c: f64, pressure_hpa: f64, humidity_pct: f64) -> f64 {
        let t = temperature_c + 273.15;
        let p = pressure_hpa;
        let es = 6.112 * ((17.67 * temperature_c) / (temperature_c + 243.5)).exp();
        let e = es * humidity_pct / 100.0;
        let n_dry = 1.0 + 77.624e-6 * p / t;
        let n_wet = 1.0 - 12.92e-6 * e / t;
        n_dry - 1.0 + n_wet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_length_calculation() {
        let sim = OpticalSimulator::new(34.49, 113.09, 420.0);
        let shadow = sim.shadow_length_from_altitude(40.0, 45.0);
        assert!((shadow - 40.0).abs() < 0.01);
    }

    #[test]
    fn test_atmospheric_refraction() {
        let sim = OpticalSimulator::new(34.49, 113.09, 420.0);
        let refr = sim.calculate_atmospheric_refraction(30.0, 15.0, 1013.25);
        assert!(refr > 0.000001);
        assert!(refr < 0.1);
    }

    #[test]
    fn test_sun_declination_winter() {
        let sim = OpticalSimulator::new(34.49, 113.09, 420.0);
        let dec = sim.sun_declination(
            Utc.with_ymd_and_hms(2023, 12, 22, 0, 0, 0).unwrap(),
        );
        assert!(dec < -23.0);
    }
}
