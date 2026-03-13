use image::{Rgb, RgbImage};
use std::time::{Duration, Instant};

pub const GRID_SIZE: usize = 25;
pub const STATE_BITS: usize = 17;
pub const CHECKSUM_BITS: usize = 7;
pub const PAYLOAD_BITS: usize = STATE_BITS + CHECKSUM_BITS;
pub const STATE_SPACE: u32 = 1 << STATE_BITS;
pub const MAX_LATENCY_MS: u32 = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternMode {
    Timestamp,
    Calibration,
}

#[derive(Clone, Debug)]
pub struct PatternFrame {
    pub state: u32,
    pub image: RgbImage,
}

#[derive(Clone, Copy, Debug)]
pub struct PatternRuntime {
    start: Instant,
}

impl PatternRuntime {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    pub fn current_state(&self) -> u32 {
        ((self.elapsed().as_millis() as u32) % STATE_SPACE).min(STATE_SPACE - 1)
    }
}

pub fn render_frame(size_px: u32, state: u32, mode: PatternMode) -> PatternFrame {
    PatternFrame {
        state,
        image: render_pattern(size_px, state, mode),
    }
}

pub fn render_pattern(size_px: u32, state: u32, mode: PatternMode) -> RgbImage {
    let mut image = RgbImage::from_pixel(size_px, size_px, Rgb([245, 245, 245]));
    let cell_size = (size_px as f32 / GRID_SIZE as f32).max(1.0);
    let payload = payload_bits(state);

    for gy in 0..GRID_SIZE {
        for gx in 0..GRID_SIZE {
            let is_dark = cell_value(gx, gy, &payload, mode);
            draw_cell(
                &mut image,
                gx,
                gy,
                cell_size,
                if is_dark { Rgb([10, 10, 10]) } else { Rgb([245, 245, 245]) },
            );
        }
    }

    image
}

pub fn payload_bits(state: u32) -> [bool; PAYLOAD_BITS] {
    let gray = to_gray(state);
    let checksum = checksum7(gray);
    let mut out = [false; PAYLOAD_BITS];

    for bit in 0..STATE_BITS {
        out[bit] = ((gray >> bit) & 1) != 0;
    }

    for bit in 0..CHECKSUM_BITS {
        out[STATE_BITS + bit] = ((checksum >> bit) & 1) != 0;
    }

    out
}

pub fn decode_payload(bits: &[bool]) -> Option<u32> {
    if bits.len() != PAYLOAD_BITS {
        return None;
    }

    let mut gray = 0u32;
    for bit in 0..STATE_BITS {
        if bits[bit] {
            gray |= 1 << bit;
        }
    }

    let mut checksum = 0u8;
    for bit in 0..CHECKSUM_BITS {
        if bits[STATE_BITS + bit] {
            checksum |= 1 << bit;
        }
    }

    if checksum7(gray) != checksum {
        return None;
    }

    Some(from_gray(gray) % STATE_SPACE)
}

pub fn static_cell_value(gx: usize, gy: usize) -> Option<bool> {
    if gx >= GRID_SIZE || gy >= GRID_SIZE {
        return None;
    }

    if gx == 0 || gy == 0 || gx == GRID_SIZE - 1 || gy == GRID_SIZE - 1 {
        return Some(true);
    }

    if let Some(value) = finder_at(1, 1, 7, gx, gy) {
        return Some(value);
    }

    if let Some(value) = finder_at(GRID_SIZE - 8, 1, 7, gx, gy) {
        return Some(value);
    }

    if let Some(value) = finder_at(1, GRID_SIZE - 8, 7, gx, gy) {
        return Some(value);
    }

    if let Some(value) = finder_at(GRID_SIZE - 6, GRID_SIZE - 6, 5, gx, gy) {
        return Some(value);
    }

    if (gx == 11 || gx == 12 || gx == 13) && (gy == 2 || gy == 3) {
        return Some(false);
    }

    if (gx == 11 || gx == 12 || gx == 13) && (gy == GRID_SIZE - 4 || gy == GRID_SIZE - 3) {
        return Some(true);
    }

    if (gy == 11 || gy == 12 || gy == 13) && (gx == 2 || gx == 3) {
        return Some(true);
    }

    if (gy == 11 || gy == 12 || gy == 13) && (gx == GRID_SIZE - 4 || gx == GRID_SIZE - 3) {
        return Some(false);
    }

    None
}

