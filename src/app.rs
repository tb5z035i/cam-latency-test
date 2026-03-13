use crate::{
    calibration::CalibrationState,
    camera::{self, ActiveCamera, CameraDescriptor, CameraOrigin},
    decoder::{decode_image, DecodeResult},
    measurement::MeasurementStats,
    pattern::{render_frame, PatternMode, PatternRuntime},
};
use crossbeam_channel::{bounded, Receiver, Sender};
use eframe::egui::{self, ColorImage, Context, TextureHandle, TextureOptions};
use image::RgbImage;
use std::{
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct LatencyApp {
    pattern_runtime: PatternRuntime,
    pattern_texture: Option<TextureHandle>,
    camera_texture: Option<TextureHandle>,
    preview_image: Option<RgbImage>,
    available_sources: Vec<CameraDescriptor>,
    selected_source: Option<String>,
    active_camera: Option<ActiveCamera>,
    analysis_worker: AnalysisWorker,
    calibration: CalibrationState,
    measurements: MeasurementStats,
    adapter_command: String,
    status_message: String,
    last_decode_success: Option<Instant>,
    preview_rate: RateCounter,
    analysis_rate: RateCounter,
}

impl LatencyApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            pattern_runtime: PatternRuntime::new(),
            pattern_texture: None,
            camera_texture: None,
            preview_image: None,
            available_sources: Vec::new(),
            selected_source: None,
            active_camera: None,
            analysis_worker: AnalysisWorker::new(),
            calibration: CalibrationState::default(),
            measurements: MeasurementStats::default(),
            adapter_command: String::new(),
            status_message: String::new(),
            last_decode_success: None,
            preview_rate: RateCounter::default(),
            analysis_rate: RateCounter::default(),
        };
        app.refresh_native_sources();
        app
    }

    fn refresh_native_sources(&mut self) {
        let previous_selection = self.selected_source.clone();
        let mut sources = self
            .available_sources
            .iter()
            .filter(|source| matches!(source.origin, CameraOrigin::Adapter { .. }))
            .cloned()
            .collect::<Vec<_>>();

        match camera::list_native_sources() {
            Ok(mut native) => {
                sources.splice(0..0, native.drain(..));
                self.status_message = format!("Found {} native camera(s).", sources.len());
            }
            Err(error) => {
                self.status_message = format!("Failed to enumerate cameras: {error:#}");
            }
        }

        self.available_sources = sources;
        self.selected_source = previous_selection
            .filter(|selected| self.available_sources.iter().any(|source| &source.id == selected))
            .or_else(|| self.available_sources.first().map(|source| source.id.clone()));
    }

    fn probe_adapter(&mut self) {
        if self.adapter_command.trim().is_empty() {
            self.status_message = "Enter an adapter command first.".to_owned();
            return;
        }

        match camera::probe_adapter(self.adapter_command.trim()) {
            Ok(mut sources) => {
                self.available_sources.retain(|source| !matches!(source.origin, CameraOrigin::Adapter { .. }));
                self.available_sources.append(&mut sources);
                self.status_message = "Adapter probed successfully.".to_owned();
                if self.selected_source.is_none() {
                    self.selected_source = self.available_sources.first().map(|source| source.id.clone());
                }
            }
            Err(error) => {
                self.status_message = format!("Adapter probe failed: {error:#}");
            }
        }
    }

    fn start_selected_camera(&mut self) {
        self.stop_camera();

        let Some(source_id) = self.selected_source.clone() else {
            self.status_message = "Select a camera source first.".to_owned();
            return;
        };
        let Some(source) = self
            .available_sources
            .iter()
            .find(|source| source.id == source_id)
            .cloned()
        else {
            self.status_message = "Selected camera source was not found.".to_owned();
            return;
        };

        match camera::open_camera(&source) {
            Ok(stream) => {
                self.active_camera = Some(stream);
                self.measurements.reset();
                self.preview_image = None;
                self.status_message = format!("Streaming {}", source.name);
            }
            Err(error) => {
                self.status_message = format!("Failed to start camera: {error:#}");
            }
        }
    }

    fn stop_camera(&mut self) {
        if let Some(camera) = self.active_camera.take() {
            camera.stop();
        }
    }

    fn current_pattern_mode(&self) -> PatternMode {
        if self.calibration.enabled {
            PatternMode::Calibration
        } else {
            PatternMode::Timestamp
        }
    }

    fn poll_camera(&mut self) {
        let Some(stream) = self.active_camera.as_ref() else {
            return;
        };

        if let Some(packet) = stream.try_recv() {
            if let Some(preview) = packet.clone().into_rgb_image() {
                self.preview_rate.tick();
                self.preview_image = Some(preview.clone());
                let request = AnalysisRequest {
                    image: preview,
                    display_state: self.pattern_runtime.current_state(),
                    mode: self.current_pattern_mode(),
                };
                self.analysis_worker.submit(request);
            }
        }

        while let Some(result) = self.analysis_worker.try_recv() {
            self.analysis_rate.tick();
            self.preview_image = Some(result.overlay.clone());
            self.handle_analysis_result(result);
        }
    }

    fn handle_analysis_result(&mut self, result: AnalysisResult) {
        if let Some(decoded) = result.decode {
            self.last_decode_success = Some(Instant::now());
            let calibration_estimate = self.calibration.estimate();
            if self
                .measurements
                .push(
                    result.display_state,
                    decoded.state,
                    decoded.confidence,
                    calibration_estimate,
                )
                .is_none()
            {
                self.measurements.reject();
            }

            self.calibration.edge_contrast_score =
                (self.calibration.edge_contrast_score * 0.85) + (decoded.calibration_match * 0.15);
            self.status_message = format!(
                "Detected target. decode={:.2} locate={:.2}",
                decoded.static_score, decoded.detection_score
            );
        } else {
            self.measurements.reject();
        }
    }

    fn update_pattern_texture(&mut self, ctx: &Context) {
        let frame = render_frame(720, self.pattern_runtime.current_state(), self.current_pattern_mode());
        update_texture(
            ctx,
            &mut self.pattern_texture,
            "pattern_texture",
            color_image_from_rgb(&frame.image),
        );
    }

    fn update_camera_texture(&mut self, ctx: &Context) {
        if let Some(image) = &self.preview_image {
            update_texture(
                ctx,
                &mut self.camera_texture,
                "camera_texture",
                color_image_from_rgb(image),
            );
        }
    }

    fn detection_warning(&self) -> Option<String> {
        if self.active_camera.is_none() {
            return None;
        }

        match self.last_decode_success {
            Some(instant) if instant.elapsed() < Duration::from_secs(2) => None,
            _ => Some(
                "Automatic ROI detection is failing repeatedly. Re-aim the camera or enlarge the window."
                    .to_owned(),
            ),
        }
    }
}

