mod animation;
mod app;
mod auth;
mod backend;
mod connection;
mod connections;
mod launcher;
mod launchers;
mod platform;
mod resources;
mod theme;
mod types;
mod views;
mod widgets;

use app::MowisApp;

fn has_display() -> bool {
    std::env::var("DISPLAY").is_ok()
        || std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("WAYLAND_SOCKET").is_ok()
        // macOS and Windows always have a display
        || cfg!(target_os = "macos")
        || cfg!(target_os = "windows")
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if !has_display() {
        eprintln!("MowisAI GUI requires a display server (X11 or Wayland).");
        eprintln!();
        eprintln!("You appear to be running in a headless environment (e.g. Cloud Shell).");
        eprintln!("Options:");
        eprintln!("  • Run on your local machine (Linux/Mac/Windows) where a display is available");
        eprintln!("  • Use the terminal UI instead:  agentd");
        eprintln!();
        eprintln!("On Linux with X11, ensure DISPLAY is set, e.g.:");
        eprintln!("  export DISPLAY=:0");
        eprintln!("  mowisai");
        std::process::exit(1);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MowisAI")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "MowisAI",
        options,
        Box::new(|cc| Ok(Box::new(MowisApp::new(cc)))),
    ) {
        eprintln!("Failed to launch GUI: {}", e);
        eprintln!();
        eprintln!("If you're on Linux, make sure a display server is running:");
        eprintln!("  X11:    export DISPLAY=:0");
        eprintln!("  Wayland: export WAYLAND_DISPLAY=wayland-0");
        eprintln!();
        eprintln!("For headless/server environments, use the terminal UI: agentd");
        std::process::exit(1);
    }
}

fn load_icon() -> egui::IconData {
    egui::IconData {
        rgba: vec![0, 0, 0, 0],
        width: 1,
        height: 1,
    }
}
