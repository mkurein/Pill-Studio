//! Pill Studio — native desktop studio (egui) for the Blue Pill (STM32F103C8T)
//! and the Black Pill (STM32F411CEU6)
//! firmware in this repository: MCU reference, board, register browser with
//! live SWD reads, UART protocol console, FreeRTOS monitor, fault analysis.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod canvas;
mod core;
mod fmt;
mod shots;
mod theme;
mod views;
mod widgets;
mod worker;

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn main() -> eframe::Result {
    let mut viewport = eframe::egui::ViewportBuilder::default();
    match eframe::icon_data::from_png_bytes(ICON_PNG) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => eprintln!("app icon not loaded: {e}"),
    }
    let options = eframe::NativeOptions {
        viewport: viewport
            .with_title("Pill Studio")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1100.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native("Pill Studio", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
