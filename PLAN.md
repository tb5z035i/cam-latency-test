# Camera Latency GUI Plan

## Goals

Build a Rust desktop application that:

1. shows a rapidly changing, time-encoded pattern on the left,
2. shows the live camera image on the right,
3. automatically locates the displayed pattern in the camera image,
4. decodes the delayed pattern from one captured image,
5. reports instant latency and average latency,
6. provides best-effort monitor-response / refresh uncertainty handling,
7. supports extensible cameras through an external adapter protocol.

## Constraints

- Cross-platform, at minimum:
  - Linux amd64
  - macOS arm64
- Avoid non-OS third-party runtime dependencies in the default build.
- Keep the UI in a single maximizable window.
- ROI detection must be automatic.
- Preview and displayed pattern should be best-effort realtime.
- Latency analysis may run asynchronously.
- The repository must also include a separate RealSense adapter example.

## Core technical approach

### Pattern design

Use a custom QR-inspired marker:

- thick outer border,
- asymmetric finder/orientation markers,
- black/white reference regions,
- Gray-coded timestamp payload,
- duplicated payload copies,
- checksum/parity bits.

This makes the same pattern usable for:

- locating,
- orienting,
- perspective rectification,
- timestamp decoding.

### Latency model

- Drive a logical timestamp state with a monotonic timer.
- Encode the logical state into the displayed pattern.
- Decode the delayed state from the camera frame.
- Compute latency as modular state difference:
  - 1 ms logical resolution
  - support at least 5 s latency
  - state space >= 50,000 (target: 131,072 states / 17 bits)

### Auto ROI

Detection pipeline:

1. grayscale / downsample,
2. threshold / normalize,
3. finder-pattern search or dominant border component detection,
4. candidate geometry assembly,
5. homography estimation,
6. rectification,
7. decode + checksum validation,
8. reuse last good ROI until confidence drops,
9. show persistent warning if auto-detection keeps failing.

### Realtime split

- Display path: render pattern continuously.
- Preview path: show latest camera frame continuously.
- Analysis path: consume latest frame only and drop stale work.

## Camera architecture

### Default backend

- Native/UVC camera support using Rust crates over OS-native camera APIs.

### External adapter protocol

Primary extensibility mechanism:

- out-of-process adapters,
- v1 transport: local `stdio`,
- language-agnostic,
- adapter provides:
  - handshake,
  - enumerate sources,
  - open/start/stop,
  - frame stream,
  - optional metadata and controls.

### Example adapters

- `adapters/adapter-stub/`: generic template
- `adapters/realsense-example/`: separate RealSense protocol example

The RealSense example must remain outside the main app dependency path.

## UI

Single window layout:

- left: generated pattern
- right: live preview with detection overlay
- bottom: controls, status, latency, averages, uncertainty, warnings

## Calibration / monitor handling

Best-effort only:

- report raw latency,
- report corrected latency heuristic,
- report uncertainty,
- surface assumptions and limitations in the UI and README.

## Planned modules

- `src/main.rs`
- `src/lib.rs`
- `src/app.rs`
- `src/pattern.rs`
- `src/detect.rs`
- `src/decoder.rs`
- `src/measurement.rs`
- `src/calibration.rs`
- `src/camera/mod.rs`
- `src/camera/native.rs`
- `src/camera/bridge.rs`
- `src/camera/protocol.rs`

## Validation

### Automated

- Gray-code roundtrip
- checksum validation
- modular latency math
- synthetic decode / warped image tests

### Manual

- run GUI,
- start native camera,
- point camera at left pattern,
- confirm auto-detection,
- confirm instant + average latency,
- confirm warning behavior on persistent detection failure,
- confirm adapter discovery / launch path.
