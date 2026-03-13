use crate::measurement::CalibrationEstimate;

#[derive(Clone, Debug)]
pub struct CalibrationState {
    pub enabled: bool,
    pub assumed_refresh_hz: f32,
    pub edge_contrast_score: f32,
}

impl Default for CalibrationState {
    fn default() -> Self {
        Self {
            enabled: false,
            assumed_refresh_hz: 60.0,
            edge_contrast_score: 1.0,
        }
    }
}

impl CalibrationState {
    pub fn estimate(&self) -> CalibrationEstimate {
        let refresh_hz = self.assumed_refresh_hz.clamp(24.0, 360.0);
        let refresh_period_ms = 1000.0 / refresh_hz;
        let blur_penalty = (1.0 - self.edge_contrast_score.clamp(0.0, 1.0)) * refresh_period_ms;

        CalibrationEstimate {
            assumed_refresh_hz: refresh_hz,
            correction_ms: refresh_period_ms / 2.0,
            uncertainty_ms: (refresh_period_ms / 2.0) + blur_penalty / 2.0,
        }
    }

    pub fn update_edge_contrast(&mut self, left_mean: f32, right_mean: f32) {
        let contrast = (left_mean - right_mean).abs().clamp(0.0, 1.0);
        self.edge_contrast_score = (self.edge_contrast_score * 0.9) + (contrast * 0.1);
    }
}
