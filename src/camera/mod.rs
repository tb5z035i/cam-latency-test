pub mod bridge;
pub mod native;
pub mod protocol;

use crate::camera::protocol::PixelFormat;
use anyhow::Result;
use image::{GrayImage, ImageBuffer, RgbImage};

#[derive(Clone, Debug)]
pub enum CameraOrigin {
    Native,
    Adapter { command: String },
}

#[derive(Clone, Debug)]
pub struct CameraDescriptor {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub nominal_fps: Option<f32>,
    pub origin: CameraOrigin,
}

#[derive(Clone, Debug)]
pub struct FramePacket {
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub sequence: u64,
    pub source_name: String,
    pub timestamp_millis: Option<u128>,
    pub data: Vec<u8>,
}

impl FramePacket {
    pub fn into_rgb_image(self) -> Option<RgbImage> {
        match self.pixel_format {
            PixelFormat::Rgb8 => ImageBuffer::from_raw(self.width, self.height, self.data),
            PixelFormat::Gray8 => {
                let gray = GrayImage::from_raw(self.width, self.height, self.data)?;
                let mut rgb = RgbImage::new(gray.width(), gray.height());
                for (x, y, pixel) in gray.enumerate_pixels() {
                    let value = pixel[0];
                    rgb.put_pixel(x, y, image::Rgb([value, value, value]));
                }
                Some(rgb)
            }
        }
    }
}

pub enum ActiveCamera {
    Native(native::NativeCameraStream),
    Adapter(bridge::AdapterCameraStream),
}

impl ActiveCamera {
    pub fn try_recv(&self) -> Option<FramePacket> {
        match self {
            ActiveCamera::Native(stream) => stream.try_recv(),
            ActiveCamera::Adapter(stream) => stream.try_recv(),
        }
    }

    pub fn stop(self) {
        match self {
            ActiveCamera::Native(stream) => stream.stop(),
            ActiveCamera::Adapter(stream) => stream.stop(),
        }
    }
}

pub fn list_native_sources() -> Result<Vec<CameraDescriptor>> {
    native::list_native_sources()
}

pub fn probe_adapter(command_line: &str) -> Result<Vec<CameraDescriptor>> {
    bridge::probe_adapter(command_line)
}

pub fn open_camera(source: &CameraDescriptor) -> Result<ActiveCamera> {
    match &source.origin {
        CameraOrigin::Native => native::open_native_camera(source).map(ActiveCamera::Native),
        CameraOrigin::Adapter { command } => {
            bridge::open_adapter_camera(command, source).map(ActiveCamera::Adapter)
        }
    }
}
