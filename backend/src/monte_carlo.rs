use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use rand_distr::{Normal, Distribution};
use crate::models::{MonteCarloConfig, MonteCarloResult, SensorMeasurement};
use crate::optics::OpticalSimulator;
use uuid::Uuid;

const CHI_TO_CUN: f64 = 10.0;
const ARCSEC_TO_DEG: f64 = 1.0 / 3600.0;
const BOOTSTRAP_RESAMPLES: usize = 2000;
const JACKKNIFE_MIN: usize = 10;

pub struct MonteCarloAnalyzer {
    simulator: OpticalSimulator,
}

struct BootstrapStats {
    mean: f64,
    std: f64,
    ci_low: f64,
    ci_high: f64,
    bca_low: f64,
    bca_high: f64,
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

        let n = config.simulation_count as usize;
        let mut shadow_lengths: Vec<f64> = Vec::with_capacity(n);
        let mut solstice_offsets: Vec<f64> = Vec::with_capacity(n);
        let mut gauge_errors: Vec<f64> = Vec::with_capacity(n);
        let mut refraction_errors: Vec<f64> = Vec::with_capacity(n);

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

        let alpha = 1.0 - config.confidence_level;
        let shadow_bs = Self::bootstrap_bca(&shadow_lengths, alpha, BOOTSTRAP_RESAMPLES);
        let solstice_bs = if !solstice_offsets.is_empty() {
            Self::bootstrap_bca(&solstice_offsets, alpha, BOOTSTRAP_RESAMPLES)
        } else {
            BootstrapStats { mean: 0.0, std: 0.0, ci_low: 0.0, ci_high: 0.0, bca_low: 0.0, bca_high: 0.0 }
        };

        let gauge_stats = Self::naive_stats(&gauge_errors);
        let refraction_stats = Self::naive_stats(&refraction_errors);

        let std_ratio_shadow = shadow_bs.std / gauge_stats.1.max(1e-9);
        let bias_correction = if std_ratio_shadow > 0.0 && shadow_lengths.len() >= 500 {
            1.0 + 0.5 / (shadow_lengths.len() as f64).sqrt()
        } else {
            1.0
        };

