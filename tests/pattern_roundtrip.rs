use cam_latency_test::pattern::{
    decode_payload, from_gray, payload_bits, to_gray, PatternMode, STATE_SPACE,
};

#[test]
fn gray_code_round_trip_holds() {
    for value in [0u32, 1, 2, 3, 17, 255, 4096, 65_535, STATE_SPACE - 1] {
        assert_eq!(from_gray(to_gray(value)), value);
    }
}

#[test]
fn payload_round_trip_holds() {
    for value in [0u32, 7, 31, 255, 1024, 42_424, STATE_SPACE - 2] {
        let bits = payload_bits(value);
        assert_eq!(decode_payload(&bits), Some(value));
    }
}

#[test]
fn payload_checksum_rejects_corruption() {
    let mut bits = payload_bits(12_345);
    bits[4] = !bits[4];
    assert_eq!(decode_payload(&bits), None);
}

#[test]
fn render_modes_share_same_state_space() {
    let timestamp = cam_latency_test::pattern::render_pattern(256, 123, PatternMode::Timestamp);
    let calibration = cam_latency_test::pattern::render_pattern(256, 123, PatternMode::Calibration);
    assert_eq!(timestamp.dimensions(), calibration.dimensions());
}
