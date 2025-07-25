#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod file_system;

use crate::app::FileManager;
use eframe::{egui, NativeOptions};
use std::sync::mpsc;
use std::thread;
use tokio::runtime::Runtime;

fn main() {
    let (tx, rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    let rt = Runtime::new().expect("Failed to create Tokio runtime");

    let file_system_handle = rt.handle().clone();
    thread::spawn(move || {
        file_system_handle.block_on(async {
            file_system::watch_directory(tx, event_rx).await;
        });
    });

    let mut native_options = NativeOptions::default();
    native_options.initial_window_size = Some(egui::vec2(800.0, 600.0));
    native_options.min_window_size = Some(egui::vec2(400.0, 300.0));

    eframe::run_native(
        "File Manager",
        native_options,
        Box::new(|_cc| Box::new(FileManager::new(rx, event_tx))),
    );
}
