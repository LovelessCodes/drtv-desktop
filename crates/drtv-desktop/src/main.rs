//! The binary. Everything it does is window setup: the app itself lives in `drtv-app` so it can be
//! tested without a display.

fn main() -> eframe::Result {
    drtv_app::config::init_tracing();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DRTV")
            .with_app_id("drtv-desktop")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1024.0, 700.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "DRTV",
        options,
        Box::new(|cc| Ok(Box::new(drtv_app::DrtvApp::new(cc)))),
    )
}
