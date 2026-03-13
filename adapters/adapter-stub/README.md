# Camera Adapter Stub

This is a minimal reference implementation of the camera adapter protocol used by the main
latency GUI.

## What it does

- speaks the same line-delimited JSON protocol as the main app expects over `stdio`
- exposes one synthetic camera source
- emits a self-locating timestamp pattern so the main app can exercise preview, ROI detection, and
  protocol integration without external hardware

## Important note

This stub is a **protocol/demo adapter**, not a physical latency reference. Its timestamp origin is
independent from the main app process, so the reported latency number is not meaningful as a real
camera measurement.

## Run

```bash
cargo run --manifest-path adapters/adapter-stub/Cargo.toml
```

Then point the main app at the adapter command:

```bash
target/debug/camera_adapter_stub
```
