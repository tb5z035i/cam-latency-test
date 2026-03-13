use crate::pattern::{MAX_LATENCY_MS, STATE_SPACE};

#[derive(Clone, Copy, Debug, Default)]
pub struct MeasurementSample {
    pub latency_ms: f32,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct CalibrationEstimate {
    pub assumed_refresh_hz: f32,
    pub correction_ms: f32,
    pub uncertainty_ms: f32,
}

impl Default for CalibrationEstimate {
    fn default() -> Self {
        let hz = 60.0;
        let period = 1000.0 / hz;
        Self {
            assumed_refresh_hz: hz,
            correction_ms: period / 2.0,
            uncertainty_ms: period / 2.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MeasurementStats {
    pub instant: Option<MeasurementSample>,
    pub average_latency_ms: Option<f32>,
    pub corrected_latency_ms: Option<f32>,
    pub corrected_average_latency_ms: Option<f32>,
    pub sample_count: u64,
    pub dropped_samples: u64,
    sum_latency_ms: f64,
}

impl MeasurementStats {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn push(
        &mut self,
        display_state: u32,
        captured_state: u32,
        confidence: f32,
        calibration: CalibrationEstimate,
    ) -> Option<MeasurementSample> {
        let delta = modular_latency_delta(display_state, captured_state)?;
        let latency_ms = delta as f32;
        let sample = MeasurementSample {
            latency_ms,
            confidence,
        };

        self.instant = Some(sample);
        self.sample_count += 1;
        self.sum_latency_ms += latency_ms as f64;
        self.average_latency_ms = Some((self.sum_latency_ms / self.sample_count as f64) as f32);
        self.corrected_latency_ms = Some((latency_ms - calibration.correction_ms).max(0.0));
        self.corrected_average_latency_ms = self
            .average_latency_ms
            .map(|avg| (avg - calibration.correction_ms).max(0.0));

        Some(sample)
    }

    pub fn reject(&mut self) {
        self.dropped_samples += 1;
    }
}

pub fn modular_latency_delta(display_state: u32, captured_state: u32) -> Option<u32> {
    let delta = (display_state + STATE_SPACE - captured_state) % STATE_SPACE;
    if delta <= MAX_LATENCY_MS + 250 {
        Some(delta)
    } else {
        None
    }
}
