use image::{GrayImage, ImageBuffer, Luma, Rgb, RgbImage};
use imageproc::{
    contrast::otsu_level,
    geometric_transformations::{warp_into, Interpolation, Projection},
    region_labelling::{connected_components, Connectivity},
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug)]
pub struct DetectionCandidate {
    pub corners: [PointF; 4],
    pub score: f32,
    pub area: u32,
}

pub fn rgb_to_gray(image: &RgbImage) -> GrayImage {
    ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
        let pixel = image.get_pixel(x, y);
        let r = pixel[0] as f32;
        let g = pixel[1] as f32;
        let b = pixel[2] as f32;
        let gray = (0.2126 * r + 0.7152 * g + 0.0722 * b) as u8;
        Luma([gray])
    })
}

pub fn detect_roi(gray: &GrayImage) -> Option<DetectionCandidate> {
    let threshold = otsu_level(gray).saturating_sub(8);
    let mut mask = GrayImage::from_pixel(gray.width(), gray.height(), Luma([0]));

    for (x, y, pixel) in gray.enumerate_pixels() {
        if pixel[0] <= threshold {
            mask.put_pixel(x, y, Luma([255]));
        }
    }

    let labels: ImageBuffer<Luma<u32>, Vec<u32>> =
        connected_components(&mask, Connectivity::Eight, Luma([0u8]));
    let image_area = (gray.width() * gray.height()) as f32;

    let mut components: HashMap<u32, ComponentStats> = HashMap::new();
    for (x, y, pixel) in labels.enumerate_pixels() {
        let label = pixel[0];
        if label == 0 {
            continue;
        }

        let entry = components.entry(label).or_insert_with(|| ComponentStats::new(x, y));
        entry.update(x, y);
    }

    components
        .into_values()
        .filter_map(|component| component.to_candidate(image_area))
        .max_by(|left, right| left.score.partial_cmp(&right.score).unwrap_or(std::cmp::Ordering::Equal))
}

pub fn rectify_rgb(image: &RgbImage, corners: &[PointF; 4], output_size: u32) -> Option<RgbImage> {
    let from = [
        (corners[0].x, corners[0].y),
        (corners[1].x, corners[1].y),
        (corners[2].x, corners[2].y),
        (corners[3].x, corners[3].y),
    ];
    let to = [
        (0.0, 0.0),
        (output_size as f32 - 1.0, 0.0),
        (output_size as f32 - 1.0, output_size as f32 - 1.0),
        (0.0, output_size as f32 - 1.0),
    ];
    let projection = Projection::from_control_points(from, to)?;
    let mut out = RgbImage::from_pixel(output_size, output_size, Rgb([255, 255, 255]));
    warp_into(
        image,
        &projection,
        Interpolation::Nearest,
        Rgb([255, 255, 255]),
        &mut out,
    );
    Some(out)
}

#[derive(Clone, Debug)]
struct ComponentStats {
    area: u32,
    min_x: u32,
    max_x: u32,
    min_y: u32,
    max_y: u32,
    top_left: PointF,
    top_right: PointF,
    bottom_right: PointF,
    bottom_left: PointF,
    min_sum: i64,
    max_sum: i64,
    min_diff: i64,
    max_diff: i64,
}

impl ComponentStats {
    fn new(x: u32, y: u32) -> Self {
        let point = PointF {
            x: x as f32,
            y: y as f32,
        };
        let sum = (x + y) as i64;
        let diff = x as i64 - y as i64;
        Self {
            area: 0,
            min_x: x,
            max_x: x,
            min_y: y,
            max_y: y,
            top_left: point,
            top_right: point,
            bottom_right: point,
            bottom_left: point,
            min_sum: sum,
            max_sum: sum,
            min_diff: diff,
            max_diff: diff,
        }
    }

    fn update(&mut self, x: u32, y: u32) {
        self.area += 1;
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);

        let point = PointF {
            x: x as f32,
            y: y as f32,
        };
        let sum = (x + y) as i64;
        let diff = x as i64 - y as i64;

        if sum <= self.min_sum {
            self.min_sum = sum;
            self.top_left = point;
        }
        if diff >= self.max_diff {
            self.max_diff = diff;
            self.top_right = point;
        }
        if sum >= self.max_sum {
            self.max_sum = sum;
            self.bottom_right = point;
        }
        if diff <= self.min_diff {
            self.min_diff = diff;
            self.bottom_left = point;
        }
    }

    fn to_candidate(self, image_area: f32) -> Option<DetectionCandidate> {
        let width = self.max_x.saturating_sub(self.min_x) + 1;
        let height = self.max_y.saturating_sub(self.min_y) + 1;
        let bbox_area = width * height;
        let aspect = width as f32 / height.max(1) as f32;
        let area_ratio = self.area as f32 / image_area;

        if self.area < 1_024 || bbox_area < 4_096 || area_ratio < 0.01 {
            return None;
        }

        if !(0.45..=1.8).contains(&aspect) {
            return None;
        }

        let squareness_penalty = (aspect - 1.0).abs();
        let score = (self.area as f32) / (1.0 + squareness_penalty * 4.0);

        Some(DetectionCandidate {
            corners: [self.top_left, self.top_right, self.bottom_right, self.bottom_left],
            score,
            area: self.area,
        })
    }
}
