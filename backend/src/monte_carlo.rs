use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use rand_distr::{Normal, Distribution};
use crate::models::{MonteCarloConfig, MonteCarloResult, SensorMeasurement};
use crate::optics::OpticalSimulator;
use uuid::Uuid;

const CHI_TO_CUN: f64 = 10.0;
const ARCSEC_TO_DEG: f64 = 1.0 / 3600.0;

pub struct MonteCarloAnalyzer {
    simulator: OpticalSimulator,
}

impl MonteCarloAnalyzer {
    pub fn new(simulator: OpticalSimulator) -> Self {
        Self { simulator }
    }

    pub fn analyze(
        &self,
        measurement: &SensorMeasurement,
        config: &MonteCarloConfig,
    ) -> MonteCarloResult {
        let mut rng = rand::thread_rng();
        let gauge_height_dist = Normal::new(0.0, config.gauge_height_error_std).unwrap();
        let refraction_dist = Normal::new(0.0, config.refraction_error_std).unwrap();

        let mut shadow_lengths: Vec<f64> = Vec::with_capacity(config.simulation_count as usize);
        let mut solstice_offsets: Vec<f64> = Vec::with_capacity(config.simulation_count as usize);
        let mut gauge_errors: Vec<f64> = Vec::with_capacity(config.simulation_count as usize);
        let mut refraction_errors: Vec<f64> = Vec::with_capacity(config.simulation_count as usize);

        let nominal_solstice = self.simulator.find_winter_solstice(
            measurement.measurement_time.year(),
        );

        for _ in 0..config.simulation_count {
            let gauge_error = gauge_height_dist.sample(&mut rng);
            let refraction_error = refraction_dist.sample(&mut rng);

            gauge_errors.push(gauge_error);
            refraction_errors.push(refraction_error);

            let perturbed_gauge = measurement.gauge_height + gauge_error;
            let perturbed_altitude = measurement.sun_altitude + refraction_error * ARCSEC_TO_DEG;

            let shadow = self.simulator.shadow_length_from_altitude(
                perturbed_gauge,
                perturbed_altitude.max(0.01),
            );
            shadow_lengths.push(shadow);

            if perturbed_altitude < 35.0 {
                let perturbed_solstice = self.perturb_solstice_search(
                    perturbed_gauge,
                    refraction_error,
                    measurement.measurement_time.year(),
                );
                let offset = (perturbed_solstice - nominal_solstice).num_milliseconds() as f64 / 1000.0;
                solstice_offsets.push(offset);
            }
        }

        let shadow_stats = Self::compute_stats(&shadow_lengths);
        let solstice_stats = if !solstice_offsets.is_empty() {
            Self::compute_stats(&solstice_offsets)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };

        let gauge_stats = Self::compute_stats(&gauge_errors);
        let refraction_stats = Self::compute_stats(&refraction_errors);

        let z_score = Self::z_score_for_confidence(config.confidence_level);
        let combined_uncertainty = (
            (shadow_stats.1 / measurement.gauge_height).powi(2) +
            (refraction_stats.1 * ARCSEC_TO_DEG / measurement.sun_altitude.max(0.1)).powi(2)
        ).sqrt();

        MonteCarloResult {
            id: Uuid::new_v4(),
            station_id: measurement.station_id.clone(),
            analysis_time: Utc::now(),
            reference_time: measurement.measurement_time,
            simulation_count: config.simulation_count,
            gauge_height_error_mean: gauge_stats.0,
            gauge_height_error_std: gauge_stats.1,
            refraction_error_mean: refraction_stats.0,
            refraction_error_std: refraction_stats.1,
            shadow_length_mean: shadow_stats.0,
            shadow_length_std: shadow_stats.1,
            shadow_length_95ci_low: shadow_stats.2,
            shadow_length_95ci_high: shadow_stats.3,
            solstice_time_mean: solstice_stats.0,
            solstice_time_std: solstice_stats.1,
            solstice_time_95ci_low: solstice_stats.2,
            solstice_time_95ci_high: solstice_stats.3,
            combined_uncertainty,
            expanded_uncertainty: combined_uncertainty * 2.0,
        }
    }

