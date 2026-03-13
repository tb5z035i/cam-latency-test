use cam_latency_test::camera::{open_camera, probe_adapter};
use std::{thread, time::Duration};

#[test]
fn adapter_stub_can_be_probed_and_stream_frames() {
    let command = "cargo run --manifest-path adapters/adapter-stub/Cargo.toml --quiet";
    let sources = probe_adapter(command).expect("adapter probe should succeed");
    assert!(!sources.is_empty(), "adapter returned no sources");

    let stream = open_camera(&sources[0]).expect("adapter stream should open");
    let mut received = None;

    for _ in 0..40 {
        if let Some(frame) = stream.try_recv() {
            received = Some(frame);
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    stream.stop();

    let frame = received.expect("adapter should emit at least one frame");
    assert!(frame.width > 0);
    assert!(frame.height > 0);
    assert!(!frame.data.is_empty());
}