impl Drop for LatencyApp {
    fn drop(&mut self) {
        self.stop_camera();
    }
}

impl eframe::App for LatencyApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll_camera();
        self.update_pattern_texture(ctx);
        self.update_camera_texture(ctx);

        egui::TopBottomPanel::bottom("status_panel")
            .resizable(true)
            .default_height(210.0)
            .show(ctx, |ui| {
                ui.heading("Camera latency status");
                if let Some(warning) = self.detection_warning() {
                    ui.colored_label(egui::Color32::YELLOW, warning);
                }
                ui.label(&self.status_message);
                ui.separator();

                ui.horizontal_wrapped(|ui| {
                    if ui.button("Refresh native cameras").clicked() {
                        self.refresh_native_sources();
                    }
                    if ui
                        .button(if self.active_camera.is_some() {
                            "Stop stream"
                        } else {
                            "Start stream"
                        })
                        .clicked()
                    {
                        if self.active_camera.is_some() {
                            self.stop_camera();
                        } else {
                            self.start_selected_camera();
                        }
                    }
                    if ui.button("Reset average").clicked() {
                        self.measurements.reset();
                    }
                    ui.checkbox(&mut self.calibration.enabled, "Calibration mode");
                    ui.add(
                        egui::DragValue::new(&mut self.calibration.assumed_refresh_hz)
                            .clamp_range(24.0..=360.0)
                            .speed(1.0)
                            .prefix("Refresh Hz "),
                    );
                });

                egui::ComboBox::from_label("Camera source")
                    .selected_text(
                        self.selected_source
                            .as_ref()
                            .and_then(|id| self.available_sources.iter().find(|source| &source.id == id))
                            .map(|source| source.name.clone())
                            .unwrap_or_else(|| "None".to_owned()),
                    )
                    .show_ui(ui, |ui| {
                        for source in &self.available_sources {
                            ui.selectable_value(
                                &mut self.selected_source,
                                Some(source.id.clone()),
                                source.name.clone(),
                            );
                        }
                    });

                ui.horizontal(|ui| {
                    ui.label("Adapter command:");
                    ui.text_edit_singleline(&mut self.adapter_command);
                    if ui.button("Probe adapter").clicked() {
                        self.probe_adapter();
                    }
                });

                ui.separator();
                let calibration = self.calibration.estimate();
                ui.label(format!(
                    "Instant latency: {}",
                    self.measurements
                        .instant
                        .map(|sample| format!("{:.1} ms", sample.latency_ms))
                        .unwrap_or_else(|| "n/a".to_owned())
                ));
                ui.label(format!(
                    "Average latency: {}",
                    self.measurements
                        .average_latency_ms
                        .map(|value| format!("{value:.1} ms"))
                        .unwrap_or_else(|| "n/a".to_owned())
                ));
                ui.label(format!(
                    "Corrected latency: {}",
                    self.measurements
                        .corrected_latency_ms
                        .map(|value| format!("{value:.1} ms"))
                        .unwrap_or_else(|| "n/a".to_owned())
                ));
                ui.label(format!(
                    "Corrected average: {}",
                    self.measurements
                        .corrected_average_latency_ms
                        .map(|value| format!("{value:.1} ms"))
                        .unwrap_or_else(|| "n/a".to_owned())
                ));
                ui.label(format!(
                    "Uncertainty: ±{:.1} ms | Samples: {} | Dropped: {}",
                    calibration.uncertainty_ms,
                    self.measurements.sample_count,
                    self.measurements.dropped_samples
                ));
                ui.label(format!(
                    "Preview FPS {:.1} | Analysis FPS {:.1}",
                    self.preview_rate.fps,
                    self.analysis_rate.fps
                ));
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |columns| {
                columns[0].heading("Encoded display pattern");
                if let Some(texture) = &self.pattern_texture {
                    let available = columns[0].available_size();
                    columns[0].image(texture.id(), available);
                } else {
                    columns[0].label("Pattern not ready");
                }

                columns[1].heading("Captured camera view");
                if let Some(texture) = &self.camera_texture {
                    let available = columns[1].available_size();
                    columns[1].image(texture.id(), available);
                } else {
                    columns[1].label("No camera preview");
                }
            });
        });

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

