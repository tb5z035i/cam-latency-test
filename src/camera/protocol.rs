use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    Rgb8,
    Gray8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdapterSourceInfo {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub nominal_fps: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdapterCommand {
    Hello { protocol_version: u32 },
    ListSources,
    Open { source_id: String },
    Start,
    Stop,
    Close,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdapterFrame {
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub sequence: u64,
    pub timestamp_millis: Option<u64>,
    pub data_base64: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdapterMessage {
    Hello {
        protocol_version: u32,
        adapter_name: String,
        adapter_version: String,
    },
    Sources {
        sources: Vec<AdapterSourceInfo>,
    },
    Ack {
        message: String,
    },
    Error {
        message: String,
    },
    Frame {
        frame: AdapterFrame,
    },
}
