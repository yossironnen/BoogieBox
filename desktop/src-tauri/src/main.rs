//! Defines Tauri desktop shell logic for Main.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    boogiebox_desktop_lib::run()
}