#[derive(Clone)]
struct AnalysisRequest {
    image: RgbImage,
    display_state: u32,
    mode: PatternMode,
}

struct AnalysisResult {
    decode: Option<DecodeResult>,
    overlay: RgbImage,
    display_state: u32,
}

struct AnalysisWorker {
    tx: Sender<AnalysisRequest>,
    rx: Receiver<AnalysisResult>,
    _join_handle: JoinHandle<()>,
}

impl AnalysisWorker {
    fn new() -> Self {
        let (request_tx, request_rx) = bounded(1);
        let (result_tx, result_rx) = bounded(2);
        let join_handle = thread::spawn(move || analysis_loop(request_rx, result_tx));
        Self {
            tx: request_tx,
            rx: result_rx,
            _join_handle: join_handle,
        }
    }

    fn submit(&self, request: AnalysisRequest) {
        let _ = self.tx.try_send(request);
    }

    fn try_recv(&self) -> Option<AnalysisResult> {
        let mut latest = None;
        while let Ok(result) = self.rx.try_recv() {
            latest = Some(result);
        }
        latest
    }
}

fn analysis_loop(request_rx: Receiver<AnalysisRequest>, result_tx: Sender<AnalysisResult>) {
    while let Ok(request) = request_rx.recv() {
        let decoded = decode_image(&request.image, request.mode);
        let overlay = decoded
            .as_ref()
            .map(|decode| decode.overlay.clone())
            .unwrap_or_else(|| request.image.clone());
        let _ = result_tx.try_send(AnalysisResult {
            decode: decoded,
            overlay,
            display_state: request.display_state,
        });
    }
}

#[derive(Default)]
struct RateCounter {
    last_window_start: Option<Instant>,
    count: u32,
    fps: f32,
}

impl RateCounter {
    fn tick(&mut self) {
        let now = Instant::now();
        let start = self.last_window_start.get_or_insert(now);
        self.count += 1;
        let elapsed = start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.fps = self.count as f32 / elapsed.as_secs_f32();
            self.count = 0;
            self.last_window_start = Some(now);
        }
    }
}

fn color_image_from_rgb(image: &RgbImage) -> ColorImage {
    ColorImage::from_rgb([image.width() as usize, image.height() as usize], image.as_raw())
}

fn update_texture(
    ctx: &Context,
    slot: &mut Option<TextureHandle>,
    name: &str,
    color_image: ColorImage,
) {
    if let Some(texture) = slot {
        texture.set(color_image, TextureOptions::NEAREST);
    } else {
        *slot = Some(ctx.load_texture(name.to_owned(), color_image, TextureOptions::NEAREST));
    }
}
