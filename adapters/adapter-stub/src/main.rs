use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, BufRead, Write},
    thread,
    time::{Duration, Instant},
};

const PROTOCOL_VERSION: u32 = 1;
const GRID_SIZE: usize = 25;
const STATE_BITS: usize = 17;
const CHECKSUM_BITS: usize = 7;
const PAYLOAD_BITS: usize = STATE_BITS + CHECKSUM_BITS;
const STATE_SPACE: u32 = 1 << STATE_BITS;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AdapterCommand {
    Hello { protocol_version: u32 },
    ListSources,
    Open { source_id: String },
    Start,
    Stop,
    Close,
}

#[derive(Debug, Serialize, Deserialize)]
struct AdapterSourceInfo {
    id: String,
    name: String,
    width: u32,
    height: u32,
    nominal_fps: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PixelFormat {
    Rgb8,
}

#[derive(Debug, Serialize, Deserialize)]
struct AdapterFrame {
    width: u32,
    height: u32,
    pixel_format: PixelFormat,
    sequence: u64,
    timestamp_millis: Option<u64>,
    data_base64: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AdapterMessage {
    Hello {
        protocol_version: u32,
        adapter_name: String,
        adapter_version: String,
    },
    Sources {
        sources: Vec<AdapterSourceInfo>,
    },
    Ack {
        message: String,
    },
    Error {
        message: String,
    },
    Frame {
        frame: AdapterFrame,
    },
}

fn main() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut selected_source = None::<String>;

    for line in stdin.lock().lines() {
        let line = line?;
        let command: AdapterCommand = serde_json::from_str(&line)?;
        match command {
            AdapterCommand::Hello { protocol_version } => {
                send(
                    &mut stdout,
                    &AdapterMessage::Hello {
                        protocol_version: protocol_version.min(PROTOCOL_VERSION),
                        adapter_name: "Synthetic Adapter".to_owned(),
                        adapter_version: "0.1.0".to_owned(),
                    },
                )?;
            }
            AdapterCommand::ListSources => {
                send(
                    &mut stdout,
                    &AdapterMessage::Sources {
                        sources: vec![AdapterSourceInfo {
                            id: "synthetic-pattern".to_owned(),
                            name: "Synthetic timestamp pattern".to_owned(),
                            width: 800,
                            height: 800,
                            nominal_fps: Some(30.0),
                        }],
                    },
                )?;
            }
            AdapterCommand::Open { source_id } => {
                selected_source = Some(source_id.clone());
                send(
                    &mut stdout,
                    &AdapterMessage::Ack {
                        message: format!("opened {source_id}"),
                    },
                )?;
            }
            AdapterCommand::Start => {
                if selected_source.is_none() {
                    send(
                        &mut stdout,
                        &AdapterMessage::Error {
                            message: "open a source before start".to_owned(),
                        },
                    )?;
                    continue;
                }
                stream_frames(&mut stdout)?;
                break;
            }
            AdapterCommand::Stop | AdapterCommand::Close => break,
        }
    }

    Ok(())
}

fn send(stdout: &mut impl Write, message: &AdapterMessage) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *stdout, message)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

fn stream_frames(stdout: &mut impl Write) -> anyhow::Result<()> {
    let start = Instant::now();
    let mut sequence = 0u64;

    loop {
        let state = ((start.elapsed().as_millis() as u32) % STATE_SPACE).min(STATE_SPACE - 1);
        let rgb = render_canvas(800, 800, state);
        let frame = AdapterFrame {
            width: 800,
            height: 800,
            pixel_format: PixelFormat::Rgb8,
            sequence,
            timestamp_millis: Some(start.elapsed().as_millis() as u64),
            data_base64: base64::engine::general_purpose::STANDARD.encode(rgb),
        };
        send(stdout, &AdapterMessage::Frame { frame })?;
        sequence += 1;
        thread::sleep(Duration::from_millis(33));
    }
}

fn render_canvas(width: u32, height: u32, state: u32) -> Vec<u8> {
    let mut pixels = vec![255u8; (width * height * 3) as usize];
    let pattern = render_pattern(620, state);
    let offset_x = ((width - 620) / 2) as usize;
    let offset_y = ((height - 620) / 2) as usize;

    for y in 0..620usize {
        for x in 0..620usize {
            let src = (y * 620 + x) * 3;
            let dst = ((y + offset_y) * width as usize + (x + offset_x)) * 3;
            pixels[dst..dst + 3].copy_from_slice(&pattern[src..src + 3]);
        }
    }

    pixels
}

fn render_pattern(size_px: usize, state: u32) -> Vec<u8> {
    let mut pixels = vec![245u8; size_px * size_px * 3];
    let cell_size = size_px as f32 / GRID_SIZE as f32;
    let payload = payload_bits(state);

    for gy in 0..GRID_SIZE {
        for gx in 0..GRID_SIZE {
            let dark = cell_value(gx, gy, &payload);
            let x0 = (gx as f32 * cell_size).round() as usize;
            let y0 = (gy as f32 * cell_size).round() as usize;
            let x1 = (((gx + 1) as f32) * cell_size).round() as usize;
            let y1 = (((gy + 1) as f32) * cell_size).round() as usize;
            let color = if dark { 10u8 } else { 245u8 };
            for y in y0..y1.min(size_px) {
                for x in x0..x1.min(size_px) {
                    let idx = (y * size_px + x) * 3;
                    pixels[idx] = color;
                    pixels[idx + 1] = color;
                    pixels[idx + 2] = color;
                }
            }
        }
    }

    pixels
}

fn payload_bits(state: u32) -> [bool; PAYLOAD_BITS] {
    let gray = state ^ (state >> 1);
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

fn checksum7(gray: u32) -> u8 {
    let mut value = gray.wrapping_mul(0x45d9_f3b);
    value ^= value >> 9;
    value ^= value >> 17;
    (value & 0x7f) as u8
}

fn cell_value(gx: usize, gy: usize, payload: &[bool; PAYLOAD_BITS]) -> bool {
    if gx == 0 || gy == 0 || gx == GRID_SIZE - 1 || gy == GRID_SIZE - 1 {
        return true;
    }
    if let Some(value) = finder_at(1, 1, 7, gx, gy) {
        return value;
    }
    if let Some(value) = finder_at(GRID_SIZE - 8, 1, 7, gx, gy) {
        return value;
    }
    if let Some(value) = finder_at(1, GRID_SIZE - 8, 7, gx, gy) {
        return value;
    }
    if let Some(value) = finder_at(GRID_SIZE - 6, GRID_SIZE - 6, 5, gx, gy) {
        return value;
    }
    if (gx == 11 || gx == 12 || gx == 13) && (gy == 2 || gy == 3) {
        return false;
    }
    if (gx == 11 || gx == 12 || gx == 13) && (gy == GRID_SIZE - 4 || gy == GRID_SIZE - 3) {
        return true;
    }

    for &(px, py, bit_idx) in &payload_positions() {
        if gx == px && gy == py {
            return payload[bit_idx];
        }
    }
    false
}

fn finder_at(origin_x: usize, origin_y: usize, size: usize, gx: usize, gy: usize) -> Option<bool> {
    if gx < origin_x || gy < origin_y || gx >= origin_x + size || gy >= origin_y + size {
        return None;
    }
    let lx = gx - origin_x;
    let ly = gy - origin_y;
    let ring = lx.min(ly).min(size - 1 - lx).min(size - 1 - ly);
    Some(matches!(ring, 0 | 2))
}

fn payload_positions() -> [(usize, usize, usize); PAYLOAD_BITS * 2] {
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
