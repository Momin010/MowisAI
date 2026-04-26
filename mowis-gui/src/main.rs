mod app;
mod backend;
mod platform;
mod theme;
mod types;
mod views;
mod widgets;

use app::MowisApp;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MowisAI")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "MowisAI",
        options,
        Box::new(|cc| Ok(Box::new(MowisApp::new(cc)))),
    )
}

fn load_icon() -> egui::IconData {
    // Placeholder 1×1 transparent icon; replace with real asset when available.
    egui::IconData {
        rgba: vec![0, 0, 0, 0],
        width: 1,
        height: 1,
    }
}
