use crate::{
    detect::{detect_roi, rectify_rgb, rgb_to_gray, DetectionCandidate, PointF},
    pattern::{
        calibration_expected_dark, calibration_patch_bounds, decode_payload, payload_positions,
        static_cell_value, PatternMode, GRID_SIZE, PAYLOAD_BITS,
    },
};
use image::{imageops, GrayImage, Rgb, RgbImage};
use imageproc::drawing::draw_line_segment_mut;

const RECTIFIED_SIZE: u32 = 500;

#[derive(Clone, Debug)]
pub struct DecodeResult {
    pub state: u32,
    pub confidence: f32,
    pub roi: [PointF; 4],
    pub overlay: RgbImage,
    pub detection_score: f32,
    pub static_score: f32,
    pub calibration_match: f32,
}

pub fn decode_image(image: &RgbImage, mode: PatternMode) -> Option<DecodeResult> {
    let gray = rgb_to_gray(image);
    let candidate = detect_roi(&gray)?;
    let rectified = rectify_rgb(image, &candidate.corners, RECTIFIED_SIZE)?;

    let (rotation, rotated_rgb, static_score) = best_orientation(&rectified)?;
    let rotated_gray = rgb_to_gray(&rotated_rgb);
    let (black_mean, white_mean) = reference_levels(&rotated_gray);
    let midpoint = (black_mean + white_mean) / 2.0;
    let payload = read_payload_bits(&rotated_gray, midpoint);
    let state = decode_payload(&payload.bits)?;
    let calibration_match =
        calibration_match_score(&rotated_gray, midpoint, state, mode, black_mean, white_mean);
    let confidence = (payload.confidence + static_score + calibration_match) / 3.0;

    if confidence < 0.45 {
        return None;
    }

    let mut overlay = image.clone();
    draw_roi_overlay(&mut overlay, &candidate, rotation);

    Some(DecodeResult {
        state,
        confidence,
        roi: candidate.corners,
        overlay,
        detection_score: candidate.score,
        static_score,
        calibration_match,
    })
}

fn best_orientation(rectified: &RgbImage) -> Option<(usize, RgbImage, f32)> {
    let candidates = [
        (0usize, rectified.clone()),
        (1usize, imageops::rotate90(rectified)),
        (2usize, imageops::rotate180(rectified)),
        (3usize, imageops::rotate270(rectified)),
    ];

    candidates
        .into_iter()
        .filter_map(|(rotation, image)| {
            let score = static_match_score(&rgb_to_gray(&image));
            (score > 0.55).then_some((rotation, image, score))
        })
        .max_by(|left, right| {
            left.2
                .partial_cmp(&right.2)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn static_match_score(image: &GrayImage) -> f32 {
    let mut total = 0.0;
    let mut count = 0.0;
    for gy in 0..GRID_SIZE {
        for gx in 0..GRID_SIZE {
            if let Some(expected_dark) = static_cell_value(gx, gy) {
                let sample = sample_cell(image, gx, gy);
                let observed = 1.0 - sample;
                let expected = if expected_dark { 1.0 } else { 0.0 };
                total += 1.0 - (observed - expected).abs();
                count += 1.0;
            }
        }
    }

    if count == 0.0 {
        0.0
    } else {
        total / count
    }
}

fn reference_levels(image: &GrayImage) -> (f32, f32) {
    let mut black_sum = 0.0;
    let mut black_count = 0.0;
    let mut white_sum = 0.0;
    let mut white_count = 0.0;

    for gy in 0..GRID_SIZE {
        for gx in 0..GRID_SIZE {
            if let Some(expected_dark) = static_cell_value(gx, gy) {
                let sample = sample_cell(image, gx, gy);
                if expected_dark {
                    black_sum += sample;
                    black_count += 1.0;
                } else {
                    white_sum += sample;
                    white_count += 1.0;
                }
            }
        }
    }

    let black_mean = if black_count > 0.0 {
        black_sum / black_count
    } else {
        0.0
    };
    let white_mean = if white_count > 0.0 {
        white_sum / white_count
    } else {
        1.0
    };

    (black_mean, white_mean)
}

fn read_payload_bits(image: &GrayImage, midpoint: f32) -> PayloadObservation {
    let mut bit_votes = [[0.0f32; 2]; PAYLOAD_BITS];

    for &(gx, gy, bit_idx) in payload_positions() {
        let sample = sample_cell(image, gx, gy);
        let dark_confidence = (midpoint - sample).abs();
        let bit = usize::from(sample < midpoint);
        bit_votes[bit_idx][bit] += dark_confidence.max(0.01);
    }

    let mut bits = [false; PAYLOAD_BITS];
    let mut confidence_sum = 0.0;
    for (bit_idx, votes) in bit_votes.iter().enumerate() {
        bits[bit_idx] = votes[1] >= votes[0];
        confidence_sum += votes[0].max(votes[1]) / (votes[0] + votes[1]).max(0.001);
    }

    PayloadObservation {
        bits,
        confidence: confidence_sum / PAYLOAD_BITS as f32,
    }
}

fn calibration_match_score(
    image: &GrayImage,
    midpoint: f32,
    state: u32,
    mode: PatternMode,
    black_mean: f32,
    white_mean: f32,
) -> f32 {
    let ((x0, y0), (x1, y1)) = calibration_patch_bounds();
    let mut sum = 0.0;
    let mut count = 0.0;
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            sum += sample_cell(image, gx, gy);
            count += 1.0;
        }
    }

    let observed = if count > 0.0 { sum / count } else { midpoint };
    let expected_dark = match mode {
        PatternMode::Timestamp => false,
        PatternMode::Calibration => calibration_expected_dark(state),
    };
    let expected = if expected_dark { black_mean } else { white_mean };
    (1.0 - (observed - expected).abs()).clamp(0.0, 1.0)
}

fn sample_cell(image: &GrayImage, gx: usize, gy: usize) -> f32 {
    let cell_size = image.width() as f32 / GRID_SIZE as f32;
    let center_x = ((gx as f32 + 0.5) * cell_size).round() as i32;
    let center_y = ((gy as f32 + 0.5) * cell_size).round() as i32;
    let radius = (cell_size * 0.22).max(1.0) as i32;

    let mut sum = 0.0;
    let mut count = 0.0;

    for y in (center_y - radius)..=(center_y + radius) {
        for x in (center_x - radius)..=(center_x + radius) {
            if x < 0 || y < 0 || x >= image.width() as i32 || y >= image.height() as i32 {
                continue;
            }
            sum += image.get_pixel(x as u32, y as u32)[0] as f32 / 255.0;
            count += 1.0;
        }
    }

    if count == 0.0 {
        1.0
    } else {
        sum / count
    }
}

fn draw_roi_overlay(image: &mut RgbImage, candidate: &DetectionCandidate, _rotation: usize) {
    let color = Rgb([255, 64, 64]);
    for index in 0..4 {
        let start = candidate.corners[index];
        let end = candidate.corners[(index + 1) % 4];
        draw_line_segment_mut(image, (start.x, start.y), (end.x, end.y), color);
    }
}

struct PayloadObservation {
    bits: [bool; PAYLOAD_BITS],
    confidence: f32,
}
