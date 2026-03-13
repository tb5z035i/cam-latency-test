use anyhow::{anyhow, ensure, Result};
use base64::Engine;
use realsense_rust::{
    config::Config,
    context::Context,
    frame::ColorFrame,
    kind::{Rs2CameraInfo, Rs2Format, Rs2StreamKind},
    pipeline::InactivePipeline,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    convert::TryFrom,
    io::{self, BufRead, Write},
    time::Duration,
};

const PROTOCOL_VERSION: u32 = 1;

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

fn main() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut selected_serial = None::<String>;

    for line in stdin.lock().lines() {
        let line = line?;
        let command: AdapterCommand = serde_json::from_str(&line)?;
        match command {
            AdapterCommand::Hello { protocol_version } => {
                send(
                    &mut stdout,
                    &AdapterMessage::Hello {
                        protocol_version: protocol_version.min(PROTOCOL_VERSION),
                        adapter_name: "RealSense Adapter Example".to_owned(),
                        adapter_version: "0.1.0".to_owned(),
                    },
                )?;
            }
            AdapterCommand::ListSources => {
                let sources = enumerate_sources()?;
                send(&mut stdout, &AdapterMessage::Sources { sources })?;
            }
            AdapterCommand::Open { source_id } => {
                selected_serial = Some(source_id.clone());
                send(
                    &mut stdout,
                    &AdapterMessage::Ack {
                        message: format!("opened {source_id}"),
                    },
                )?;
            }
            AdapterCommand::Start => {
                let serial = selected_serial
                    .clone()
                    .ok_or_else(|| anyhow!("open a source before start"))?;
                stream_realsense(&mut stdout, &serial)?;
                break;
            }
            AdapterCommand::Stop | AdapterCommand::Close => break,
        }
    }

    Ok(())
}

fn enumerate_sources() -> Result<Vec<AdapterSourceInfo>> {
    let context = Context::new()?;
    let devices = context.query_devices(HashSet::new());
    ensure!(!devices.is_empty(), "No RealSense devices found");

    let mut sources = Vec::new();
    for device in devices {
        let serial = info_string(&device, Rs2CameraInfo::SerialNumber)
            .ok_or_else(|| anyhow!("device missing serial number"))?;
        let name = info_string(&device, Rs2CameraInfo::Name).unwrap_or_else(|| "RealSense".to_owned());
        sources.push(AdapterSourceInfo {
            id: serial,
            name,
            width: 640,
            height: 480,
            nominal_fps: Some(30.0),
        });
    }

    Ok(sources)
}

fn stream_realsense(stdout: &mut impl Write, serial: &str) -> Result<()> {
    let context = Context::new()?;
    let pipeline = InactivePipeline::try_from(&context)?;
    let mut config = Config::new();
    config
        .enable_device_from_serial(serial)?
        .disable_all_streams()?
        .enable_stream(Rs2StreamKind::Color, None, 640, 480, Rs2Format::Rgb8, 30)?;

    let mut pipeline = pipeline.start(Some(config))?;
    let mut sequence = 0u64;

    loop {
        let frames = pipeline.wait(Some(Duration::from_millis(1000)))?;
        let mut color_frames = frames.frames_of_type::<ColorFrame>();
        let Some(color) = color_frames.pop() else {
            continue;
        };

        let data = unsafe {
            std::slice::from_raw_parts(
                color.get_data() as *const _ as *const u8,
                color.get_data_size(),
            )
        }
        .to_vec();

        let frame = AdapterFrame {
            width: color.width() as u32,
            height: color.height() as u32,
            pixel_format: PixelFormat::Rgb8,
            sequence,
            timestamp_millis: None,
            data_base64: base64::engine::general_purpose::STANDARD.encode(data),
        };
        send(stdout, &AdapterMessage::Frame { frame })?;
        sequence += 1;
    }
}

fn info_string(
    device: &realsense_rust::device::Device,
    info: Rs2CameraInfo,
) -> Option<String> {
    device
        .info(info)
        .and_then(|value| value.to_str().ok().map(|s| s.to_owned()))
}

fn send(stdout: &mut impl Write, message: &AdapterMessage) -> Result<()> {
    serde_json::to_writer(&mut *stdout, message)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