        let combined_uncertainty = (
            (shadow_bs.std / measurement.gauge_height).powi(2) +
            (refraction_stats.1 * ARCSEC_TO_DEG / measurement.sun_altitude.max(0.1)).powi(2)
        ).sqrt() * bias_correction;

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
            shadow_length_mean: shadow_bs.mean,
            shadow_length_std: shadow_bs.std,
            shadow_length_95ci_low: shadow_bs.bca_low,
            shadow_length_95ci_high: shadow_bs.bca_high,
            solstice_time_mean: solstice_bs.mean,
            solstice_time_std: solstice_bs.std,
            solstice_time_95ci_low: solstice_bs.bca_low,
            solstice_time_95ci_high: solstice_bs.bca_high,
            combined_uncertainty,
            expanded_uncertainty: combined_uncertainty * 2.0,
        }
    }

    fn bootstrap_bca(
        values: &[f64],
        alpha: f64,
        n_resamples: usize,
    ) -> BootstrapStats {
        let n = values.len();
        if n == 0 {
            return BootstrapStats { mean: 0.0, std: 0.0, ci_low: 0.0, ci_high: 0.0, bca_low: 0.0, bca_high: 0.0 };
        }

        let naive = Self::naive_stats(values);

        if n < 20 || n_resamples < 100 {
            return BootstrapStats {
                mean: naive.0,
                std: naive.1,
                ci_low: naive.2,
                ci_high: naive.3,
                bca_low: naive.2,
                bca_high: naive.3,
            };
        }

        let mut rng = rand::thread_rng();
        let mut resampled_stat: Vec<f64> = Vec::with_capacity(n_resamples);
        let mut resampled_means: Vec<f64> = Vec::with_capacity(n_resamples);

        for _ in 0..n_resamples {
            let mut sum = 0.0;
            for _ in 0..n {
                let idx: usize = rng.gen_range(0..n);
                sum += values[idx];
            }
            let mean = sum / n as f64;
            resampled_stat.push(mean);
            resampled_means.push(mean);
        }

        let resampled_stats_naive = Self::naive_stats(&resampled_means);

        let less_than_theta0 = resampled_stat.iter().filter(|&&x| x < naive.0).count() as f64;
        let z0 = Self::probit(less_than_theta0 / n_resamples as f64);

        let a = if n >= JACKKNIFE_MIN {
            Self::jackknife_acceleration(values)
        } else {
            0.0
        };

        let alpha_lo = alpha / 2.0;
        let alpha_hi = 1.0 - alpha / 2.0;

        let z_lo = Self::probit(alpha_lo);
        let z_hi = Self::probit(alpha_hi);

        let denom_lo = 1.0 - a * (z0 + z_lo);
        let denom_hi = 1.0 - a * (z0 + z_hi);

        let adj_alpha_lo = if denom_lo.abs() > 1e-6 {
            let p = z0 + (z0 + z_lo) / denom_lo;
            Self::normal_cdf(p)
        } else {
            alpha_lo
        };

        let adj_alpha_hi = if denom_hi.abs() > 1e-6 {
            let p = z0 + (z0 + z_hi) / denom_hi;
            Self::normal_cdf(p)
        } else {
            alpha_hi
        };

        let idx_lo = ((adj_alpha_lo * n_resamples as f64).floor() as usize).min(n_resamples - 1);
        let idx_hi = ((adj_alpha_hi * n_resamples as f64).ceil() as usize).min(n_resamples - 1);

        resampled_means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        BootstrapStats {
            mean: resampled_stats_naive.0,
            std: resampled_stats_naive.1,
            ci_low: resampled_stats_naive.2,
            ci_high: resampled_stats_naive.3,
            bca_low: resampled_means[idx_lo],
            bca_high: resampled_means[idx_hi],
        }
    }

    fn probit(p: f64) -> f64 {
        let p = p.clamp(1e-10, 1.0 - 1e-10);
        Self::rational_approximation_probit(p)
    }

    fn rational_approximation_probit(p: f64) -> f64 {
        let a: [f64; 6] = [
            -3.969683028665376e+01,
            2.209460984245205e+02,
            -2.759285104469687e+02,
            1.383577518672690e+02,
            -3.066479806614716e+01,
            2.506628277459239e+00,
        ];
        let b: [f64; 5] = [
            -5.447609879822406e+01,
            1.615858368580409e+02,
            -1.556989798598866e+02,
            6.680131188771972e+01,
            -1.328068155288572e+01,
        ];
        let c: [f64; 6] = [
            -7.784894002430293e-03,
            -3.223964580411365e-01,
            -2.400758277161838e+00,
            -2.549732539343734e+00,
            4.374664141464968e+00,
            2.938163982698783e+00,
        ];
        let d: [f64; 4] = [
            7.784695709041462e-03,
            3.224671290700398e-01,
            2.445134137142996e+00,
            3.754408661907416e+00,
        ];

        let p_low = 0.02425;
        let p_high = 1.0 - p_low;

        if p < p_low {
            let q = (-2.0 * p.ln()).sqrt();
            (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5]) /
                ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
        } else if p <= p_high {
            let q = p - 0.5;
            let r = q * q;
            (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q /
                (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0)
        } else {
            let q = (-2.0 * (1.0 - p).ln()).sqrt();
            -(((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5]) /
                ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
        }
    }

    fn normal_cdf(x: f64) -> f64 {
        0.5 * (1.0 + libm_mimic_erf(x / std::f64::consts::SQRT_2))
    }

    fn jackknife_acceleration(values: &[f64]) -> f64 {
        let n = values.len();
        if n < JACKKNIFE_MIN {
            return 0.0;
        }
        let total_sum: f64 = values.iter().sum();
        let mut pseudo_values: Vec<f64> = Vec::with_capacity(n);
        for i in 0..n {
            let jackknife_mean = (total_sum - values[i]) / (n as f64 - 1.0);
            pseudo_values.push(jackknife_mean);
        }
        let pv_mean = pseudo_values.iter().sum::<f64>() / n as f64;
        let mut num = 0.0;
        let mut den = 0.0;
        for pv in &pseudo_values {
            let diff = pv_mean - pv;
            num += diff.powi(3);
            den += diff.powi(2);
        }
        let den_pow = den.powf(1.5);
        if den_pow < 1e-15 {
            return 0.0;
        }
        num / (6.0 * den_pow)
    }

    fn naive_stats(values: &[f64]) -> (f64, f64, f64, f64) {
        if values.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        let variance = values.iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / n;
        let std_dev = variance.sqrt();
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx_low = (sorted.len() as f64 * 0.025).floor() as usize;
        let idx_high = (sorted.len() as f64 * 0.975).floor() as usize;
        let idx_low = idx_low.min(sorted.len() - 1);
        let idx_high = idx_high.min(sorted.len() - 1);
        (mean, std_dev, sorted[idx_low], sorted[idx_high])
    }

    fn perturb_solstice_search(
        &self,
        gauge_height: f64,
        refraction_bias: f64,
        year: i32,
    ) -> DateTime<Utc> {
        let dec_21 = Utc.with_ymd_and_hms(year, 12, 21, 12, 0, 0).unwrap();
        let mut best_time = dec_21;
        let mut min_shadow = f64::MAX;

        let mut hour = -24;
        while hour <= 24 {
            let candidate = dec_21 + Duration::hours(hour);
            let base_alt = self.simulator.solar_altitude(candidate);
            let alt = base_alt + refraction_bias * ARCSEC_TO_DEG;
            let shadow = self.simulator.shadow_length_from_altitude(gauge_height, alt.max(0.01));
            if shadow < min_shadow {
                min_shadow = shadow;
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
            if shadow < min_shadow {
                min_shadow = shadow;
                best_time = candidate;
            }
            minute += 0.1;
        }

        best_time
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

fn libm_mimic_erf(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    const A1: f64 = 0.254829592;
    const A2: f64 = -0.284496736;
    const A3: f64 = 1.421413741;
    const A4: f64 = -1.453152027;
    const A5: f64 = 1.061405429;
    const P: f64 = 0.3275911;

    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + P * x);
    let y = 1.0 - (((((A5 * t + A4) * t) + A3) * t + A2) * t + A1) * t * (-x * x).exp();
    sign * y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MonteCarloConfig;
    use chrono::TimeZone;

    #[test]
    fn test_naive_stats() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let (mean, std, low, high) = MonteCarloAnalyzer::naive_stats(&values);
        assert!((mean - 3.0).abs() < 0.01);
        assert!(std > 1.0);
        assert!(low <= 2.0);
        assert!(high >= 4.0);
    }

    #[test]
    fn test_bootstrap_bca_basic() {
        let data: Vec<f64> = (0..200).map(|i| 50.0 + (i as f64) * 0.01 + (i as f64 % 7) as f64 * 0.005).collect();
        let bs = MonteCarloAnalyzer::bootstrap_bca(&data, 0.05, 500);
        assert!(bs.bca_low < bs.bca_high);
        assert!((bs.mean - 51.0).abs() < 1.0);
        assert!(bs.std > 0.0);
    }

    #[test]
    fn test_probit_normal_cdf_inverse() {
        let ps = [0.025, 0.05, 0.5, 0.95, 0.975];
        for p in ps {
            let z = MonteCarloAnalyzer::probit(p);
            let cdf = MonteCarloAnalyzer::normal_cdf(z);
            assert!((cdf - p).abs() < 0.001, "probit({})={} cdf back={}", p, z, cdf);
        }
    }

    #[test]
    fn test_jackknife_acceleration_skewed() {
        let skewed: Vec<f64> = (0..50).map(|i| (i as f64).exp() * 0.0001 + 10.0).collect();
        let a = MonteCarloAnalyzer::jackknife_acceleration(&skewed);
        assert!(a.is_finite());
    }

    #[test]
    fn test_monte_carlo_analysis_bootstrap() {
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
            simulation_count: 500,
            ..Default::default()
        };
        let result = analyzer.analyze(&measurement, &config);
        assert_eq!(result.simulation_count, 500);
        assert!(result.shadow_length_std > 0.0);
        assert!(result.shadow_length_95ci_low < result.shadow_length_mean);
        assert!(result.shadow_length_95ci_high > result.shadow_length_mean);
        assert!(result.combined_uncertainty > 0.0);
        assert!(result.expanded_uncertainty > result.combined_uncertainty);
    }
}
