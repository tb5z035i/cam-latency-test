use cam_latency_test::app::LatencyApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(eframe::egui::vec2(1500.0, 920.0)),
        maximized: true,
        ..Default::default()
    };

    eframe::run_native(
        "Camera Latency Test Tool",
        options,
        Box::new(|cc| Box::new(LatencyApp::new(cc))),
    )
}
