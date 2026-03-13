# RealSense Adapter Example

This example shows how to implement the camera adapter protocol for an Intel RealSense device as a
**separate project** from the main latency GUI.

## Purpose

This example is intentionally not part of the main app dependency path. It demonstrates:

- adapter handshake
- source enumeration
- source selection by serial number
- RealSense color streaming
- packaging frames into the adapter protocol

## Requirements

- `librealsense2`
- a RealSense device supported by `realsense-rust`
- any extra platform setup required by the RealSense SDK

## Run

```bash
cargo run --manifest-path adapters/realsense-example/Cargo.toml
```

Then use the resulting executable path as the adapter command in the main app.

## Notes

- This example is a reference implementation, not a mandatory dependency of the main tool.
- The main app remains buildable and runnable without RealSense installed.