    fn perturb_solstice_search(
        &self,
        gauge_height: f64,
        refraction_bias: f64,
        year: i32,
    ) -> DateTime<Utc> {
        let dec_21 = Utc.with_ymd_and_hms(year, 12, 21, 12, 0, 0).unwrap();
        let mut best_time = dec_21;
        let mut min_alt = f64::MAX;

        let mut hour = -24;
        while hour <= 24 {
            let candidate = dec_21 + Duration::hours(hour);
            let base_alt = self.simulator.solar_altitude(candidate);
            let alt = base_alt + refraction_bias * ARCSEC_TO_DEG;
            let shadow = self.simulator.shadow_length_from_altitude(gauge_height, alt.max(0.01));
            if shadow < min_alt {
                min_alt = shadow;
                best_time = candidate;
            }
            hour += 1;
        }

        let mut minute = -60.0;
        while minute <= 60.0 {
            let candidate = best_time + Duration::minutes(minute as i64);
            let base_alt = self.simulator.solar_altitude(candidate);
            let alt = base_alt + refraction_bias * ARCSEC_TO_DEG;
            let shadow = self.simulator.shadow_length_from_altitude(gauge_height, alt.max(0.01));
            if shadow < min_alt {
                min_alt = shadow;
                best_time = candidate;
            }
            minute += 0.1;
        }

        best_time
    }

    fn compute_stats(values: &[f64]) -> (f64, f64, f64, f64) {
        if values.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }

        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / values.len() as f64;
        let std_dev = variance.sqrt();

        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx_low = (sorted.len() as f64 * 0.025).floor() as usize;
        let idx_high = (sorted.len() as f64 * 0.975).floor() as usize;
        let idx_low = idx_low.min(sorted.len() - 1);
        let idx_high = idx_high.min(sorted.len() - 1);

        (mean, std_dev, sorted[idx_low], sorted[idx_high])
    }

    fn z_score_for_confidence(level: f64) -> f64 {
        match level {
            0.90 => 1.645,
            0.95 => 1.96,
            0.99 => 2.576,
            0.997 => 3.0,
            _ => 1.96,
        }
    }

    pub fn uncertainty_budget(
        &self,
        measurement: &SensorMeasurement,
        config: &MonteCarloConfig,
    ) -> Vec<(String, f64, f64)> {
        let mut budget = Vec::new();

        let u_gauge = config.gauge_height_error_std / measurement.gauge_height;
        budget.push(("表高不确定度".to_string(), u_gauge, u_gauge.powi(2)));

        let u_refraction = config.refraction_error_std * ARCSEC_TO_DEG
            / measurement.sun_altitude.max(0.1);
        budget.push(("蒙气差不确定度".to_string(), u_refraction, u_refraction.powi(2)));

        let u_temperature = 0.001;
        budget.push(("温度影响".to_string(), u_temperature, u_temperature.powi(2)));

        let u_reading = 0.0005;
        budget.push(("读数误差".to_string(), u_reading, u_reading.powi(2)));

        budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MonteCarloConfig;
    use chrono::TimeZone;

    #[test]
    fn test_compute_stats() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let (mean, std, low, high) = MonteCarloAnalyzer::compute_stats(&values);
        assert!((mean - 3.0).abs() < 0.01);
        assert!(std > 1.0);
        assert!(low <= 2.0);
        assert!(high >= 4.0);
    }

    #[test]
    fn test_monte_carlo_analysis() {
        let sim = OpticalSimulator::new(34.49, 113.09, 420.0);
        let analyzer = MonteCarloAnalyzer::new(sim);
        let measurement = SensorMeasurement {
            id: Uuid::new_v4(),
            station_id: "test".to_string(),
            station_name: "Test".to_string(),
            measurement_time: Utc.with_ymd_and_hms(2023, 12, 22, 4, 30, 0).unwrap(),
            gauge_height: 40.0,
            shadow_length: 88.0,
            sun_altitude: 24.5,
            sun_azimuth: 180.0,
            atmospheric_refraction: 1.00029,
            temperature: 5.0,
            pressure: 1013.25,
            humidity: 50.0,
            is_solstice: 1,
        };
        let config = MonteCarloConfig {
            simulation_count: 1000,
            ..Default::default()
        };
        let result = analyzer.analyze(&measurement, &config);
        assert_eq!(result.simulation_count, 1000);
        assert!(result.shadow_length_std > 0.0);
        assert!(result.combined_uncertainty > 0.0);
    }
}
