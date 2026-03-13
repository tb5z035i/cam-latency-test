# Camera Latency Test Tool

Rust GUI tool for estimating camera-system latency from a self-locating time code shown on-screen.

## What it does

- renders a QR-inspired self-locating timestamp pattern on the **left**
- shows the **captured camera image** on the **right**
- automatically locates the displayed target in the camera image
- decodes the delayed timestamp from a single captured frame
- reports:
  - **instant latency**
  - **average latency**
  - **best-effort corrected latency**
  - **uncertainty**

## Measurement model

The tool does **not** depend on synchronizing a camera timestamp with a display timestamp.

Instead:

1. the left panel displays a spatial timestamp code,
2. the camera captures an older version of that code,
3. the decoder reads the delayed state from the captured image,
4. latency is computed as the modular difference between the current displayed state and the
   decoded captured state.

The state space is 17-bit Gray code (`131072` states), which comfortably exceeds the requested
`50000` states and supports the required `5 s` latency window at `1 ms` logical resolution.

## Auto ROI

The displayed marker is designed to work like a custom QR-inspired code:

- thick outer border
- asymmetric finder markers
- orientation marker
- duplicated payload bits
- checksum bits

This lets the same pattern provide:

- localization
- orientation
- perspective rectification
- timestamp decode

If auto-detection keeps failing, the GUI surfaces a persistent warning.

## Current camera support

### Built in

- native camera backend through `ccap-rs`

### Extensible

- external adapter protocol over **local stdio**
- user-provided adapters can be written in Rust, C++, Python, etc.

The repository includes:

- `adapters/adapter-stub/` — minimal synthetic protocol reference
- `adapters/realsense-example/` — separate Intel RealSense adapter example

These adapters are intentionally kept **outside the main app build path**.

## Build

```bash
cargo run
```

### Linux build note

The native camera backend currently builds the vendored `ccap` C++ source. The repo includes:

- `.cargo/config.toml` setting `CXX=g++`
- Linux linker override to `g++`

This keeps local Linux builds working in the provided environment.

## Test

```bash
cargo test --tests
```

## CI / release automation

The repository includes GitHub Actions workflows for:

- **pushes to `main`**: build release packages for:
  - Linux amd64
  - macOS arm64
- **tag pushes** (for example `v0.1.0`): build the same packages, create a GitHub release, and
  upload the packaged binaries as release assets

The release packages currently include:

- `cam_latency_test`
- `camera_adapter_stub`

The separate `adapters/realsense-example/` project is **not** built in CI release packaging
because it requires the external `librealsense2` SDK/runtime.

## Adapter protocol overview

The app expects line-delimited JSON over `stdio`.

### Commands

- `hello`
- `list_sources`
- `open`
- `start`
- `stop`
- `close`

### Messages

- `hello`
- `sources`
- `ack`
- `error`
- `frame`

Each `frame` message includes:

- width
- height
- pixel format
- sequence number
- optional timestamp
- base64-encoded pixel payload

## Limitations

- The tool provides **1 ms logical coding**, but real-world physical accuracy is still limited by:
  - display refresh
  - display scanout
  - panel response time
  - camera exposure
  - shutter behavior
- The corrected latency and uncertainty are **best-effort** heuristics, not hardware-grade
  calibration.
- The adapter stub is a protocol/demo source, not a physical latency reference.