pub fn payload_positions() -> &'static [(usize, usize, usize)] {
    &PAYLOAD_POSITIONS
}

pub fn calibration_patch_bounds() -> ((usize, usize), (usize, usize)) {
    ((9, 17), (15, 21))
}

pub fn calibration_expected_dark(state: u32) -> bool {
    ((state >> 5) & 1) != 0
}

pub fn to_gray(value: u32) -> u32 {
    value ^ (value >> 1)
}

pub fn from_gray(mut gray: u32) -> u32 {
    let mut binary = 0u32;
    while gray != 0 {
        binary ^= gray;
        gray >>= 1;
    }
    binary
}

fn checksum7(gray: u32) -> u8 {
    let mut value = gray.wrapping_mul(0x45d9_f3b);
    value ^= value >> 9;
    value ^= value >> 17;
    (value & 0x7f) as u8
}

fn cell_value(gx: usize, gy: usize, payload: &[bool; PAYLOAD_BITS], mode: PatternMode) -> bool {
    if let Some(value) = static_cell_value(gx, gy) {
        return value;
    }

    if let Some(value) = payload_cell_value(gx, gy, payload) {
        return value;
    }

    let ((x0, y0), (x1, y1)) = calibration_patch_bounds();
    if gx >= x0 && gx <= x1 && gy >= y0 && gy <= y1 {
        return match mode {
            PatternMode::Timestamp => false,
            PatternMode::Calibration => calibration_expected_dark(from_payload(payload)),
        };
    }

    false
}

fn from_payload(payload: &[bool; PAYLOAD_BITS]) -> u32 {
    let mut gray = 0u32;
    for bit in 0..STATE_BITS {
        if payload[bit] {
            gray |= 1 << bit;
        }
    }
    from_gray(gray)
}

fn payload_cell_value(gx: usize, gy: usize, payload: &[bool; PAYLOAD_BITS]) -> Option<bool> {
    for &(px, py, bit_idx) in payload_positions() {
        if px == gx && py == gy {
            return Some(payload[bit_idx]);
        }
    }
    None
}

fn draw_cell(image: &mut RgbImage, gx: usize, gy: usize, cell_size: f32, color: Rgb<u8>) {
    let x0 = (gx as f32 * cell_size).round() as u32;
    let y0 = (gy as f32 * cell_size).round() as u32;
    let x1 = (((gx + 1) as f32) * cell_size).round() as u32;
    let y1 = (((gy + 1) as f32) * cell_size).round() as u32;

    for y in y0..y1.min(image.height()) {
        for x in x0..x1.min(image.width()) {
            image.put_pixel(x, y, color);
        }
    }
}

fn finder_at(origin_x: usize, origin_y: usize, size: usize, gx: usize, gy: usize) -> Option<bool> {
    if gx < origin_x || gy < origin_y || gx >= origin_x + size || gy >= origin_y + size {
        return None;
    }

    let lx = gx - origin_x;
    let ly = gy - origin_y;
    let ring = lx.min(ly).min(size - 1 - lx).min(size - 1 - ly);

    Some(match size {
        7 => matches!(ring, 0 | 2),
        5 => matches!(ring, 0 | 2),
        _ => false,
    })
}

const PAYLOAD_POSITIONS: [(usize, usize, usize); PAYLOAD_BITS * 2] = build_payload_positions();

const fn build_payload_positions() -> [(usize, usize, usize); PAYLOAD_BITS * 2] {
    let mut positions = [(0usize, 0usize, 0usize); PAYLOAD_BITS * 2];
    let mut idx = 0usize;
    let mut bit = 0usize;

    while bit < PAYLOAD_BITS {
        positions[idx] = (9 + (bit % 6), 9 + (bit / 6), bit);
        idx += 1;
        bit += 1;
    }

    bit = 0;
    while bit < PAYLOAD_BITS {
        positions[idx] = (16 + (bit % 6), 9 + (bit / 6), bit);
        idx += 1;
        bit += 1;
    }

    positions
}
