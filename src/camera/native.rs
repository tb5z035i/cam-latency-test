use crate::camera::protocol::PixelFormat;
use crate::camera::{CameraDescriptor, CameraOrigin, FramePacket};
use anyhow::{Context, Result};
use ccap::{PixelFormat as CcapPixelFormat, Provider};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct NativeCameraStream {
    rx: Receiver<FramePacket>,
    stop_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl NativeCameraStream {
    pub fn try_recv(&self) -> Option<FramePacket> {
        let mut latest = None;
        while let Ok(frame) = self.rx.try_recv() {
            latest = Some(frame);
        }
        latest
    }

    pub fn stop(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn list_native_sources() -> Result<Vec<CameraDescriptor>> {
    let devices = Provider::get_devices()?;
    Ok(devices
        .into_iter()
        .map(|device| {
            let best_resolution = device
                .supported_resolutions
                .iter()
                .max_by_key(|resolution| resolution.width * resolution.height)
                .cloned();
            CameraDescriptor {
                id: format!("native:{}", device.name),
                name: device.name,
                width: best_resolution.as_ref().map(|resolution| resolution.width).unwrap_or_default(),
                height: best_resolution
                    .as_ref()
                    .map(|resolution| resolution.height)
                    .unwrap_or_default(),
                nominal_fps: None,
                origin: CameraOrigin::Native,
            }
        })
        .collect())
}

pub fn open_native_camera(source: &CameraDescriptor) -> Result<NativeCameraStream> {
    let uri = source
        .id
        .strip_prefix("native:")
        .context("invalid native source id")?
        .to_owned();

    let (tx, rx) = bounded(2);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let thread_stop = stop_flag.clone();
    let source_name = source.name.clone();

    let join_handle = thread::spawn(move || {
        if let Err(error) = capture_loop(uri, source_name, tx, thread_stop) {
            eprintln!("native camera stream ended: {error:?}");
        }
    });

    Ok(NativeCameraStream {
        rx,
        stop_flag,
        join_handle: Some(join_handle),
    })
}

fn capture_loop(
    device_name: String,
    source_name: String,
    tx: Sender<FramePacket>,
    stop_flag: Arc<AtomicBool>,
) -> Result<()> {
    let mut provider = Provider::with_device_name(&device_name)?;
    provider.open()?;
    provider
        .set_pixel_format(CcapPixelFormat::Rgb24)
        .or_else(|_| provider.set_pixel_format(CcapPixelFormat::Bgr24))
        .ok();
    provider.start_capture()?;
    let mut sequence = 0u64;
    let start = Instant::now();

    while !stop_flag.load(Ordering::Relaxed) {
        let Some(frame) = provider.grab_frame(1000)? else {
            continue;
        };
        let data = frame.data()?.to_vec();
        let (pixel_format, converted) = normalize_frame(&frame, data)?;
        let packet = FramePacket {
            width: frame.width(),
            height: frame.height(),
            pixel_format,
            sequence,
            source_name: source_name.clone(),
            timestamp_millis: Some(start.elapsed().as_millis()),
            data: converted,
        };
        sequence += 1;

        let _ = tx.try_send(packet);

        thread::sleep(Duration::from_millis(1));
    }

    Ok(())
}

fn normalize_frame(frame: &ccap::VideoFrame, data: Vec<u8>) -> Result<(PixelFormat, Vec<u8>)> {
    match frame.pixel_format() {
        CcapPixelFormat::Rgb24 => Ok((PixelFormat::Rgb8, data)),
        CcapPixelFormat::Bgr24 => {
            let mut rgb = data;
            for pixel in rgb.chunks_exact_mut(3) {
                pixel.swap(0, 2);
            }
            Ok((PixelFormat::Rgb8, rgb))
        }
        CcapPixelFormat::Rgba32 => {
            let mut rgb = Vec::with_capacity((frame.width() * frame.height() * 3) as usize);
            for pixel in data.chunks_exact(4) {
                rgb.extend_from_slice(&[pixel[0], pixel[1], pixel[2]]);
            }
            Ok((PixelFormat::Rgb8, rgb))
        }
        CcapPixelFormat::Bgra32 => {
            let mut rgb = Vec::with_capacity((frame.width() * frame.height() * 3) as usize);
            for pixel in data.chunks_exact(4) {
                rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            }
            Ok((PixelFormat::Rgb8, rgb))
        }
        other => anyhow::bail!("unsupported native pixel format: {:?}", other),
    }
}
