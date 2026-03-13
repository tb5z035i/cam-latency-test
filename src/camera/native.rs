use crate::camera::protocol::PixelFormat;
use crate::camera::{CameraDescriptor, CameraOrigin, FramePacket};
use anyhow::{anyhow, Context, Result};
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
    let provider = Provider::new()?;
    let device_names = provider.list_devices()?;
    let mut sources = Vec::new();

    for (index, fallback_name) in device_names.into_iter().enumerate() {
        if let Some(source) = probe_native_source(index as i32, &fallback_name) {
            sources.push(source);
        }
    }

    Ok(sources)
}

pub fn open_native_camera(source: &CameraDescriptor) -> Result<NativeCameraStream> {
    let native_index = parse_native_source_index(&source.id)?;

    let (tx, rx) = bounded(2);
    let (startup_tx, startup_rx) = bounded(1);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let thread_stop = stop_flag.clone();
    let source_name = source.name.clone();

    let join_handle = thread::spawn(move || {
        let result = capture_loop(native_index, source_name, tx, thread_stop, Some(startup_tx));
        if let Err(error) = &result {
            eprintln!("native camera stream ended: {error:?}");
        }
    });

    match startup_rx
        .recv_timeout(Duration::from_secs(3))
        .unwrap_or_else(|_| Err("camera startup timed out".to_owned()))
    {
        Ok(()) => Ok(NativeCameraStream {
            rx,
            stop_flag,
            join_handle: Some(join_handle),
        }),
        Err(error) => {
            stop_flag.store(true, Ordering::Relaxed);
            let _ = join_handle.join();
            Err(anyhow!(error))
        }
    }
}

fn capture_loop(
    device_index: i32,
    source_name: String,
    tx: Sender<FramePacket>,
    stop_flag: Arc<AtomicBool>,
    startup_tx: Option<Sender<std::result::Result<(), String>>>,
) -> Result<()> {
    let mut startup_tx = startup_tx;
    let mut provider = match Provider::with_device(device_index) {
        Ok(provider) => provider,
        Err(error) => {
            if let Some(startup_tx) = startup_tx.take() {
                let _ = startup_tx.send(Err(format!(
                    "Failed to open native camera index {device_index}: {error}"
                )));
            }
            return Err(anyhow!(
                "Failed to open native camera index {device_index}: {error}"
            ));
        }
    };
    select_preview_pixel_format(&mut provider);
    if let Err(error) = provider.start_capture() {
        if let Some(startup_tx) = startup_tx.take() {
            let _ = startup_tx.send(Err(format!(
                "Failed to start native camera index {device_index}: {error}"
            )));
        }
        return Err(anyhow!(
            "Failed to start native camera index {device_index}: {error}"
        ));
    }
    if let Some(startup_tx) = startup_tx.take() {
        let _ = startup_tx.send(Ok(()));
    }
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
            timestamp_millis: Some(start.elapsed().as_millis() as u64),
            data: converted,
        };
        sequence += 1;

        let _ = tx.try_send(packet);

        thread::sleep(Duration::from_millis(1));
    }

    Ok(())
}

fn select_preview_pixel_format(provider: &mut Provider) {
    provider
        .set_pixel_format(CcapPixelFormat::Rgb24)
        .or_else(|_| provider.set_pixel_format(CcapPixelFormat::Bgr24))
        .or_else(|_| provider.set_pixel_format(CcapPixelFormat::Rgba32))
        .or_else(|_| provider.set_pixel_format(CcapPixelFormat::Bgra32))
        .ok();
}

fn probe_native_source(index: i32, fallback_name: &str) -> Option<CameraDescriptor> {
    let mut provider = Provider::with_device(index).ok()?;
    let info = provider.device_info().ok();
    let source_name = info
        .as_ref()
        .map(|details| details.name.clone())
        .unwrap_or_else(|| fallback_name.to_owned());
    let best_resolution = info.as_ref().and_then(|details| {
        details
            .supported_resolutions
            .iter()
            .max_by_key(|resolution| resolution.width * resolution.height)
            .cloned()
    });
    select_preview_pixel_format(&mut provider);
    if provider.start_capture().is_err() {
        return None;
    }
    let _ = provider.stop_capture();

    let label = match best_resolution {
        Some(ref resolution) => {
            format!(
                "{source_name} [native #{index}, {}x{}]",
                resolution.width, resolution.height
            )
        }
        None => format!("{source_name} [native #{index}]"),
    };

    Some(CameraDescriptor {
        id: native_source_id(index),
        name: label,
        width: best_resolution
            .as_ref()
            .map(|resolution| resolution.width)
            .unwrap_or_default(),
        height: best_resolution
            .as_ref()
            .map(|resolution| resolution.height)
            .unwrap_or_default(),
        nominal_fps: None,
        origin: CameraOrigin::Native,
    })
}

fn native_source_id(index: i32) -> String {
    format!("native-index:{index}")
}

fn parse_native_source_index(id: &str) -> Result<i32> {
    id.strip_prefix("native-index:")
        .context("invalid native source id")?
        .parse::<i32>()
        .context("invalid native source index")
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

#[cfg(test)]
mod tests {
    use super::{native_source_id, parse_native_source_index};

    #[test]
    fn native_source_id_round_trips() {
        let id = native_source_id(7);
        assert_eq!(parse_native_source_index(&id).unwrap(), 7);
    }
}
