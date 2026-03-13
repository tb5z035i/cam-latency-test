use cam_latency_test::{
    measurement::{modular_latency_delta, CalibrationEstimate, MeasurementStats},
    pattern::STATE_SPACE,
};

#[test]
fn modular_delta_accepts_normal_values() {
    assert_eq!(modular_latency_delta(1_000, 940), Some(60));
}

#[test]
fn modular_delta_handles_wraparound() {
    assert_eq!(modular_latency_delta(15, STATE_SPACE - 10), Some(25));
}

#[test]
fn modular_delta_rejects_implausible_values() {
    assert_eq!(modular_latency_delta(10_000, 50_000), None);
}

#[test]
fn measurement_stats_accumulate_average() {
    let mut stats = MeasurementStats::default();
    let calibration = CalibrationEstimate::default();

    stats.push(1_000, 950, 0.9, calibration);
    stats.push(1_100, 1_000, 0.8, calibration);

    assert_eq!(stats.sample_count, 2);
    assert_eq!(stats.instant.map(|sample| sample.latency_ms), Some(100.0));
    assert_eq!(stats.average_latency_ms, Some(75.0));
    assert!(stats.corrected_latency_ms.is_some());
}
