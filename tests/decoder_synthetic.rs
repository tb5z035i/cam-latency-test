use cam_latency_test::{
    decoder::decode_image,
    pattern::{render_pattern, PatternMode},
};
use image::{imageops, Rgb, RgbImage};
use imageproc::geometric_transformations::{warp_into, Interpolation, Projection};

#[test]
fn decoder_reads_direct_pattern() {
    let image = render_pattern(600, 3_210, PatternMode::Timestamp);
    let decoded = decode_image(&image, PatternMode::Timestamp).expect("pattern should decode");
    assert_eq!(decoded.state, 3_210);
    assert!(
        decoded.confidence > 0.6,
        "confidence was {}",
        decoded.confidence
    );
}

#[test]
fn decoder_reads_rotated_pattern() {
    let image = render_pattern(600, 7_654, PatternMode::Timestamp);
    let rotated = imageops::rotate90(&image);
    let decoded =
        decode_image(&rotated, PatternMode::Timestamp).expect("rotated pattern should decode");
    assert_eq!(decoded.state, 7_654);
}

#[test]
fn decoder_reads_perspective_warped_pattern() {
    let image = render_pattern(500, 12_345, PatternMode::Timestamp);
    let mut canvas = RgbImage::from_pixel(900, 900, Rgb([255, 255, 255]));
    let projection = Projection::from_control_points(
        [(0.0, 0.0), (499.0, 0.0), (499.0, 499.0), (0.0, 499.0)],
        [
            (120.0, 110.0),
            (760.0, 150.0),
            (700.0, 760.0),
            (180.0, 700.0),
        ],
    )
    .expect("projection should build");
    warp_into(
        &image,
        &projection,
        Interpolation::Nearest,
        Rgb([255, 255, 255]),
        &mut canvas,
    );

    let decoded =
        decode_image(&canvas, PatternMode::Timestamp).expect("warped pattern should decode");
    assert_eq!(decoded.state, 12_345);
}
