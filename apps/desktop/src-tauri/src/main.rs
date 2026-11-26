#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if mylodb_desktop_lib::is_background_run() {
        mylodb_desktop_lib::run_due_jobs()
    } else {
        mylodb_desktop_lib::run()
    }
}
